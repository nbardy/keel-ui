//! Real-GPU harness shared by the kit_wgpu integration tests: a target texture, a `WgpuUi` over it, and readback.
use kit_paint::{DrawList, RenderError};
use kit_view::CosmicText;
use kit_wgpu::{FrameTarget, WgpuSurface, WgpuUi};

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
pub type Px = [u8; 4];

pub struct Harness {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub ui: WgpuUi,
    texture: wgpu::Texture,
    pub surface: WgpuSurface,
    pub size: (u32, u32),
}

impl Harness {
    pub fn new(w: u32, h: u32, scale: f32) -> Self {
        Self::with_format(w, h, scale, FORMAT)
    }

    pub fn with_format(w: u32, h: u32, scale: f32, format: wgpu::TextureFormat) -> Self {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter =
            pollster::block_on(instance.request_adapter(&Default::default())).expect("GPU adapter");
        let (device, queue) =
            pollster::block_on(adapter.request_device(&Default::default())).expect("device");
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("target"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let surface = WgpuSurface {
            view: texture.create_view(&Default::default()),
            format,
            size: (w, h),
            scale_factor: scale,
        };
        Self {
            ui: WgpuUi::new(device.clone(), queue.clone(), format),
            device,
            queue,
            texture,
            surface,
            size: (w, h),
        }
    }

    /// One encoder: optional clear (the "game scene"), then the UI on top.
    pub fn frame(
        &mut self,
        clear: Option<[f64; 4]>,
        list: &DrawList,
        text: &mut CosmicText,
    ) -> Result<(), RenderError> {
        let mut encoder = self.device.create_command_encoder(&Default::default());
        if let Some([r, g, b, a]) = clear {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("scene"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.surface.view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color { r, g, b, a }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
        }
        let viewport = kit_core::Viewport::new(
            kit_core::kurbo::Size::new(
                f64::from(self.size.0) / f64::from(self.surface.scale_factor),
                f64::from(self.size.1) / f64::from(self.surface.scale_factor),
            ),
            f64::from(self.surface.scale_factor),
        );
        let mut frame = self.ui.begin_frame(text, viewport);
        let result = frame.record(
            list,
            FrameTarget {
                encoder: &mut encoder,
                view: &self.surface.view,
                format: self.surface.format,
                size: self.size,
            },
        );
        frame.submitted(self.queue.submit([encoder.finish()]));
        result
    }

    pub fn read(&self) -> Vec<Px> {
        let (w, h) = self.size;
        let padded = (w * 4).next_multiple_of(256);
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: u64::from(padded * h),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            self.texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: Some(h),
                },
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit([encoder.finish()]);
        let (tx, rx) = std::sync::mpsc::channel();
        buffer.map_async(wgpu::MapMode::Read, .., move |r| {
            tx.send(r).expect("receiver")
        });
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("poll");
        rx.recv().expect("reply").expect("map");
        let data = buffer.get_mapped_range(..).expect("range");
        data.chunks_exact(padded as usize)
            .flat_map(|row| {
                row[..(w * 4) as usize]
                    .chunks_exact(4)
                    .map(|p| [p[0], p[1], p[2], p[3]])
                    .collect::<Vec<_>>()
            })
            .collect()
    }
}
