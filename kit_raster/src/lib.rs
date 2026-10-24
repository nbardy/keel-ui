//! Seam: the CPU renderer for `kit_paint::DrawList`, on `tiny-skia`, plus the golden-image harness that the kit's
//! own tests (and games) use to pin pixels.
//!
//! - [`RasterUi`] draws a `DrawList` onto a `tiny_skia::Pixmap`.
//! - [`golden`] compares a pixmap with `tests/golden/<name>.png` (bless with `KIT_BLESS=1`).
//! - [`scene`] builds the fixture pictures through the real pipeline (`kit_view` builders, `laid_out`, `paint`).
//!
//! Text is the real `CosmicText` over the bundled Inter (`kit_view::testing::text_system`), so goldens move when
//! cosmic-text or the font bytes change: re-bless them in one commit.

mod convert;
mod glyph;
pub mod golden;
mod layers;
mod renderer;
pub mod scene;
mod shadow;

pub use renderer::RasterUi;
pub use tiny_skia;
