//! Integration tests on the embedded Inter faces (`kit_text/fonts`), so they run
//! on every OS. The colour-emoji test needs something Inter lacks and still reads a macOS system font; it skips LOUDLY
//! (`SKIPPED(no font) ...` on stderr) elsewhere. Tests of cosmic-text itself (kerning, ligatures, alignment) are not
//! kept: they test the library, not the kit.

use kit_core::peniko::Color;
use kit_text::cosmic_text::{self, fontdb};
use kit_text::{
    Affinity, CacheKey, CosmicText, Cursor, FamilyOwned, FontError, FontSource, GenericFamilies,
    GlyphMask, GlyphRaster, LayoutGlyph, MaxWidth, Numerals, Overflow, RichText, SpanColor,
    SpanStyle, SpanWeight, TextConfig, TextDiagnostic, TextLayoutId, TextStyle, Weight, Wrap,
};

/// The shipped faces: static Regular + Bold (see THIRD_PARTY.md). Kerning and `tnum` yes; Hebrew, `liga` and emoji no.
const INTER_REGULAR: &[u8] = include_bytes!("../fonts/Inter-Regular.ttf");
const INTER_BOLD: &[u8] = include_bytes!("../fonts/Inter-Bold.ttf");
const EMOJI: &str = "/System/Library/Fonts/Apple Color Emoji.ttc";

fn inter(config: TextConfig) -> CosmicText {
    let generics = GenericFamilies {
        sans: "Inter".into(),
        serif: "Inter".into(),
        mono: "Inter".into(),
    };
    let faces = vec![INTER_REGULAR.to_vec(), INTER_BOLD.to_vec()];
    CosmicText::new(FontSource::Embedded { faces, generics }, config).expect("Inter loads")
}

static FACES: [&[u8]; 2] = [INTER_REGULAR, INTER_BOLD];
/// What ships in the wasm (`kit_text/fonts/README.md`): the subset, not the full files above.
static SHIPPED: [&[u8]; 2] = [
    include_bytes!("../fonts/subset/Inter-Regular.latin.ttf"),
    include_bytes!("../fonts/subset/Inter-Bold.latin.ttf"),
];

fn generics() -> GenericFamilies {
    GenericFamilies {
        sans: "Inter".into(),
        serif: "Inter".into(),
        mono: "Inter".into(),
    }
}

/// Two canvases build two text systems over one font database (J-web-3). Each must shape and rasterise on its own
/// while sharing the database's `RefCell`: a borrow still held across a call into the other system would panic here,
/// and a `Static` source must measure exactly like the same bytes passed as owned `Embedded` ones.
#[test]
fn text_systems_share_one_font_database_and_static_bytes_match_owned_bytes() {
    let source = FontSource::Static {
        faces: &FACES,
        generics: generics(),
    };
    let fonts = source
        .into_shared(TextConfig::default().locale)
        .expect("static Inter loads");
    let mut sea = CosmicText::with_fonts(fonts.clone(), TextConfig::default());
    let mut board = CosmicText::with_fonts(fonts, TextConfig::default());
    let mut owned = inter(TextConfig::default());
    let st = style(18.0);
    let expected = width(&mut owned, "Hull 70 / 70", &st);
    for text in [&mut sea, &mut board] {
        assert_eq!(width(text, "Hull 70 / 70", &st), expected);
    }
    for text in [&mut sea, &mut board] {
        let id = text
            .layout(&RichText::plain("Hi"), &st, MaxWidth::Unbounded)
            .id;
        let key = glyphs(text, id)[0].physical((0.0, 0.0), 1.0).cache_key;
        assert!(matches!(text.rasterize(key), GlyphRaster::Image(_)));
    }
}

/// The subset is cut by a command line, not by looking at the game: a symbol a screen draws that the range lacks would
/// render as a .notdef box in production only (the full fixtures above cover everything and would never show it).
/// Regression guard for the J-web-3 subsetting; the list is every non-ASCII symbol the game's UI strings draw.
/// `◈` (the DOM UI's cargo glyph) is not in Inter at all and is deliberately absent.
#[test]
fn the_shipped_subset_covers_every_symbol_the_game_draws() {
    let source = FontSource::Static {
        faces: &SHIPPED,
        generics: generics(),
    };
    let mut text = CosmicText::new(source, TextConfig::default()).expect("subset loads");
    let symbols = "· × — – ’ “ ” … → ↑ ↓ ↗ ↘ ↙ ↰ ↱ ↻ ± ° − ≥ ≤ ✓ π ∞ ⊕ é § ² ³ Hull 70 / 70";
    for weight in [Weight::NORMAL, Weight::BOLD] {
        let st = style(16.0).weight(weight);
        let id = text
            .layout(&RichText::plain(symbols), &st, MaxWidth::Unbounded)
            .id;
        let missing: Vec<u16> = glyphs(&text, id)
            .iter()
            .map(|g| g.glyph_id)
            .filter(|glyph| *glyph == 0)
            .collect();
        assert!(
            missing.is_empty(),
            "{weight:?}: .notdef glyphs in the shipped subset"
        );
    }
    // Tabular numerals survive the cut: digits must keep equal widths (timers and gold rely on `tnum`).
    let tabular = style(16.0).numerals(Numerals::Tabular);
    let (one, eight) = (
        width(&mut text, "1111", &tabular),
        width(&mut text, "8888", &tabular),
    );
    assert!(
        (one - eight).abs() < 0.01,
        "tnum was subset away: {one} vs {eight}"
    );
}

fn style(size: f32) -> TextStyle {
    TextStyle::new(size, Color::BLACK)
}

fn width(t: &mut CosmicText, s: &str, st: &TextStyle) -> f64 {
    t.layout(&RichText::plain(s), st, MaxWidth::Unbounded)
        .size
        .width
}

fn glyphs(t: &CosmicText, id: TextLayoutId) -> Vec<LayoutGlyph> {
    t.buffer(id)
        .expect("live layout")
        .layout_runs()
        .flat_map(|r| r.glyphs.iter().cloned())
        .collect()
}

fn caret_x(t: &CosmicText, id: TextLayoutId, index: usize, affinity: Affinity) -> f64 {
    t.caret_rect(id, Cursor::new_with_affinity(0, index, affinity))
        .expect("live layout")
        .x0
}

#[test]
fn bidi_caret_follows_logical_order() {
    let mut t = inter(TextConfig::default());
    // "abc " LTR, three Hebrew letters (2 bytes each, bytes 4..10), " def" LTR.
    let text = "abc \u{5d0}\u{5d1}\u{5d2} def";
    let id = t
        .layout(&RichText::plain(text), &style(32.0), MaxWidth::Unbounded)
        .id;
    let x = |i, a| caret_x(&t, id, i, a);
    let d = Affinity::After;

    // LTR run: logical order is left to right.
    assert!(x(0, d) < x(1, d) && x(1, d) < x(2, d) && x(2, d) < x(3, d));
    // Hebrew run: logical order is right to left, so each later boundary sits further left.
    assert!(
        x(4, d) > x(6, d) && x(6, d) > x(8, d),
        "Hebrew carets must run right-to-left"
    );
    // The boundary between runs has two visual homes; affinity picks which. End of Hebrew (upstream) is the left edge
    // of the Hebrew run, start of the following space (downstream) is the right edge.
    assert!(x(10, Affinity::Before) < x(8, d));
    assert!(x(10, d) > x(4, Affinity::Before));
}

#[test]
fn wrapped_lines_never_exceed_max_width() {
    let mut t = inter(TextConfig::default());
    let st = style(24.0).wrap(Wrap::WordOrGlyph);
    let line_h = t
        .layout(&RichText::plain("Hello"), &st, MaxWidth::Unbounded)
        .size
        .height;
    // Includes one unbreakable run far wider than the box, which must break at glyph level.
    let text = "The quick brown fox jumps over the lazy dog Pneumonoultramicroscopicsilicovolcanoconiosis again";
    for max in [90.0, 140.0, 300.0] {
        let laid = t.layout(&RichText::plain(text), &st, MaxWidth::Wrap(max));
        let size = laid.size;
        assert!(size.width <= max, "width {} exceeds {max}", size.width);
        assert!(
            size.height > line_h * 1.5,
            "text should have wrapped at {max}"
        );
        let right_edge = glyphs(&t, laid.id)
            .iter()
            .map(|g| f64::from(g.x))
            .fold(0.0, f64::max);
        assert!(right_edge <= max, "a glyph starts beyond the box at {max}");
    }
}

#[test]
fn layout_is_idempotent_and_keyed_by_every_input() {
    let mut t = inter(TextConfig::default());
    let rt = RichText::plain("Gold: 120").with_span(
        6..9,
        SpanStyle {
            color: SpanColor::Color(Color::from_rgb8(255, 200, 0)),
            weight: SpanWeight::Inherit,
        },
    );
    let st = style(20.0);
    let a = t.layout(&rt, &st, MaxWidth::Wrap(200.0));
    let b = t.layout(&rt, &st, MaxWidth::Wrap(200.0));
    assert_eq!(a, b);
    assert_eq!(t.cached_layouts(), 1);
    // Base colour is paint-time (glyphs without a span colour take it), so recolouring the node reuses the layout...
    let mut recoloured = st.clone();
    recoloured.color = Color::from_rgb8(1, 2, 3);
    assert_eq!(t.layout(&rt, &recoloured, MaxWidth::Wrap(200.0)), a);
    // ...while anything that changes glyphs or positions must not.
    assert_ne!(t.layout(&rt, &st, MaxWidth::Wrap(201.0)), a);
    assert_ne!(t.layout(&rt, &style(21.0), MaxWidth::Wrap(200.0)), a);
    assert_ne!(
        t.layout(&RichText::plain("Gold: 120"), &st, MaxWidth::Wrap(200.0)),
        a
    );
}

#[test]
fn frame_eviction_drops_unused_and_bounds_the_cache() {
    let config = TextConfig {
        retain_frames: 2,
        max_layouts: 3,
        ..TextConfig::default()
    };
    let mut t = inter(config);
    let st = style(16.0);
    t.begin_frame();
    let keep = t.layout(&RichText::plain("keep"), &st, MaxWidth::Unbounded);
    let drop = t.layout(&RichText::plain("drop"), &st, MaxWidth::Unbounded);
    t.end_frame();
    for _ in 0..3 {
        t.begin_frame();
        assert_eq!(
            t.layout(&RichText::plain("keep"), &st, MaxWidth::Unbounded),
            keep
        );
        t.end_frame();
    }
    assert!(t.is_live(keep.id));
    assert!(!t.is_live(drop.id), "unused layout must be evicted");
    assert!(
        t.buffer(drop.id).is_none() && !t.is_live(drop.id),
        "a dead id is not live, never someone else's layout"
    );

    // Past the bound, only layouts last used in an earlier frame go; the ten used now are all kept (see the next test),
    // and the frame after them evicts down to the bound.
    t.begin_frame();
    for i in 0..10 {
        t.layout(
            &RichText::plain(format!("label {i}")),
            &st,
            MaxWidth::Unbounded,
        );
    }
    t.end_frame();
    t.begin_frame();
    t.end_frame();
    assert!(
        t.cached_layouts() <= 3,
        "hard bound violated: {}",
        t.cached_layouts()
    );
}

/// Regression: `max_layouts` evicted by age alone, so a frame using more distinct layouts than the bound (about 1,300
/// three-label list rows against 4096) lost the least recently used of them at every `end_frame` and re-shaped them
/// next frame, every frame. A frame's layouts must survive its own `end_frame`, and an unchanged next frame must
/// shape nothing.
#[test]
fn a_frame_using_more_layouts_than_the_bound_keeps_all_of_them() {
    let config = TextConfig {
        max_layouts: 3,
        ..TextConfig::default()
    };
    let mut t = inter(config);
    let st = style(16.0);
    let labels: Vec<_> = (0..10)
        .map(|i| RichText::plain(format!("row {i}")))
        .collect();
    let frame = |t: &mut CosmicText| {
        t.begin_frame();
        let ids: Vec<_> = labels
            .iter()
            .map(|l| t.layout(l, &st, MaxWidth::Unbounded).id)
            .collect();
        t.end_frame();
        ids
    };
    let first = frame(&mut t);
    assert!(first.iter().all(|id| t.is_live(*id)));
    let second = frame(&mut t);
    assert_eq!(first, second, "an unchanged frame re-shapes nothing");
}

#[test]
fn tabular_numerals_have_equal_advances() {
    let mut t = inter(TextConfig::default());
    let st = style(32.0);
    assert!(
        (width(&mut t, "1111", &st) - width(&mut t, "8888", &st)).abs() > 5.0,
        "fixture font must be proportional"
    );
    let st = st.numerals(Numerals::Tabular);
    let (ones, eights) = (width(&mut t, "1111", &st), width(&mut t, "8888", &st));
    assert!((ones - eights).abs() < 1e-3, "tabular: {ones} vs {eights}");
}

#[test]
fn span_colour_and_weight_split_runs() {
    let mut t = inter(TextConfig::default());
    let gold = Color::from_rgb8(255, 200, 0);
    let rt = RichText::plain("ab cd")
        .with_span(
            3..5,
            SpanStyle {
                color: SpanColor::Color(gold),
                weight: SpanWeight::Inherit,
            },
        )
        .with_span(
            4..5,
            SpanStyle {
                color: SpanColor::Inherit,
                weight: SpanWeight::Weight(Weight::BOLD),
            },
        )
        // Invalid (past the end): skipped and counted, never half-applied.
        .with_span(
            4..99,
            SpanStyle {
                color: SpanColor::Color(Color::BLACK),
                weight: SpanWeight::Inherit,
            },
        );
    let id = t.layout(&rt, &style(20.0), MaxWidth::Unbounded).id;
    let g = glyphs(&t, id);
    assert_eq!(t.diagnostics().invalid_spans, 1);
    // "ab " plain; "c" gold; "d" gold + bold (overlap merges per property; cosmic's add_span would replace).
    let gold8 = Some(cosmic_text::Color::rgb(255, 200, 0));
    assert_eq!(
        g.iter().map(|g| g.color_opt).collect::<Vec<_>>(),
        vec![None, None, None, gold8, gold8]
    );
    assert_ne!(g[3].font_id, g[4].font_id, "bold must select the bold face");
}

#[test]
fn ellipsis_fits_and_clip_does_not() {
    let mut t = inter(TextConfig::default());
    let text = RichText::plain("A rather long item name that cannot fit");
    let mut st = style(20.0);
    st.overflow = Overflow::Clip;
    let clipped = t.layout(&text, &st, MaxWidth::Wrap(120.0));
    assert!(
        clipped.size.width > 120.0,
        "clip keeps natural width for the renderer to clip"
    );
    st.overflow = Overflow::Ellipsis;
    let cut = t.layout(&text, &st, MaxWidth::Wrap(120.0));
    assert!(cut.size.width <= 120.0);
    assert!(glyphs(&t, cut.id).len() < text.text.len());
}

#[test]
fn rasterize_unknown_font_is_missing_and_real_glyphs_have_ink() {
    let mut t = inter(TextConfig::default());
    let id = t
        .layout(&RichText::plain("H "), &style(32.0), MaxWidth::Unbounded)
        .id;
    let g = glyphs(&t, id);
    let key = |x: f32, glyph: &LayoutGlyph| glyph.physical((x, 0.0), 2.0).cache_key;

    let unknown = CacheKey {
        font_id: fontdb::ID::dummy(),
        ..key(0.0, &g[0])
    };
    assert_eq!(t.rasterize(unknown), GlyphRaster::Missing);
    let no_size = CacheKey {
        font_size_bits: 0.0_f32.to_bits(),
        ..key(0.0, &g[0])
    };
    assert_eq!(t.rasterize(no_size), GlyphRaster::Missing, "zero size");
    assert_eq!(
        t.rasterize(key(0.0, &g[1])),
        GlyphRaster::Empty,
        "space draws nothing"
    );

    let GlyphRaster::Image(h0) = t.rasterize(key(0.0, &g[0])) else {
        panic!("H must rasterise")
    };
    assert_eq!(h0.mask, GlyphMask::Alpha);
    assert!(
        h0.data.iter().any(|&a| a > 200),
        "stem should reach full coverage"
    );
    assert_eq!(h0.data.len(), (h0.width * h0.height) as usize);
    assert!(h0.top < 0, "H sits above the baseline (y down)");
    // Quarter-pixel positioning must actually move the ink.
    let GlyphRaster::Image(h2) = t.rasterize(key(0.5, &g[0])) else {
        panic!()
    };
    assert_ne!(h0.data, h2.data);
    // Cached result is identical.
    assert_eq!(t.rasterize(key(0.0, &g[0])), GlyphRaster::Image(h0));
}

#[test]
fn colour_emoji_rasterises_as_colour() {
    let Ok(emoji) = std::fs::read(EMOJI) else {
        eprintln!("SKIPPED(no font) {EMOJI}");
        return;
    };
    let generics = GenericFamilies {
        sans: "Inter".into(),
        serif: "Inter".into(),
        mono: "Inter".into(),
    };
    let faces = vec![INTER_REGULAR.to_vec(), emoji];
    let mut t = CosmicText::new(
        FontSource::Embedded { faces, generics },
        TextConfig::default(),
    )
    .expect("fonts load");
    let id = t
        .layout(
            &RichText::plain("\u{1F600}"),
            &style(32.0),
            MaxWidth::Unbounded,
        )
        .id;
    let key = glyphs(&t, id)[0].physical((0.0, 0.0), 1.0).cache_key;
    let GlyphRaster::Image(img) = t.rasterize(key) else {
        panic!("emoji must rasterise (fallback to the emoji face)")
    };
    assert_eq!(img.mask, GlyphMask::Color);
    // Premultiplied invariant: no channel exceeds alpha.
    assert!(
        img.data
            .chunks_exact(4)
            .all(|p| p[0] <= p[3] && p[1] <= p[3] && p[2] <= p[3])
    );
    assert!(img.data.chunks_exact(4).any(|p| p[3] == 255));
}

#[test]
fn font_source_errors_are_typed() {
    let none = FontSource::Embedded {
        faces: vec![],
        generics: GenericFamilies {
            sans: "x".into(),
            serif: "x".into(),
            mono: "x".into(),
        },
    };
    assert!(matches!(
        CosmicText::new(none, TextConfig::default()),
        Err(FontError::NoFaces)
    ));
    let wrong = FontSource::Embedded {
        faces: vec![INTER_REGULAR.to_vec()],
        generics: GenericFamilies {
            sans: "Inter".into(),
            serif: "Inter".into(),
            mono: "Nope Mono".into(),
        },
    };
    assert!(
        matches!(CosmicText::new(wrong, TextConfig::default()), Err(FontError::MissingFamily(n)) if n == "Nope Mono")
    );
}

/// Regression guard (cleanup pass): a `Named` family that is not loaded used to fall through cosmic-text's fallback
/// chain with no trace. It must lay out in the generic sans stand-in AND leave a typed diagnostic.
#[test]
fn a_named_family_that_is_not_loaded_is_recorded_and_laid_out_in_the_generic_sans() {
    let mut t = inter(TextConfig::default());
    let named = |family: &str| {
        let mut st = style(24.0);
        st.attrs.family_owned = FamilyOwned::Name(family.into());
        st
    };
    let sans = width(&mut t, "AVAV illum", &style(24.0));

    assert_eq!(width(&mut t, "AVAV illum", &named("Inter")), sans);
    assert!(
        t.diagnostics().raised.is_empty(),
        "a loaded name is not a diagnostic"
    );

    assert_eq!(
        width(&mut t, "AVAV illum", &named("Comic Nonexistent")),
        sans
    );
    let missing = TextDiagnostic::FamilyNotLoaded("Comic Nonexistent".into());
    assert!(t.diagnostics().raised.contains(&missing));
}
