//! Pure geometry every backend needs and must agree on. Kept here so the CPU and GPU renderers cannot drift apart:
//! glyph pixel placement, the tofu box and the caret bar.
use crate::RenderError;
use kit_core::kurbo::{Point, Rect};
use kit_core::peniko::{Brush, Color};
use kit_view::kit_text::{LayoutGlyph, PhysicalGlyph};
use kit_view::{CosmicText, TextLayoutId};

/// Device-px placement of `glyph` (on a `LayoutRun` whose baseline is `line_y`) for a layout whose top-left is `origin`
/// (logical px) at `scale` device px per logical px. cosmic-text's own `physical`: x is subpixel-binned in quarters
/// (the bin is in the cache key), the baseline truncates to a whole pixel.
pub fn place_glyph(glyph: &LayoutGlyph, line_y: f32, origin: Point, scale: f32) -> PhysicalGlyph {
    let offset = (origin.x as f32 * scale, (origin.y as f32 + line_y) * scale);
    glyph.physical(offset, scale)
}

/// Every glyph of `layout` placed for drawing (see [`place_glyph`]) with its ink: its own span colour, else `brush`.
/// Collected up front because rasterising needs `&mut` on the text system that owns the buffer.
pub fn layout_glyphs(
    text: &CosmicText,
    layout: TextLayoutId,
    origin: Point,
    scale: f32,
    brush: &Brush,
) -> Result<Vec<(PhysicalGlyph, Brush)>, RenderError> {
    let buffer = text
        .buffer(layout)
        .ok_or(RenderError::TextNotLive(layout))?;
    let mut out = Vec::new();
    for run in buffer.layout_runs() {
        for g in run.glyphs {
            let ink = match g.color_opt {
                None => brush.clone(),
                Some(c) => Brush::Solid(Color::from_rgba8(c.r(), c.g(), c.b(), c.a())),
            };
            out.push((place_glyph(g, run.line_y, origin, scale), ink));
        }
    }
    Ok(out)
}

/// Font size of a rasterised glyph in device px (the cache key stores it as bits).
pub fn em(at: &PhysicalGlyph) -> f64 {
    f64::from(f32::from_bits(at.cache_key.font_size_bits))
}

/// Line width (device px) of the hollow box that stands in for a glyph the text system does not know.
pub const TOFU_STROKE: f64 = 1.0;

/// Centre line of the tofu box for a glyph with pen `x` and `baseline` (device px) at `em` device px per em.
pub fn tofu_box(x: f64, baseline: f64, em: f64) -> Rect {
    let (w, h) = ((em * 0.5).max(3.0), (em * 0.7).max(4.0));
    Rect::new(x + 0.5, baseline - h, x + w, baseline - 0.5)
}

/// The caret bar in logical px, snapped to the device pixel grid so it never blurs across two columns: `at` is the
/// zero-width caret rect (`Draw::Caret::rect`), `scale` device px per logical px. The left edge rounds to a whole
/// device px and the width to a whole number of them, never below one.
pub fn caret_bar(at: Rect, width: f64, scale: f64) -> Rect {
    let x = (at.x0 * scale).round();
    let w = (width * scale).round().max(1.0);
    Rect::new(x / scale, at.y0, (x + w) / scale, at.y1)
}
