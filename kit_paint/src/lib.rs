//! Seam: the renderer boundary. `Draw`/`DrawList` is everything a backend must draw and nothing else; `paint` is the
//! default painter from a `LaidOut` tree. Backends (wgpu, CPU raster) are concrete types with their own inherent
//! `render`; nothing is generic over them.

mod draw;
mod geometry;
mod painter;
mod scrollbar;

pub use draw::{Draw, DrawList, RenderError, image_texels};
pub use geometry::{TOFU_STROKE, caret_bar, em, layout_glyphs, place_glyph, tofu_box};
pub use painter::paint;
