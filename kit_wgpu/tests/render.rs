//! Real-GPU readback tests (Metal on the dev Mac) for what the CPU-vs-GPU parity test cannot say: ordering under
//! batching, frame-to-frame pixel retention, the glyph-atlas lifetime fence and typed errors. Text is the bundled Inter
//! through the real `CosmicText` (`kit_view::testing::text_system`).
use kit_core::kurbo::{Affine, Point, Rect, RoundedRectRadii, Vec2};
use kit_core::peniko::{Brush, Color, Gradient};
use kit_core::{Border, Clip, Shadow, Shape};
use kit_paint::{Draw, DrawList, RenderError};
use kit_view::kit_text::GlyphRaster;
use kit_view::testing::text_system;
use kit_view::{CosmicText, MaxWidth, RichText, TextStyle};
use kit_wgpu::WgpuTarget;

mod common;
use common::{Harness, Px};

impl Harness {
    fn px(&self, pixels: &[Px], x: u32, y: u32) -> Px {
        pixels[(y * self.size.0 + x) as usize]
    }
}

fn rgba(r: u8, g: u8, b: u8, a: u8) -> Brush {
    Brush::Solid(Color::from_rgba8(r, g, b, a))
}
fn solid_rect(rect: Rect, fill: Brush) -> Draw {
    Draw::RoundRect {
        rect,
        radii: RoundedRectRadii::from_single_radius(0.0),
        fill,
        border: Border::None,
        shadow: Shadow::None,
    }
}
fn list(items: Vec<Draw>) -> DrawList {
    DrawList { items }
}
fn push(opacity: f32) -> Draw {
    Draw::PushLayer {
        opacity,
        clip: Clip::Unclipped,
    }
}
fn assert_px(got: Px, want: Px, tol: u8) {
    assert!(
        got.iter().zip(want).all(|(g, w)| g.abs_diff(w) <= tol),
        "pixel {got:?} != {want:?} (tol {tol})"
    );
}
fn lit(pixels: &[Px]) -> usize {
    pixels.iter().filter(|p| p[3] > 0).count()
}

/// One layout of `s` at `size` px, and the draw that paints it.
fn text_draw(text: &mut CosmicText, s: &str, size: f32, at: Point) -> Draw {
    let layout = text
        .layout(
            &RichText::plain(s),
            &TextStyle::new(size, Color::WHITE),
            MaxWidth::Unbounded,
        )
        .id;
    Draw::Glyphs {
        layout,
        origin: at,
        brush: rgba(255, 255, 255, 255),
    }
}

#[test]
fn nested_opacity_multiplies() {
    let mut h = Harness::new(20, 20, 1.0);
    let l = list(vec![
        push(0.5),
        push(0.5),
        solid_rect(Rect::new(0.0, 0.0, 20.0, 20.0), rgba(255, 255, 255, 255)),
        Draw::PopLayer,
        Draw::PopLayer,
    ]);
    h.frame(Some([0.0; 4]), &l, &mut text_system()).unwrap();
    assert_px(h.px(&h.read(), 10, 10), [64, 64, 64, 64], 2);
}

#[test]
fn later_draws_stay_on_top_even_across_batch_kinds() {
    // quad, tessellated fill, quad: the second quad overlaps the fill, so batching must not hoist it under it.
    let mut h = Harness::new(40, 40, 1.0);
    let fill = Draw::Fill {
        shape: Shape::Rect(Rect::new(10.0, 10.0, 30.0, 30.0)),
        brush: rgba(0, 255, 0, 255),
        transform: Affine::IDENTITY,
    };
    let l = list(vec![
        solid_rect(Rect::new(0.0, 0.0, 20.0, 20.0), rgba(255, 0, 0, 255)),
        fill,
        solid_rect(Rect::new(15.0, 15.0, 25.0, 25.0), rgba(0, 0, 255, 255)),
    ]);
    h.frame(Some([0.0; 4]), &l, &mut text_system()).unwrap();
    let p = h.read();
    assert_px(h.px(&p, 5, 5), [255, 0, 0, 255], 1);
    assert_px(h.px(&p, 12, 12), [0, 255, 0, 255], 2);
    assert_px(h.px(&p, 20, 20), [0, 0, 255, 255], 2);
}

#[test]
fn whitespace_draws_nothing() {
    let mut h = Harness::new(60, 30, 1.0);
    let mut text = text_system();
    let space = list(vec![text_draw(
        &mut text,
        "   ",
        24.0,
        Point::new(10.0, 4.0),
    )]);
    h.frame(Some([0.0; 4]), &space, &mut text).unwrap();
    assert_eq!(lit(&h.read()), 0, "whitespace draws nothing");
}

const HUGE: f32 = 900.0;

/// Capitals whose bitmap at `HUGE` px is over half a 1024 px atlas page both ways (and fits one): exactly one per page,
/// so the four alpha pages hold four glyphs. Measured from the font rather than assumed, so a font update moves the
/// letters, not the arithmetic.
fn page_sized_letters(text: &mut CosmicText) -> Vec<char> {
    let letters: Vec<char> = "ABCDEGHKMNOQRUVWXYZ"
        .chars()
        .filter(|c| {
            let id = text
                .layout(
                    &RichText::plain(c.to_string()),
                    &TextStyle::new(HUGE, Color::WHITE),
                    MaxWidth::Unbounded,
                )
                .id;
            let buffer = text.buffer(id).expect("live");
            let key = buffer.layout_runs().next().expect("one line").glyphs[0]
                .physical((0.0, 0.0), 1.0)
                .cache_key;
            matches!(text.rasterize(key), GlyphRaster::Image(i)
                if (513..1023).contains(&i.width) && (513..1023).contains(&i.height))
        })
        .collect();
    assert!(
        letters.len() >= 11,
        "need 11 page-sized glyphs, have {letters:?}"
    );
    letters
}

fn huge_glyphs(text: &mut CosmicText, chars: &[char]) -> DrawList {
    let s: String = chars.iter().collect();
    list(vec![text_draw(text, &s, HUGE, Point::ZERO)])
}

#[test]
fn glyph_atlas_evicts_old_pages_but_never_within_a_frame() {
    let mut h = Harness::new(16, 16, 1.0);
    let mut text = text_system();
    let letters = page_sized_letters(&mut text);
    let first = huge_glyphs(&mut text, &letters[..4]);
    h.frame(None, &first, &mut text).unwrap();
    assert_eq!(h.ui.atlas_pages(), 4);
    let second = huge_glyphs(&mut text, &letters[4..6]);
    h.frame(None, &second, &mut text).unwrap();
    assert_eq!(h.ui.atlas_pages(), 4, "full atlas recycles the oldest page");
    let too_many = huge_glyphs(&mut text, &letters[6..11]);
    let err = h.frame(None, &too_many, &mut text);
    assert!(matches!(err, Err(RenderError::Backend(_))), "{err:?}");
}

/// Regression guard (cleanup pass): eviction used to key on "not used in this render", so a second render recorded into
/// the same unsubmitted encoder could reset and refill a page the first render's draws still sample (the refill is a
/// `write_texture`, which lands before the whole submission).
#[test]
fn atlas_never_recycles_a_page_an_unsubmitted_encoder_still_samples() {
    let mut h = Harness::new(16, 16, 1.0);
    let mut text = text_system();
    let letters = page_sized_letters(&mut text);
    let all = huge_glyphs(&mut text, &letters[..4]);
    h.frame(None, &all, &mut text).unwrap();
    assert_eq!(h.ui.atlas_pages(), 4);

    let mut encoder = h.device.create_command_encoder(&Default::default());
    // Touches all four (resident) pages, so each is now referenced by this unsubmitted encoder.
    h.ui.render(
        &all,
        &mut text,
        WgpuTarget {
            encoder: &mut encoder,
            surface: &h.surface,
        },
    )
    .unwrap();
    let fresh = huge_glyphs(&mut text, &letters[4..6]);
    let blocked = h.ui.render(
        &fresh,
        &mut text,
        WgpuTarget {
            encoder: &mut encoder,
            surface: &h.surface,
        },
    );
    assert!(
        matches!(&blocked, Err(RenderError::Backend(m)) if m.contains("frame_submitted")),
        "{blocked:?}"
    );

    h.queue.submit([encoder.finish()]);
    h.ui.frame_submitted();
    h.frame(None, &fresh, &mut text)
        .expect("after the submit boundary the oldest page may be recycled");
}

#[test]
fn unbalanced_layers_and_non_solid_brushes_are_typed_errors() {
    let mut h = Harness::new(8, 8, 1.0);
    for items in [vec![Draw::PopLayer], vec![push(1.0)]] {
        let e = h.frame(None, &list(items), &mut text_system());
        assert_eq!(e, Err(RenderError::UnbalancedLayers));
    }
    // A gradient must fail loudly, not draw some other colour (the CPU backend does the same).
    let gradient = Gradient::new_linear((0.0, 0.0), (8.0, 0.0))
        .with_stops([(0.0_f32, Color::WHITE), (1.0, Color::BLACK)]);
    let fill = Draw::Fill {
        shape: Shape::Rect(Rect::new(0.0, 0.0, 8.0, 8.0)),
        brush: Brush::Gradient(gradient),
        transform: Affine::IDENTITY,
    };
    let e = h.frame(None, &list(vec![fill]), &mut text_system());
    assert_eq!(e, Err(RenderError::UnsupportedBrush));
}

#[test]
fn drawing_keeps_existing_pixels_it_does_not_touch() {
    let mut h = Harness::new(30, 30, 1.0);
    let l = list(vec![solid_rect(
        Rect::new(10.0, 10.0, 20.0, 20.0),
        rgba(255, 0, 0, 255),
    )]);
    h.frame(Some([1.0, 0.0, 1.0, 1.0]), &l, &mut text_system())
        .unwrap();
    let p = h.read();
    assert_px(h.px(&p, 2, 2), [255, 0, 255, 255], 0);
    assert_px(h.px(&p, 15, 15), [255, 0, 0, 255], 1);
    // a second UI frame on the same texture, with no clear in between, keeps the first one
    let l2 = list(vec![solid_rect(
        Rect::new(0.0, 0.0, 4.0, 4.0),
        rgba(0, 0, 255, 255),
    )]);
    h.frame(None, &l2, &mut text_system()).unwrap();
    let p = h.read();
    assert_px(h.px(&p, 15, 15), [255, 0, 0, 255], 1);
    assert_px(h.px(&p, 1, 1), [0, 0, 255, 255], 1);
}

#[test]
fn typical_hud_is_a_handful_of_draw_calls() {
    // Panels, interleaved text and fills: batching must not turn interleaving into a draw call per item.
    let mut h = Harness::new(200, 120, 1.0);
    let mut text = text_system();
    let mut items = Vec::new();
    for i in 0..4 {
        let x = 4.0 + f64::from(i) * 48.0;
        items.push(Draw::RoundRect {
            rect: Rect::new(x, 4.0, x + 44.0, 60.0),
            radii: RoundedRectRadii::from_single_radius(6.0),
            fill: rgba(30, 30, 40, 220),
            border: Border::Solid {
                width: 1.0,
                color: Color::from_rgba8(200, 200, 220, 255),
            },
            shadow: Shadow::Drop {
                offset: Vec2::new(0.0, 2.0),
                blur: 4.0,
                spread: 0.0,
                color: Color::from_rgba8(0, 0, 0, 128),
            },
        });
        items.push(text_draw(&mut text, "12", 14.0, Point::new(x + 4.0, 8.0)));
        items.push(text_draw(&mut text, "ABC", 12.0, Point::new(x + 4.0, 40.0)));
        items.push(Draw::Fill {
            shape: Shape::Rect(Rect::new(x + 4.0, 48.0, x + 40.0, 52.0)),
            brush: rgba(60, 200, 90, 255),
            transform: Affine::IDENTITY,
        });
    }
    h.frame(Some([0.0; 4]), &list(items), &mut text).unwrap();
    let stats = h.ui.last_stats();
    eprintln!("HUD draw calls: {stats:?}");
    assert!(stats.draw_calls <= 6, "{stats:?}");
}
