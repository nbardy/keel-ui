//! What a text node asks for: cosmic-text's own attribute, metric, wrap and alignment types, plus the few things the
//! kit adds on top (node ink applied at paint time, per-property spans, ellipsis and numeral policy).
use core::ops::Range;
use cosmic_text::{Align, Attrs, AttrsOwned, Metrics, Weight, Wrap};
use kit_core::kurbo::Size;
use kit_core::peniko::Color;

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub struct TextLayoutId(pub u64);

/// A layout handed out by [`crate::CosmicText::layout`]: its id (valid until evicted by `end_frame`) and its size.
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct TextLayout {
    pub id: TextLayoutId,
    pub size: Size,
}

/// What to do with a single line that does not fit (cosmic-text only ellipsizes against a width it is given).
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub enum Overflow {
    Clip,
    /// Only for `Wrap::None`: wrapped text has no height bound in this API, so there is nothing to truncate.
    Ellipsis,
}

/// Digit widths. Becomes the `pnum`/`tnum` OpenType feature on top of `attrs.font_features`.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub enum Numerals {
    Proportional,
    /// Equal-width digits: timers, gold counters do not jitter.
    Tabular,
}

#[derive(Clone, PartialEq, Debug)]
pub struct TextStyle {
    /// Family, weight, style, stretch, features. `color_opt: None` (the default) means the glyphs take `color`
    /// when painted, so a recolour reuses the layout.
    pub attrs: AttrsOwned,
    /// Font size and line height, logical px.
    pub metrics: Metrics,
    /// `Wrap::None` is one line at its natural width; any other mode breaks to the box width.
    pub wrap: Wrap,
    /// cosmic-text's alignment; `None` is the start of the paragraph's direction (left for LTR, right for RTL).
    pub align: Option<Align>,
    pub overflow: Overflow,
    pub numerals: Numerals,
    /// The node's ink.
    pub color: Color,
}

impl TextStyle {
    /// Sans, regular weight, line height 1.25 x `size`, one unwrapped start-aligned line.
    pub fn new(size: f32, color: Color) -> Self {
        Self {
            attrs: AttrsOwned::new(&Attrs::new()),
            metrics: Metrics::relative(size, 1.25),
            wrap: Wrap::None,
            align: None,
            overflow: Overflow::Clip,
            numerals: Numerals::Proportional,
            color,
        }
    }
    pub fn weight(mut self, weight: Weight) -> Self {
        self.attrs.weight = weight;
        self
    }
    pub fn wrap(mut self, wrap: Wrap) -> Self {
        self.wrap = wrap;
        self
    }
    pub fn align(mut self, align: Align) -> Self {
        self.align = Some(align);
        self
    }
    pub fn numerals(mut self, numerals: Numerals) -> Self {
        self.numerals = numerals;
        self
    }
}

/// Per-span override, merged per property (cosmic-text's own `add_span` replaces the whole `Attrs`). `Inherit` means
/// "use the node's `TextStyle`".
#[derive(Clone, PartialEq, Debug)]
pub struct SpanStyle {
    pub color: SpanColor,
    pub weight: SpanWeight,
}
#[derive(Copy, Clone, PartialEq, Debug)]
pub enum SpanColor {
    Inherit,
    Color(Color),
}
#[derive(Copy, Clone, PartialEq, Debug)]
pub enum SpanWeight {
    Inherit,
    Weight(Weight),
}
#[derive(Clone, PartialEq, Debug)]
pub struct Span {
    /// Byte range into `RichText::text` (char boundaries).
    pub range: Range<usize>,
    pub style: SpanStyle,
}
#[derive(Clone, PartialEq, Debug, Default)]
pub struct RichText {
    pub text: String,
    pub spans: Vec<Span>,
}
impl RichText {
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            spans: Vec::new(),
        }
    }
    pub fn with_span(mut self, range: Range<usize>, style: SpanStyle) -> Self {
        self.spans.push(Span { range, style });
        self
    }
}

#[derive(Copy, Clone, PartialEq, Debug)]
pub enum MaxWidth {
    Unbounded,
    Wrap(f64),
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum GlyphMask {
    /// One coverage byte per pixel; tinted by the glyph's colour.
    Alpha,
    /// Premultiplied RGBA8 per pixel (colour emoji); not tinted.
    Color,
}
#[derive(Clone, PartialEq, Debug)]
pub struct GlyphImage {
    /// Offset of the image's top-left from the glyph origin on the baseline, device px (y down).
    pub left: i32,
    pub top: i32,
    pub width: u32,
    pub height: u32,
    pub mask: GlyphMask,
    pub data: Vec<u8>,
}
#[derive(Clone, PartialEq, Debug)]
pub enum GlyphRaster {
    Image(GlyphImage),
    /// Whitespace or zero-area glyph: nothing to draw.
    Empty,
    /// The rasteriser has no image for the key: renderers draw a visible tofu box, never skip silently.
    Missing,
}
