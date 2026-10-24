//! The kit's text: cosmic-text 0.19 (shaping with kerning/ligatures, bidi, fallback, wrapping, alignment, ellipsis,
//! tabular numerals) and swash rasterisation, used directly. Styles are cosmic `AttrsOwned`/`Metrics`/`Wrap`/`Align`,
//! carets are cosmic `Cursor`s, and renderers walk [`CosmicText::buffer`]`.layout_runs()` and rasterise each glyph's
//! `LayoutGlyph::physical(..).cache_key` through [`CosmicText::rasterize`], the single source of glyph pixels (so
//! the CPU and GPU backends agree and never load fonts).
//!
//! What the kit adds: per-property span merging (`resolve_segments`; cosmic's `add_span` replaces whole `Attrs`),
//! `Numerals`/`Overflow` policy, a layout cache with frame eviction, a bounded raster cache (`SwashCache`'s own is
//! unbounded), premultiplied colour glyphs, typed font errors and the `FamilyNotLoaded` diagnostic.
//!
//! Construction is explicit: [`CosmicText::new`] takes a [`FontSource`]; nothing is discovered implicitly (wasm has no
//! system fonts, so `FontSource::Static` / `Embedded` are the primary paths). [`inter::source`] is the bundled UI font.
//!
//! Layout cache: `layout` is idempotent per `(text, spans, style without its colour, max width)` and returns the same
//! [`TextLayoutId`] while the entry is alive. Ids are valid until evicted; eviction happens only in
//! [`CosmicText::end_frame`], and any `layout`, `buffer` or `caret_rect` call counts as a use. Accessors answer
//! `None` for an id that is not live (never issued, or evicted).
//!
//! Missing fonts are never silent. A family that no loaded face provides is laid out with the configured generic sans
//! family (an explicit stand-in, not cosmic-text's fallback chain) and recorded as
//! [`TextDiagnostic::FamilyNotLoaded`] in [`CosmicText::diagnostics`]. A glyph no face has renders as the primary
//! face's `.notdef`.

mod editor;
mod font;
mod geometry;
mod layout;
mod style;

pub use cosmic_text;
pub use cosmic_text::{
    Affinity, Align, Attrs, AttrsOwned, Buffer, CacheKey, Cursor, FamilyOwned, LayoutGlyph,
    LayoutRun, Metrics, PhysicalGlyph, Weight, Wrap,
};
pub use editor::{
    Composition, Edit, EditorState, FieldView, Goal, Mode, Motion, Unit, next_grapheme,
    paragraph_at, prev_grapheme, word_at, word_left, word_right,
};
pub use font::{FontError, FontSource, GenericFamilies, SharedFonts, inter};
pub use geometry::{
    Caret, Cluster, FieldGeometry, LineEdge, Masking, Shaped, Vertical, VisualLine,
};
pub use kit_state::{Applied, Dir, History, Rev, Selection, TextValue};
pub use style::{
    GlyphImage, GlyphMask, GlyphRaster, MaxWidth, Numerals, Overflow, RichText, Span, SpanColor,
    SpanStyle, SpanWeight, TextLayout, TextLayoutId, TextStyle,
};

use core::hash::{Hash, Hasher};
use core::mem::discriminant;
use cosmic_text::{SwashCache, SwashContent};
use kit_core::kurbo::{Rect, Size};
use layout::{FamilyUse, Segment, build_buffer, resolve_family, resolve_segments};
use std::cell::Cell;
use std::collections::{BTreeSet, HashMap};
use std::sync::atomic::{AtomicU64, Ordering};

/// Source of `CosmicText::instance` values: process-wide, so two systems never share one.
static NEXT_INSTANCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug)]
pub struct TextConfig {
    /// BCP-47 locale for script/fallback selection (never read from the platform: wasm has none).
    pub locale: String,
    /// Layouts untouched for this many frames are dropped by `end_frame`.
    pub retain_frames: u64,
    /// Hard bound on cached layouts after `end_frame` (least recently used go first).
    pub max_layouts: usize,
    /// Hard bound on cached glyph rasters after `end_frame` (cleared wholesale when exceeded).
    pub max_rasters: usize,
}
impl Default for TextConfig {
    fn default() -> Self {
        Self {
            locale: "en-US".to_owned(),
            retain_frames: 2,
            max_layouts: 4096,
            max_rasters: 8192,
        }
    }
}

/// Everything that changes a layout. The node colour is excluded on purpose (glyphs without a span colour take it at
/// paint time), so a recolour reuses the layout. `Wrap`/`Align` are not `Hash`, hence the manual impl.
#[derive(Clone, Eq, PartialEq)]
struct LayoutKey {
    text: String,
    segments: Vec<Segment>,
    attrs: AttrsOwned,
    metrics: [u32; 2],
    wrap: Wrap,
    align: Option<Align>,
    overflow: Overflow,
    numerals: Numerals,
    max_width: Option<u64>,
}

impl Hash for LayoutKey {
    fn hash<H: Hasher>(&self, h: &mut H) {
        self.text.hash(h);
        self.segments.hash(h);
        self.attrs.hash(h);
        self.metrics.hash(h);
        discriminant(&self.wrap).hash(h);
        self.align.map(|a| discriminant(&a)).hash(h);
        self.overflow.hash(h);
        self.numerals.hash(h);
        self.max_width.hash(h);
    }
}

impl LayoutKey {
    fn new(
        text: &RichText,
        segments: Vec<Segment>,
        style: &TextStyle,
        max_width: MaxWidth,
    ) -> Self {
        Self {
            text: text.text.clone(),
            segments,
            attrs: style.attrs.clone(),
            metrics: [
                style.metrics.font_size.to_bits(),
                style.metrics.line_height.to_bits(),
            ],
            wrap: style.wrap,
            align: style.align,
            overflow: style.overflow,
            numerals: style.numerals,
            max_width: match max_width {
                MaxWidth::Unbounded => None,
                MaxWidth::Wrap(w) => Some(w.to_bits()),
            },
        }
    }
}

struct Laid {
    buffer: Buffer,
    size: Size,
    line_height: f64,
    last_used: Cell<u64>,
}

impl Laid {
    /// (x, line top) of the caret. cosmic-text ignores affinity, so exact glyph boundaries are resolved here: a boundary
    /// has up to two homes (the glyph ending at the index and the glyph starting at it) which differ at bidi level
    /// changes and soft wraps; `Before` takes the trailing edge of the former, `After` the leading edge of the latter,
    /// each falling back to the other at the text's ends. Mid-cluster indices (inside a ligature) and empty lines go
    /// through cosmic-text's own placement.
    fn caret_position(&self, c: Cursor) -> (f32, f32) {
        let on_line = || self.buffer.layout_runs().filter(|r| r.line_i == c.line);
        let leading = |g: &LayoutGlyph| if g.level.is_rtl() { g.x + g.w } else { g.x };
        let trailing = |g: &LayoutGlyph| if g.level.is_rtl() { g.x } else { g.x + g.w };
        let after = on_line().find_map(|r| {
            r.glyphs
                .iter()
                .find(|g| g.start == c.index)
                .map(|g| (leading(g), r.line_top))
        });
        let before = on_line().find_map(|r| {
            r.glyphs
                .iter()
                .find(|g| g.end == c.index)
                .map(|g| (trailing(g), r.line_top))
        });
        let chosen = match c.affinity {
            Affinity::After => after.or(before),
            Affinity::Before => before.or(after),
        };
        chosen
            .or_else(|| self.buffer.cursor_position(&c))
            .unwrap_or_else(|| {
                // Empty line: it sits at the line start.
                (0.0, on_line().next().map_or(0.0, |r| r.line_top))
            })
    }
}

/// A request the text system could not honour as asked. Each distinct value is recorded once.
#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Debug)]
pub enum TextDiagnostic {
    /// The family (a name, or a generic `FontSource` does not configure) is not loaded; text in it was laid out in
    /// the generic sans family.
    FamilyNotLoaded(String),
}

/// Things that were not applied as asked. `invalid_spans` counts `Span`s skipped (bad range); `raised` holds the
/// distinct [`TextDiagnostic`]s seen so far.
#[derive(Clone, Eq, PartialEq, Default, Debug)]
pub struct Diagnostics {
    pub invalid_spans: usize,
    pub raised: BTreeSet<TextDiagnostic>,
}

pub struct CosmicText {
    instance: u64,
    fs: SharedFonts,
    swash: SwashCache,
    config: TextConfig,
    generation: u64,
    by_key: HashMap<LayoutKey, TextLayoutId>,
    laid: HashMap<TextLayoutId, Laid>,
    next_id: u64,
    layout_calls: u64,
    rasters: HashMap<CacheKey, GlyphRaster>,
    diagnostics: Diagnostics,
}

impl CosmicText {
    pub fn new(source: FontSource, config: TextConfig) -> Result<Self, FontError> {
        let fonts = source.into_shared(config.locale.clone())?;
        Ok(Self::with_fonts(fonts, config))
    }

    /// A text system over fonts another one already loaded (see [`SharedFonts`]). Layout and glyph caches stay
    /// per-system; only the faces and shaping state are shared.
    pub fn with_fonts(fonts: SharedFonts, config: TextConfig) -> Self {
        Self {
            instance: NEXT_INSTANCE.fetch_add(1, Ordering::Relaxed),
            fs: fonts,
            swash: SwashCache::new(),
            config,
            generation: 0,
            by_key: HashMap::new(),
            laid: HashMap::new(),
            next_id: 0,
            layout_calls: 0,
            rasters: HashMap::new(),
            diagnostics: Diagnostics::default(),
        }
    }

    /// Which text system this is: unique per constructed `CosmicText`, never reused. A size this system measured is
    /// only valid for this system (its fonts and config decide it), so a caller that keeps measured sizes between
    /// frames (`kit_layout`'s retained tree) keeps this beside them and drops them when it changes.
    pub fn instance(&self) -> u64 {
        self.instance
    }

    pub fn diagnostics(&self) -> &Diagnostics {
        &self.diagnostics
    }

    /// How many times `layout` has been asked, hit or miss. A hit still builds and hashes the full cache key, so
    /// callers that ask the same question repeatedly within a frame (taffy's measure) should remember the answer;
    /// tests use this to hold them to it.
    pub fn layout_calls(&self) -> u64 {
        self.layout_calls
    }

    pub fn cached_layouts(&self) -> usize {
        self.laid.len()
    }

    pub fn is_live(&self, id: TextLayoutId) -> bool {
        self.laid.contains_key(&id)
    }

    pub fn begin_frame(&mut self) {
        self.generation += 1;
    }

    /// Evict layouts unused for `retain_frames` frames, then enforce the hard bounds.
    pub fn end_frame(&mut self) {
        let (generation, retain) = (self.generation, self.config.retain_frames);
        self.laid
            .retain(|_, l| generation.saturating_sub(l.last_used.get()) < retain);
        if self.laid.len() > self.config.max_layouts {
            let mut by_age: Vec<_> = self
                .laid
                .iter()
                .map(|(id, l)| (l.last_used.get(), *id))
                .collect();
            by_age.sort_unstable();
            let excess = self.laid.len() - self.config.max_layouts;
            by_age.iter().take(excess).for_each(|(_, id)| {
                self.laid.remove(id);
            });
        }
        let laid = &self.laid;
        self.by_key.retain(|_, id| laid.contains_key(id));
        if self.rasters.len() > self.config.max_rasters {
            self.rasters.clear();
        }
    }

    fn touch(&self, id: TextLayoutId) -> Option<&Laid> {
        self.laid
            .get(&id)
            .inspect(|l| l.last_used.set(self.generation))
    }

    /// Shape `text` (idempotent: an identical request returns the same live layout). `layout` is the only call that
    /// shapes; the returned size is the ink-relevant box (trailing whitespace hangs, an empty text is one line tall).
    pub fn layout(
        &mut self,
        text: &RichText,
        style: &TextStyle,
        max_width: MaxWidth,
    ) -> TextLayout {
        self.layout_calls += 1;
        let resolved = resolve_segments(text, style);
        let key = LayoutKey::new(text, resolved.segments, style, max_width);
        if let Some((id, l)) = self
            .by_key
            .get(&key)
            .and_then(|id| self.laid.get(id).map(|l| (*id, l)))
        {
            l.last_used.set(self.generation);
            return TextLayout { id, size: l.size };
        }
        self.diagnostics.invalid_spans += resolved.invalid_spans;
        let laid = self.build(text, &key.segments, style, max_width);
        let id = TextLayoutId(self.next_id);
        self.next_id += 1;
        let size = laid.size;
        self.laid.insert(id, laid);
        self.by_key.insert(key, id);
        TextLayout { id, size }
    }

    /// The shaped buffer: iterate `layout_runs()` to draw. `None` for an id that is not live.
    pub fn buffer(&self, id: TextLayoutId) -> Option<&Buffer> {
        self.touch(id).map(|l| &l.buffer)
    }

    /// The caret at `c` relative to the layout's top-left, one line tall and zero wide. `None` for an id that is not
    /// live.
    pub fn caret_rect(&self, id: TextLayoutId, c: Cursor) -> Option<Rect> {
        self.touch(id).map(|l| {
            let (x, top) = l.caret_position(c);
            let (x, top) = (f64::from(x), f64::from(top));
            Rect::new(x, top, x, top + l.line_height)
        })
    }

    /// The editing geometry of a field's shaped `shown` text (see [`FieldGeometry`]). `None` for an id that is not
    /// live.
    pub fn field_geometry(
        &self,
        id: TextLayoutId,
        shown: Shaped,
        view: &FieldView,
        view_width: f32,
    ) -> Option<FieldGeometry> {
        self.buffer(id)
            .map(|buffer| FieldGeometry::build(buffer, shown, view, view_width))
    }

    /// Pixels for one glyph, keyed by `LayoutGlyph::physical(..).cache_key` (device-px size and subpixel bin).
    /// Cached up to `TextConfig::max_rasters`.
    pub fn rasterize(&mut self, key: CacheKey) -> GlyphRaster {
        if let Some(hit) = self.rasters.get(&key) {
            return hit.clone();
        }
        let raster = self.rasterize_uncached(key);
        self.rasters.insert(key, raster.clone());
        raster
    }

    fn build(
        &mut self,
        text: &RichText,
        segments: &[Segment],
        style: &TextStyle,
        max_width: MaxWidth,
    ) -> Laid {
        let mut fs = self.fs.0.borrow_mut();
        let family = match resolve_family(&style.attrs.family_owned, fs.db()) {
            FamilyUse::Loaded(family) => family,
            FamilyUse::NotLoaded { requested } => {
                self.diagnostics
                    .raised
                    .insert(TextDiagnostic::FamilyNotLoaded(requested));
                // Explicit stand-in: the generic sans family `FontSource` made the caller name.
                cosmic_text::Family::SansSerif
            }
        };
        let buffer = build_buffer(&mut fs, segments, &text.text, style, family, max_width);
        let line_height = f64::from(style.metrics.line_height);
        let mut width = 0.0_f64;
        let mut bottom = 0.0_f64;
        for run in buffer.layout_runs() {
            width = width.max(f64::from(run.line_w) - hanging_whitespace(&run));
            bottom = bottom.max(f64::from(run.line_top + run.line_height));
        }
        Laid {
            buffer,
            // An empty text still occupies one line (a text field needs a caret height).
            size: Size::new(width, bottom.max(line_height)),
            line_height,
            last_used: Cell::new(self.generation),
        }
    }

    fn rasterize_uncached(&mut self, key: CacheKey) -> GlyphRaster {
        let px = f32::from_bits(key.font_size_bits);
        if !(px.is_finite() && px > 0.0) {
            return GlyphRaster::Missing;
        }
        match self
            .swash
            .get_image_uncached(&mut self.fs.0.borrow_mut(), key)
        {
            None => GlyphRaster::Missing,
            Some(img) if img.placement.width == 0 || img.placement.height == 0 => {
                GlyphRaster::Empty
            }
            Some(img) => {
                let (mask, data) = match img.content {
                    SwashContent::Mask => (GlyphMask::Alpha, img.data),
                    // swash emits straight-alpha RGBA; renderers composite premultiplied.
                    SwashContent::Color => (GlyphMask::Color, premultiply(img.data)),
                    // cosmic-text's swash cache asks for `Format::Alpha`, so subpixel (LCD) masks are not produced.
                    SwashContent::SubpixelMask => return GlyphRaster::Missing,
                };
                GlyphRaster::Image(GlyphImage {
                    left: img.placement.left,
                    top: -img.placement.top,
                    width: img.placement.width,
                    height: img.placement.height,
                    mask,
                    data,
                })
            }
        }
    }
}

/// Trailing whitespace hangs past the box edge (as in CSS) and is not part of the line's width, so a wrapped line
/// never reports wider than the box it was broken for.
fn hanging_whitespace(run: &LayoutRun<'_>) -> f64 {
    let blank = |g: &&LayoutGlyph| run.text[g.start..g.end].chars().all(char::is_whitespace);
    let ink_end = run
        .glyphs
        .iter()
        .filter(|g| !blank(g))
        .map(|g| g.end)
        .max()
        .unwrap_or(0);
    run.glyphs
        .iter()
        .filter(|g| g.start >= ink_end && blank(g))
        .map(|g| f64::from(g.w))
        .sum()
}

fn premultiply(mut rgba: Vec<u8>) -> Vec<u8> {
    rgba.chunks_exact_mut(4).for_each(|p| {
        let a = u32::from(p[3]);
        p[..3]
            .iter_mut()
            .for_each(|c| *c = ((u32::from(*c) * a + 127) / 255) as u8);
    });
    rgba
}
