//! One frame of UI as a value: it owns the viewport the frame is laid out and drawn at, the text system's frame
//! lifecycle, and the glyph-atlas fence, so none of the three can be forgotten or disagree.
//!
//! ```ignore
//! let mut frame = ui.begin_frame(&mut text, viewport);           // text.begin_frame()
//! let laid = hud.lay_out(&mut layout, frame.text(), viewport)?;   // the same viewport the renderer will use
//! renderer.render_with(&scene, |overlay| {
//!     frame.record(&list, FrameTarget { encoder: overlay.encoder, view: overlay.view, .. })
//! });                                                            // submitted when this returns
//! drop(frame);                                                   // frame_submitted() + text.end_frame()
//! ```

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

/// A UI frame in flight. Dropping it is the end of the frame: the atlas learns that everything recorded is submitted
/// (idempotent, so a frame the surface skipped is fine) and the text system evicts what it no longer needs. Keep it
/// alive across the scene's submission and drop it after.
#[must_use = "the frame ends (and the atlas fence runs) when it is dropped"]
pub struct UiFrame<'a> {
    gpu: &'a mut WgpuUi,
    text: &'a mut CosmicText,
    viewport: Viewport,
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
        }
    }
}

impl UiFrame<'_> {
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
        self.gpu.frame_submitted();
        self.text.end_frame();
    }
}
