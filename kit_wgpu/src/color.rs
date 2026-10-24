//! Brush colour -> premultiplied shader colour, in the encoding the target expects.
use kit_core::peniko::Color;

/// Straight sRGB components go to an `*Srgb` target as linear values (the hardware re-encodes on write); to any
/// other target they go through unchanged. Decided once from the target format.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub(crate) enum Output {
    Raw,
    Srgb,
}

impl Output {
    pub fn of(format: wgpu::TextureFormat) -> Self {
        if format.is_srgb() {
            Self::Srgb
        } else {
            Self::Raw
        }
    }
    fn channel(self, c: f32) -> f32 {
        match self {
            Self::Raw => c,
            Self::Srgb => {
                if c <= 0.04045 {
                    c / 12.92
                } else {
                    ((c + 0.055) / 1.055).powf(2.4)
                }
            }
        }
    }
    /// Premultiplied RGBA with `opacity` folded into alpha.
    pub fn premul(self, color: Color, opacity: f32) -> [f32; 4] {
        let [r, g, b, a] = color.components;
        let a = a * opacity;
        [
            self.channel(r) * a,
            self.channel(g) * a,
            self.channel(b) * a,
            a,
        ]
    }
}
