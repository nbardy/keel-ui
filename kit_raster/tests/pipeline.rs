//! Numeric behaviour of the renderer on hand-built draw lists (the goldens cover what the real painter emits).
use kit_core::kurbo::{Affine, Point, Rect, RoundedRectRadii, Vec2};
use kit_core::peniko::{Brush, Color, Gradient};
use kit_core::{Border, Clip, Shadow, Shape};
use kit_paint::{Draw, DrawList, RenderError};
use kit_raster::RasterUi;
use kit_raster::tiny_skia::{Color as Sk, Pixmap, PremultipliedColorU8};
use kit_view::testing::text_system;
use kit_view::{MaxWidth, RichText, SpanColor, SpanStyle, SpanWeight, TextLayoutId, TextStyle};

fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::from_rgba8(r, g, b, 255)
}
fn solid(c: Color) -> Brush {
    Brush::Solid(c)
}
fn canvas(w: u32, h: u32, c: [u8; 3]) -> Pixmap {
    let mut p = Pixmap::new(w, h).expect("size");
    p.fill(Sk::from_rgba8(c[0], c[1], c[2], 255));
    p
}
fn px(p: &Pixmap, x: u32, y: u32) -> [u8; 3] {
    let c: PremultipliedColorU8 = p.pixel(x, y).expect("in bounds");
    [c.red(), c.green(), c.blue()]
}
fn fill(rect: Rect, c: Color) -> Draw {
    Draw::Fill {
        shape: Shape::Rect(rect),
        brush: solid(c),
        transform: Affine::IDENTITY,
    }
}
fn push(opacity: f32, clip: Clip) -> Draw {
    Draw::PushLayer { opacity, clip }
}
fn render(items: Vec<Draw>, target: &mut Pixmap) -> Result<(), RenderError> {
    RasterUi::new().render(
        &DrawList {
            items,
            ..Default::default()
        },
        &mut text_system(),
        target,
    )
}
fn near(a: [u8; 3], b: [u8; 3], tol: u8) -> bool {
    a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= tol)
}

#[test]
fn nested_layer_opacity_multiplies_and_a_group_is_composited_once() {
    let all = Rect::new(0.0, 0.0, 40.0, 40.0);
    // 0.5 inside 0.5 over black = 25% white.
    let mut p = canvas(40, 40, [0, 0, 0]);
    let items = vec![
        push(0.5, Clip::Unclipped),
        push(0.5, Clip::Unclipped),
        fill(all, rgb(255, 255, 255)),
        Draw::PopLayer,
        Draw::PopLayer,
    ];
    render(items, &mut p).expect("renders");
    assert!(
        near(px(&p, 20, 20), [64, 64, 64], 1),
        "got {:?}",
        px(&p, 20, 20)
    );

    // One 50% group with two overlapping opaque squares: the overlap shows only the top square at 50%, not a
    // double-composited mix (that would be what per-item opacity gives).
    let mut p = canvas(40, 40, [0, 0, 0]);
    let items = vec![
        push(0.5, Clip::Unclipped),
        fill(Rect::new(0.0, 0.0, 30.0, 30.0), rgb(200, 0, 0)),
        fill(Rect::new(10.0, 10.0, 40.0, 40.0), rgb(0, 0, 200)),
        Draw::PopLayer,
    ];
    render(items, &mut p).expect("renders");
    assert!(near(px(&p, 5, 5), [100, 0, 0], 1), "red only");
    assert!(
        near(px(&p, 20, 20), [0, 0, 100], 1),
        "overlap is blue only: {:?}",
        px(&p, 20, 20)
    );
}

#[test]
fn a_clipped_layer_leaves_every_pixel_outside_the_clip_untouched() {
    let sentinel = [10, 20, 30];
    let window = Rect::new(10.0, 10.0, 30.0, 30.0);
    let mut p = canvas(48, 48, sentinel);
    let items = vec![
        push(1.0, Clip::ToRect(window)),
        fill(Rect::new(0.0, 0.0, 48.0, 48.0), rgb(0, 200, 0)),
        Draw::PopLayer,
    ];
    render(items, &mut p).expect("renders");
    for y in 0..48 {
        for x in 0..48 {
            let inside = (10..30).contains(&x) && (10..30).contains(&y);
            let want = if inside { [0, 200, 0] } else { sentinel };
            assert_eq!(px(&p, x, y), want, "pixel ({x},{y})");
        }
    }
}

#[test]
fn unbalanced_layers_error_and_leave_the_target_untouched() {
    let all = Rect::new(0.0, 0.0, 8.0, 8.0);
    let before = canvas(8, 8, [1, 2, 3]);
    for items in [
        vec![fill(all, rgb(255, 0, 0)), Draw::PopLayer],
        vec![push(0.5, Clip::Unclipped), fill(all, rgb(255, 0, 0))],
        vec![push(0.5, Clip::Unclipped), Draw::PopLayer, Draw::PopLayer],
    ] {
        let mut p = before.clone();
        assert_eq!(render(items, &mut p), Err(RenderError::UnbalancedLayers));
        assert_eq!(p.data(), before.data());
    }
}

/// The game paints solid colours only; a gradient must fail loudly, not draw some other colour.
#[test]
fn a_gradient_brush_is_a_typed_error_and_leaves_the_target_untouched() {
    let before = canvas(8, 8, [1, 2, 3]);
    let mut p = before.clone();
    let gradient = Gradient::new_linear((0.0, 0.0), (8.0, 0.0))
        .with_stops([(0.0_f32, rgb(255, 0, 0)), (1.0, rgb(0, 0, 255))]);
    let draw = Draw::Fill {
        shape: Shape::Rect(Rect::new(0.0, 0.0, 8.0, 8.0)),
        brush: Brush::Gradient(gradient),
        transform: Affine::IDENTITY,
    };
    assert_eq!(
        render(vec![draw], &mut p),
        Err(RenderError::UnsupportedBrush)
    );
    assert_eq!(p.data(), before.data());
}

#[test]
fn glyphs_take_the_brush_unless_a_span_coloured_them_and_whitespace_draws_nothing() {
    let mut text = text_system();
    let style = TextStyle::new(24.0, rgb(255, 0, 0));
    let mut draw_text = |s: RichText, brush: Color| {
        let layout = text.layout(&s, &style, MaxWidth::Unbounded).id;
        let mut p = canvas(80, 40, [0, 0, 0]);
        let list = DrawList {
            redraw: kit_core::Redraw::Idle,
            items: vec![Draw::Glyphs {
                layout,
                origin: Point::new(4.0, 4.0),
                brush: solid(brush),
            }],
        };
        RasterUi::new()
            .render(&list, &mut text, &mut p)
            .expect("renders");
        p
    };
    let lit = |p: &Pixmap| {
        (0..40)
            .flat_map(|y| (0..80).map(move |x| (x, y)))
            .filter(|&(x, y)| px(p, x, y) != [0, 0, 0])
            .collect::<Vec<_>>()
    };
    let only = |p: &Pixmap, channel: usize| {
        let on = lit(p);
        !on.is_empty()
            && on.iter().all(|&(x, y)| {
                let c = px(p, x, y);
                (0..3).all(|i| (i == channel) == (c[i] > 0))
            })
    };

    let red = draw_text(RichText::plain("A"), rgb(255, 0, 0));
    assert!(only(&red, 0), "coverage mask takes the brush colour");
    let green_span = RichText::plain("A").with_span(
        0..1,
        SpanStyle {
            color: SpanColor::Color(rgb(0, 255, 0)),
            weight: SpanWeight::Inherit,
        },
    );
    let green = draw_text(green_span, rgb(255, 0, 0));
    assert!(only(&green, 1), "a span colour wins over the brush");

    let space = draw_text(RichText::plain("   "), rgb(255, 255, 255));
    assert!(lit(&space).is_empty(), "whitespace draws nothing");
}

/// A layout evicted by `end_frame` (or never issued) is a typed error, not an empty draw.
#[test]
fn glyphs_of_a_layout_that_is_not_live_are_a_typed_error() {
    let mut p = canvas(8, 8, [0, 0, 0]);
    let stale = TextLayoutId(999);
    let draw = Draw::Glyphs {
        layout: stale,
        origin: Point::ZERO,
        brush: solid(rgb(255, 255, 255)),
    };
    assert_eq!(
        render(vec![draw], &mut p),
        Err(RenderError::TextNotLive(stale))
    );
}

#[test]
fn drop_shadow_is_soft_offset_and_bounded() {
    let rect = Rect::new(40.0, 20.0, 80.0, 50.0);
    let mut p = canvas(120, 100, [200, 200, 200]);
    let item = Draw::RoundRect {
        rect,
        radii: RoundedRectRadii::from_single_radius(4.0),
        fill: solid(rgb(255, 255, 255)),
        border: Border::None,
        shadow: Shadow::Drop {
            offset: Vec2::new(0.0, 6.0),
            blur: 8.0,
            spread: 0.0,
            color: Color::from_rgba8(0, 0, 0, 200),
        },
    };
    render(vec![item], &mut p).expect("renders");
    let below = |y| px(&p, 60, y)[0];
    assert!(
        below(52) < 150,
        "just under the box, shadow is strong: {}",
        below(52)
    );
    assert!(
        below(52) < below(58) && below(58) < below(62),
        "falls off monotonically"
    );
    assert_eq!(
        px(&p, 60, 95),
        [200, 200, 200],
        "far from the box nothing is drawn"
    );
    assert_eq!(
        px(&p, 60, 35),
        [255, 255, 255],
        "the fill covers the shadow"
    );
}
