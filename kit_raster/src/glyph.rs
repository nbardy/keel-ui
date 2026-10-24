//! Glyph pixels -> pixmap tiles. `CosmicText::rasterize` is the only source of glyph pixels.
use crate::convert::unsupported;
use crate::layers::blank;
use kit_paint::RenderError;
use kit_view::kit_text::{GlyphImage, GlyphMask};
use tiny_skia::{IntSize, Paint, Pixmap, Rect, Transform};

fn scale255(v: u8, coverage: u8) -> u8 {
    ((u16::from(v) * u16::from(coverage) + 127) / 255) as u8
}

/// A `width x height` tile of the glyph: coverage masks are inked with `ink` (whose shader is in device coordinates
/// relative to the tile origin); colour glyphs are used as-is.
pub fn tile(image: &GlyphImage, ink: &Paint<'_>) -> Result<Pixmap, RenderError> {
    let (w, h) = (image.width, image.height);
    let pixels = w as usize * h as usize;
    let per_pixel = match image.mask {
        GlyphMask::Alpha => 1,
        GlyphMask::Color => 4,
    };
    if pixels == 0 || image.data.len() != pixels * per_pixel {
        return Err(unsupported(
            "glyph image whose data does not match its size",
        ));
    }
    match image.mask {
        GlyphMask::Alpha => {
            let mut tile = blank(w, h)?;
            let full = Rect::from_xywh(0.0, 0.0, w as f32, h as f32)
                .ok_or_else(|| unsupported("glyph tile rect"))?;
            tile.fill_rect(full, ink, Transform::identity(), None);
            for (px, coverage) in tile.data_mut().chunks_exact_mut(4).zip(&image.data) {
                for channel in px {
                    *channel = scale255(*channel, *coverage);
                }
            }
            Ok(tile)
        }
        GlyphMask::Color => {
            let size = IntSize::from_wh(w, h).ok_or_else(|| unsupported("glyph tile size"))?;
            Pixmap::from_vec(image.data.clone(), size)
                .ok_or_else(|| unsupported("colour glyph pixels"))
        }
    }
}
