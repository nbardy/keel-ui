//! `RichText` + `TextStyle` -> a shaped cosmic-text `Buffer`.

use crate::style::{MaxWidth, Numerals, Overflow, RichText, SpanColor, SpanWeight, TextStyle};
use core::ops::Range;
use cosmic_text::{
    Attrs, Buffer, CacheKeyFlags, Color as CtColor, Ellipsize, EllipsizeHeightLimit, Family,
    FamilyOwned, FeatureTag, FontSystem, Shaping, Weight, Wrap, fontdb,
};
use kit_core::peniko::Color;

/// One run of text with fully resolved attributes (overlapping spans merged, later span wins per property).
/// `color: None` is the meaning "the node's ink" (cosmic-text's own `color_opt`).
#[derive(Clone, Eq, PartialEq, Hash, Debug)]
pub(crate) struct Segment {
    pub range: Range<usize>,
    pub color: Option<CtColor>,
    pub weight: Weight,
}

/// Spans that cannot be applied (empty/inverted, out of range, or off a char boundary). Counted, never silently fixed.
pub(crate) struct Resolved {
    pub segments: Vec<Segment>,
    pub invalid_spans: usize,
}

/// cosmic-text keeps colours as RGBA8.
fn rgba8(c: Color) -> CtColor {
    let [r, g, b, a] = c.to_rgba8().to_u8_array();
    CtColor::rgba(r, g, b, a)
}

/// Per-property merge of the spans over the base attributes. cosmic-text's `AttrsList::add_span` replaces the whole
/// `Attrs` of a range, so a colour span over a bold span would lose the bold; this keeps both.
pub(crate) fn resolve_segments(text: &RichText, base: &TextStyle) -> Resolved {
    let len = text.text.len();
    let valid = |r: &Range<usize>| {
        r.start < r.end
            && r.end <= len
            && text.text.is_char_boundary(r.start)
            && text.text.is_char_boundary(r.end)
    };
    let spans: Vec<_> = text.spans.iter().filter(|s| valid(&s.range)).collect();
    let invalid_spans = text.spans.len() - spans.len();
    let mut cuts: Vec<usize> = vec![0, len];
    for s in &spans {
        cuts.push(s.range.start);
        cuts.push(s.range.end);
    }
    cuts.sort_unstable();
    cuts.dedup();
    let segments = cuts
        .windows(2)
        .map(|w| {
            let covering = spans
                .iter()
                .filter(|s| s.range.start <= w[0] && w[1] <= s.range.end);
            let color = covering
                .clone()
                .fold(base.attrs.color_opt, |acc, s| match s.style.color {
                    SpanColor::Inherit => acc,
                    SpanColor::Color(c) => Some(rgba8(c)),
                });
            let weight = covering.fold(base.attrs.weight, |acc, s| match s.style.weight {
                SpanWeight::Inherit => acc,
                SpanWeight::Weight(w) => w,
            });
            Segment {
                range: w[0]..w[1],
                color,
                weight,
            }
        })
        .collect();
    Resolved {
        segments,
        invalid_spans,
    }
}

/// A requested family measured against what is actually loaded.
pub(crate) enum FamilyUse<'a> {
    Loaded(Family<'a>),
    /// No loaded face provides it. The caller must record it and pick an explicit stand-in.
    NotLoaded {
        requested: String,
    },
}

/// `FontSource` guarantees the sans/serif/mono generics; a named family must be in the database, and cursive/fantasy
/// were never configured, so they are reported rather than left to cosmic-text's fallback chain.
pub(crate) fn resolve_family<'a>(f: &'a FamilyOwned, db: &fontdb::Database) -> FamilyUse<'a> {
    match f {
        FamilyOwned::SansSerif => FamilyUse::Loaded(Family::SansSerif),
        FamilyOwned::Serif => FamilyUse::Loaded(Family::Serif),
        FamilyOwned::Monospace => FamilyUse::Loaded(Family::Monospace),
        FamilyOwned::Cursive => FamilyUse::NotLoaded {
            requested: "cursive".to_owned(),
        },
        FamilyOwned::Fantasy => FamilyUse::NotLoaded {
            requested: "fantasy".to_owned(),
        },
        FamilyOwned::Name(n) => {
            // The same query cosmic-text runs, so "loaded" means exactly "cosmic-text would find it".
            let query = fontdb::Query {
                families: &[fontdb::Family::Name(n)],
                ..fontdb::Query::default()
            };
            match db.query(&query) {
                Some(_) => FamilyUse::Loaded(Family::Name(n)),
                None => FamilyUse::NotLoaded {
                    requested: n.to_string(),
                },
            }
        }
    }
}

/// `Overflow::Ellipsis` truncates the single line of `Wrap::None`; wrapped text has no height bound in this API.
fn ellipsize(wrap: Wrap, overflow: Overflow) -> Ellipsize {
    match (wrap, overflow) {
        (Wrap::None, Overflow::Ellipsis) => Ellipsize::End(EllipsizeHeightLimit::Lines(1)),
        (Wrap::None, Overflow::Clip)
        | (Wrap::Glyph | Wrap::Word | Wrap::WordOrGlyph, Overflow::Clip | Overflow::Ellipsis) => {
            Ellipsize::None
        }
    }
}

/// Shape and lay out in `family` (the caller resolved it against the database).
pub(crate) fn build_buffer(
    fs: &mut FontSystem,
    segments: &[Segment],
    text: &str,
    style: &TextStyle,
    family: Family<'_>,
    max_width: MaxWidth,
) -> Buffer {
    let mut buffer = Buffer::new(fs, style.metrics);
    buffer.set_wrap(style.wrap);
    buffer.set_ellipsize(ellipsize(style.wrap, style.overflow));
    buffer.set_size(
        match max_width {
            MaxWidth::Unbounded => None,
            MaxWidth::Wrap(w) => Some(w as f32),
        },
        None,
    );
    let mut base = style.attrs.as_attrs().family(family);
    match style.numerals {
        Numerals::Proportional => base.font_features.enable(FeatureTag::new(b"pnum")),
        Numerals::Tabular => base.font_features.enable(FeatureTag::new(b"tnum")),
    };
    // Layout metrics are unhinted; hinted rasters would shift ink against them (and differ per size).
    base.cache_key_flags |= CacheKeyFlags::DISABLE_HINTING;
    let parts = segments.iter().map(|s| {
        let attrs = Attrs {
            color_opt: s.color,
            ..base.clone().weight(s.weight)
        };
        (&text[s.range.clone()], attrs)
    });
    buffer.set_rich_text(parts, &base, Shaping::Advanced, style.align);
    buffer.shape_until_scroll(fs, false);
    buffer
}
