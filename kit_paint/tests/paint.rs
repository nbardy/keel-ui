use kit_core::kurbo::{Point, Rect, Size, Vec2};
use kit_core::peniko::{Brush, Color};
use kit_core::{Axis, ColorVision, Epoch, Identity, MotionPref, Theme, Time, Viewport, WidgetId};
use kit_input::*;
use kit_paint::*;
use kit_view::testing::laid_out;
use kit_view::*;

#[derive(Clone, Debug, PartialEq)]
enum Msg {
    Tap,
}

fn theme() -> Theme {
    Theme::new(ColorVision::Normal, MotionPref::Full)
}
fn memory() -> UiMemory {
    UiMemory::new(Viewport::new(Size::new(1000.0, 1000.0), 1.0))
}
fn black() -> Brush {
    Brush::Solid(Color::from_rgba8(0, 0, 0, 255))
}
fn leaf() -> Node<Msg> {
    kit_view::rect(Style::column(), black())
}
fn r() -> Rect {
    Rect::new(0.0, 0.0, 100.0, 100.0)
}
fn paint_tree(tree: Node<Msg>) -> DrawList {
    let n = tree.count();
    paint(
        &laid_out(tree, &vec![r(); n]),
        &memory(),
        &theme(),
        Time::default(),
    )
}

#[test]
fn nested_layer_opacity_multiplies_and_resets_after_the_layer_closes() {
    let tree = column(vec![layer(0.5, layer(0.5, leaf())), leaf()]);
    let list = paint_tree(tree);
    let fills: Vec<f32> = list
        .items
        .iter()
        .zip(list.effective_opacities())
        .filter(|(d, _)| matches!(d, Draw::Fill { .. }))
        .map(|(_, o)| o)
        .collect();
    assert_eq!(fills, vec![0.25, 1.0]);
    // Out-of-range authoring values are clamped at emit time, so a renderer never sees opacity > 1.
    let list = paint_tree(layer(7.0, leaf()));
    assert!(matches!(list.items[0], Draw::PushLayer { opacity, .. } if opacity == 1.0));
}

/// Deterministic pseudo-random tree generator (xorshift) covering every wrapper that emits layers.
struct Gen(u64);
impl Gen {
    fn next(&mut self, n: u64) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 % n
    }
    fn tree(&mut self, depth: u32) -> Node<Msg> {
        let kind = if depth == 0 {
            self.next(3)
        } else {
            self.next(8)
        };
        match kind {
            0 => leaf(),
            1 => text("t", TextStyle::new(12.0, Color::from_rgba8(0, 0, 0, 255))),
            2 => spacer(),
            3 => column((0..self.next(3)).map(|_| self.tree(depth - 1))),
            4 => layer(0.5, self.tree(depth - 1)),
            5 => clipped(self.tree(depth - 1)),
            6 => scroll(
                Identity::Positional,
                Axis::Both,
                Style::column(),
                self.tree(depth - 1),
            ),
            _ => interactive(
                Identity::Positional,
                Bindings::activate(Msg::Tap),
                A11y::Hidden,
                self.tree(depth - 1),
            ),
        }
    }
}
#[test]
fn clip_and_layer_push_pop_balance_for_any_tree_including_scroll() {
    let mut g = Gen(0x9e37_79b9_7f4a_7c15);
    let mut pushes = 0;
    for _ in 0..400 {
        let list = paint_tree(g.tree(5));
        assert!(list.layers_balanced(), "unbalanced: {:?}", list.items);
        pushes += list
            .items
            .iter()
            .filter(|d| matches!(d, Draw::PushLayer { .. }))
            .count();
    }
    assert!(pushes > 200, "generator must exercise layers, got {pushes}");
}

#[test]
fn drag_ghost_is_painted_balanced_above_the_scene_and_vanishes_with_the_epoch() {
    let src = interactive(
        WidgetId::key("src"),
        Bindings::<Msg>::none().with_drag(DragPayload { kind: 1, value: 1 }),
        A11y::Hidden,
        leaf(),
    );
    let tree = column(vec![src]);
    let tile = Rect::new(0.0, 0.0, 40.0, 40.0);
    let laid = laid_out(tree, &[r(), tile, tile]).with_epoch(Epoch([1, 0]));
    let mut mem = memory();
    let router = Router::default();
    for (phase, x) in [(Phase::Down, 5.0), (Phase::Move, 300.0)] {
        let ev = PointerEvent {
            id: PointerId(0),
            kind: PointerKind::Mouse,
            phase,
            at: Point::new(x, 5.0),
            pressure: 1.0,
        };
        router.route(&laid, &mut mem, InputEvent::Pointer(ev), Time::default());
    }
    let ghosted = paint(&laid, &mem, &theme(), Time::default());
    assert!(ghosted.layers_balanced());
    let Some(Draw::PushLayer { opacity, .. }) = ghosted
        .items
        .iter()
        .rev()
        .find(|d| matches!(d, Draw::PushLayer { .. }))
    else {
        panic!("ghost layer missing")
    };
    assert!(*opacity < 1.0);
    let Some(Draw::Fill {
        shape: kit_core::Shape::Rect(ghost),
        ..
    }) = ghosted
        .items
        .iter()
        .rev()
        .find(|d| matches!(d, Draw::Fill { .. }))
    else {
        panic!("ghost fill missing")
    };
    assert_eq!(
        ghost.center(),
        Point::new(300.0, 5.0),
        "ghost is centred under the pointer"
    );

    // Same memory, but the screen was replaced: no ghost, even though the router has not seen another event.
    let replaced = laid_out(column(vec![leaf()]), &[r(), tile]).with_epoch(Epoch([2, 0]));
    let list = paint(&replaced, &mem, &theme(), Time::default());
    assert!(
        !list
            .items
            .iter()
            .any(|d| matches!(d, Draw::PushLayer { .. }))
    );
}

#[test]
fn scroll_offset_moves_content_but_not_the_clip() {
    let content = Rect::new(0.0, 0.0, 100.0, 400.0);
    let tree = scroll(
        WidgetId::key("s"),
        Axis::Vertical,
        Style::column(),
        column(vec![leaf()]),
    );
    let laid = laid_out(tree, &[r(), content, Rect::new(0.0, 10.0, 100.0, 60.0)]);
    let mut mem = memory();
    Router::default().route(
        &laid,
        &mut mem,
        InputEvent::Wheel {
            at: Point::new(5.0, 5.0),
            delta: Vec2::new(0.0, 25.0),
            unit: DeltaUnit::Pixels,
        },
        Time::default(),
    );
    let list = paint(&laid, &mem, &theme(), Time::default());
    let Some(Draw::PushLayer {
        clip: kit_core::Clip::ToRect(clip),
        ..
    }) = list.items.first()
    else {
        panic!("scroll clip first")
    };
    assert_eq!(*clip, r());
    let Some(Draw::Fill {
        shape: kit_core::Shape::Rect(fill),
        ..
    }) = list.items.iter().find(|d| matches!(d, Draw::Fill { .. }))
    else {
        panic!("fill")
    };
    assert_eq!(*fill, Rect::new(0.0, -15.0, 100.0, 35.0));
}
