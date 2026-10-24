//! Seam: the wgpu renderer for `kit_paint::DrawList`. A concrete type with an inherent [`WgpuUi::render`]; nothing is
//! generic over the backend.
//!
//! # Use
//! Prefer [`UiFrame`] (`WgpuUi::begin_frame`), which owns the frame lifecycle below; the raw calls are:
//! ```ignore
//! let mut ui = WgpuUi::new(device.clone(), queue.clone(), format);
//! // after the game scene, same encoder, same texture (existing pixels are kept):
//! ui.render(&list, &mut text, WgpuTarget { encoder: &mut encoder, surface: &surface })?;
//! queue.submit([encoder.finish()]);
//! ui.frame_submitted(); // tells the glyph atlas everything recorded so far is on the GPU timeline
//! ```
//! [`WgpuTarget`] is the caller's encoder, so the UI never submits anything itself and several renders may share one
//! submission.
//!
//! # Atlas lifetime
//! Glyph texels are written with `Queue::write_texture`, which lands before the next submission's command buffers.
//! An atlas page is therefore only recycled once its last use is in a *submitted* encoder: call
//! [`WgpuUi::frame_submitted`] after each `queue.submit`. A page still referenced by an unsubmitted encoder is never
//! evicted; if every page is in that state the render fails with a typed error instead of corrupting earlier draws.
//!
//! # Pipelines (draw order is preserved wherever it is visible; see `plan.rs`)
//! 1. Instanced SDF rounded-rect quads: fill, border, drop shadow, premultiplied.
//! 2. Lyon-tessellated `Fill`/`Stroke` with feathered (egui-style) coverage ramps instead of MSAA; meshes are cached
//!    by exact shape bits and evicted after 120 unused frames.
//! 3. Textured instances: glyph atlas pages (R8 mask / RGBA8 colour) and group composites.
//!
//! # Layers
//! * Rect clip: scissor.
//! * Opacity: a translucent layer with two or more drawing items renders as one offscreen group and is composited
//!   once (group alpha: overlapping children do not show through each other, as on `kit_raster`). A lone item cannot
//!   overlap itself, so it just has `DrawList::effective_opacities` multiplied into its colour. Each group costs one
//!   full-target offscreen pass: many simultaneous fading panels are not free.
//! * Brushes are solid only; a gradient or image brush is `RenderError::UnsupportedBrush`.

mod atlas;
mod color;
mod exec;
mod frame;
mod geom;
mod gpu;
mod image;
mod plan;
mod tess;

pub use exec::FrameStats;
pub use frame::{FrameTarget, UiFrame};

use color::Output;
use exec::{Attachments, Resources};
use gpu::{Gpu, Pipelines};
use kit_paint::{DrawList, RenderError};
use kit_view::CosmicText;
use plan::{PlanEnv, Planner};
use wgpu::util::DeviceExt;

/// The texture the UI draws onto. Existing contents are kept (`LoadOp::Load`).
#[derive(Clone, Debug)]
pub struct WgpuSurface {
    pub view: wgpu::TextureView,
    /// Must equal the format given to [`WgpuUi::new`].
    pub format: wgpu::TextureFormat,
    /// Physical pixels.
    pub size: (u32, u32),
    /// Device px per logical px.
    pub scale_factor: f32,
}

/// A surface plus the encoder to record into, so the UI can follow the game scene in the same command buffer.
pub struct WgpuTarget<'a> {
    pub encoder: &'a mut wgpu::CommandEncoder,
    pub surface: &'a WgpuSurface,
}

pub struct WgpuUi {
    gpu: Gpu,
    pipelines: Pipelines,
    atlas: atlas::Atlas,
    images: image::Images,
    tess: tess::TessCache,
    attachments: Attachments,
    out: Output,
    generation: u64,
    stats: FrameStats,
    /// Frames dropped without explicit submission (including skipped surfaces).
    pub abandoned_frames: u64,
}

impl WgpuUi {
    /// `format` is the colour format of every surface this renderer will draw to.
    pub fn new(device: wgpu::Device, queue: wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let gpu = Gpu::new(device, queue, format);
        Self {
            pipelines: Pipelines::new(&gpu),
            atlas: atlas::Atlas::new(),
            images: image::Images::default(),
            tess: tess::TessCache::new(),
            attachments: Attachments::new(),
            out: Output::of(format),
            generation: 0,
            stats: FrameStats::default(),
            abandoned_frames: 0,
            gpu,
        }
    }

    /// Register immutable straight-alpha sRGB RGBA8 pixels; live registration is idempotent.
    pub fn register_pixels(
        &mut self,
        pixels: &kit_view::ImagePixels,
    ) -> Result<kit_view::Image, RenderError> {
        self.images.register_pixels(&self.gpu, self.out, pixels)
    }

    /// Register a game-owned, filterable, single-sample 2D texture view with TEXTURE_BINDING usage.
    /// `size` is the selected view's mip size. Sampled values must be premultiplied in the target's
    /// blending space (linear for Srgb targets, encoded for Unorm). Retains the view until release;
    /// the game may render new contents between UI passes. Do not sample the current UI attachment.
    pub fn register_texture(
        &mut self,
        view: &wgpu::TextureView,
        size: kit_view::ImageSize,
    ) -> kit_view::Image {
        self.images.register_texture(&self.gpu, view, size)
    }

    /// Release GPU bindings. Recorded command buffers retain their resources; future draws fail.
    pub fn release(&mut self, image: kit_view::Image) -> Result<(), RenderError> {
        self.images.release(image)
    }

    /// Draw-call statistics of the most recent successful render.
    pub fn last_stats(&self) -> FrameStats {
        self.stats
    }

    pub fn cached_meshes(&self) -> usize {
        self.tess.len()
    }

    pub fn atlas_pages(&self) -> usize {
        self.atlas.page_count()
    }

    /// Call after the `queue.submit` that carried every encoder this renderer recorded into so far. Until then the
    /// glyph atlas treats all pages used since the previous call as still referenced.
    pub fn frame_submitted(&mut self) {
        self.atlas.submitted(self.generation);
    }
}

impl WgpuUi {
    /// Records the UI into `target.encoder`, over the existing contents of `target.surface`.
    pub fn render(
        &mut self,
        list: &DrawList,
        text: &mut CosmicText,
        target: WgpuTarget<'_>,
    ) -> Result<(), RenderError> {
        if !list.layers_balanced() {
            return Err(RenderError::UnbalancedLayers);
        }
        let surface = target.surface;
        if surface.format != self.gpu.format {
            return Err(RenderError::Backend(format!(
                "target format {:?} differs from the renderer's {:?}",
                surface.format, self.gpu.format
            )));
        }
        let (w, h) = surface.size;
        if w == 0 || h == 0 {
            return Ok(());
        }
        self.generation += 1;
        self.tess.evict(self.generation);
        let plan = Planner::new(PlanEnv {
            images: &self.images,
            gpu: &self.gpu,
            tess: &mut self.tess,
            atlas: &mut self.atlas,
            text,
            scale: f64::from(surface.scale_factor),
            size: surface.size,
            out: self.out,
            generation: self.generation,
        })
        .plan(list)?;
        self.attachments
            .ensure(&self.gpu, surface.size, plan.groups);
        let (passes, bytes) = exec::lower(plan.passes);
        let device = &self.gpu.device;
        // Fresh per render: several renders may share one submission, so buffers must not be overwritten.
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("kit arena"),
            contents: &bytes,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::INDEX,
        });
        let view: Vec<u8> = [w as f32, h as f32, 0.0, 0.0]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect();
        let view_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("kit view"),
            contents: &view,
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let view_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("kit view"),
            layout: &self.gpu.view_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: view_buffer.as_entire_binding(),
            }],
        });
        self.stats = exec::run(
            target.encoder,
            &Resources {
                pipelines: &self.pipelines,
                atlas: &self.atlas,
                images: &self.images,
                attachments: &self.attachments,
                view_bind: &view_bind,
                buffer: &buffer,
                main_view: &surface.view,
            },
            &passes,
        );
        Ok(())
    }
}
