//! Offscreen compositing groups. The root layer is a private copy of the target, so a failed render never leaves the
//! caller's pixmap half-drawn.
use crate::convert::unsupported;
use kit_paint::RenderError;
use tiny_skia::{Mask, Pixmap, PixmapPaint, Transform};

pub fn blank(width: u32, height: u32) -> Result<Pixmap, RenderError> {
    Pixmap::new(width, height).ok_or_else(|| unsupported("zero-sized or oversized pixmap"))
}

pub fn blank_mask(width: u32, height: u32) -> Result<Mask, RenderError> {
    Mask::new(width, height).ok_or_else(|| unsupported("zero-sized or oversized clip mask"))
}

/// An open group: everything drawn while it is on top lands in `pixmap`, and `pop` composites it onto its parent.
pub struct Layer {
    pub pixmap: Pixmap,
    pub opacity: f32,
    /// `None` is "unclipped" (that is the meaning of `Clip::Unclipped`).
    pub clip: Option<Mask>,
}

pub struct LayerStack {
    base: Pixmap,
    open: Vec<Layer>,
}

impl LayerStack {
    pub fn over(target: &Pixmap) -> Self {
        Self {
            base: target.clone(),
            open: Vec::new(),
        }
    }

    pub fn top(&mut self) -> &mut Pixmap {
        self.open
            .last_mut()
            .map_or(&mut self.base, |layer| &mut layer.pixmap)
    }

    pub fn push(&mut self, layer: Layer) {
        self.open.push(layer);
    }

    pub fn pop(&mut self) -> Result<(), RenderError> {
        let layer = self.open.pop().ok_or(RenderError::UnbalancedLayers)?;
        let paint = PixmapPaint {
            opacity: layer.opacity.clamp(0.0, 1.0),
            ..PixmapPaint::default()
        };
        self.top().draw_pixmap(
            0,
            0,
            layer.pixmap.as_ref(),
            &paint,
            Transform::identity(),
            layer.clip.as_ref(),
        );
        Ok(())
    }

    /// The finished picture; a layer left open is an error.
    pub fn finish(self) -> Result<Pixmap, RenderError> {
        match self.open.is_empty() {
            true => Ok(self.base),
            false => Err(RenderError::UnbalancedLayers),
        }
    }
}
