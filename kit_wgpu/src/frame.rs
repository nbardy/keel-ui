//! A UI frame owns one viewport and the CPU text-frame lifetime. After queue.submit,
//! call UiFrame::submitted with its submission index, even if record returned an error.
//! Dropping without submission abandons the frame and keeps its atlas pages referenced.

use crate::{WgpuSurface, WgpuTarget, WgpuUi};
use kit_core::Viewport;
use kit_paint::{DrawList, RenderError};
use kit_view::CosmicText;

/// Where a frame draws: the scene's own encoder and colour target. The device-pixel scale comes from the frame's
/// viewport, never from the caller, so layout and drawing cannot disagree on it.
pub struct FrameTarget<'a> {
    pub encoder: &'a mut wgpu::CommandEncoder,
    pub view: &'a wgpu::TextureView,
    pub format: wgpu::TextureFormat,
    /// Physical pixels.
    pub size: (u32, u32),
}

/// A UI frame in flight. Call `submitted` after the submit carrying its commands,
/// including a partial recording that returned an error. Dropping abandons it:
/// CPU text cleanup still runs, but atlas pages remain referenced until a later submit
/// covers all outstanding recordings. Never submit an abandoned encoder after that fence.
#[must_use = "call submitted after queue.submit; dropping abandons the frame"]
pub struct UiFrame<'a> {
    gpu: &'a mut WgpuUi,
    text: &'a mut CosmicText,
    viewport: Viewport,
    submitted: bool,
}

impl WgpuUi {
    /// Begin a frame at `viewport` (logical size and device scale).
    pub fn begin_frame<'a>(
        &'a mut self,
        text: &'a mut CosmicText,
        viewport: Viewport,
    ) -> UiFrame<'a> {
        text.begin_frame();
        UiFrame {
            gpu: self,
            text,
            viewport,
            submitted: false,
        }
    }
}

impl UiFrame<'_> {
    /// Consume this frame after queue.submit, even when record returned an error.
    /// The submission must cover every encoder recorded since the preceding fence.
    pub fn submitted(mut self, _submission: wgpu::SubmissionIndex) {
        self.gpu.frame_submitted();
        self.submitted = true;
    }
    pub fn viewport(&self) -> Viewport {
        self.viewport
    }
    /// The text system, for the layout engine to measure with.
    pub fn text(&mut self) -> &mut CosmicText {
        self.text
    }
    /// Record `list` over `target`'s existing pixels.
    pub fn record(&mut self, list: &DrawList, target: FrameTarget<'_>) -> Result<(), RenderError> {
        let surface = WgpuSurface {
            view: target.view.clone(),
            format: target.format,
            size: target.size,
            scale_factor: self.viewport.scale_factor as f32,
        };
        self.gpu.render(
            list,
            self.text,
            WgpuTarget {
                encoder: target.encoder,
                surface: &surface,
            },
        )
    }
}

impl Drop for UiFrame<'_> {
    fn drop(&mut self) {
        if !self.submitted {
            self.gpu.abandoned_frames += 1;
        }
        self.text.end_frame();
    }
}
