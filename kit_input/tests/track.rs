//! Pointer bindings (`Bindings::on_pointer`) through the real router: capture beyond the node, release outside,
//! cancel by epoch change and Escape, and local coordinates inside a scrolled area.
use kit_core::kurbo::{Point, Rect, Size, Vec2};
use kit_core::peniko::{Brush, Color};
use kit_core::{Axis, Epoch, Key, NamedKey, Time, Viewport, WidgetId};
use kit_input::*;
use kit_view::testing::laid_out;
use kit_view::*;

fn block() -> Node<PointerAt> {
    rect(
        Style::column(),
        Brush::Solid(Color::from_rgba8(0, 0, 0, 255)),
    )
}
fn knob() -> Node<PointerAt> {
    interactive(
        WidgetId::key("knob"),
        Bindings::none().on_pointer(|p| p),
        A11y::Hidden,
        block(),
    )
}
/// The knob at (100, 100)..(200, 140) in a 1000 px column.
fn scene(epoch: Epoch) -> LaidOut<PointerAt> {
    let rects = [
        Rect::new(0.0, 0.0, 1000.0, 1000.0),
        Rect::new(100.0, 100.0, 200.0, 140.0),
    ];
    let rects = [rects[0], rects[1], rects[1]];
    laid_out(column(vec![knob()]), &rects).with_epoch(epoch)
}
fn memory() -> UiMemory {
    UiMemory::new(Viewport::new(Size::new(1000.0, 1000.0), 1.0))
}
fn mouse(phase: Phase, x: f64, y: f64) -> InputEvent {
    InputEvent::Pointer(PointerEvent {
        id: PointerId(0),
        kind: PointerKind::Mouse,
        phase,
        at: Point::new(x, y),
        pressure: 1.0,
    })
}
fn route(laid: &LaidOut<PointerAt>, mem: &mut UiMemory, e: InputEvent) -> Routing<PointerAt> {
    Router::default().route(laid, mem, e, Time::default())
}
fn at(phase: PointerPhase, x: f64, y: f64, travel: Travel) -> PointerAt {
    PointerAt {
        phase,
        local: Point::new(x, y),
        size: Size::new(100.0, 40.0),
        travel,
    }
}

#[test]
fn drag_beyond_the_node_keeps_reporting_and_release_outside_still_arrives() {
    let (laid, mut mem) = (scene(Epoch::default()), memory());
    let mut got = Vec::new();
    for e in [
        mouse(Phase::Down, 150.0, 120.0),
        mouse(Phase::Move, 152.0, 121.0),
        mouse(Phase::Move, 350.0, 300.0),
        mouse(Phase::Move, 60.0, 90.0),
        mouse(Phase::Up, 400.0, 500.0),
        // Released: a later move over the knob reports nothing.
        mouse(Phase::Move, 150.0, 120.0),
    ] {
        got.extend(route(&laid, &mut mem, e).messages());
    }
    use PointerPhase::*;
    assert_eq!(
        got,
        [
            at(Press, 50.0, 20.0, Travel::Still),
            at(Drag, 52.0, 21.0, Travel::Still),
            at(Drag, 250.0, 200.0, Travel::Moved),
            // Back near the press point: it has still travelled (a drag, not a click).
            at(Drag, -40.0, -10.0, Travel::Moved),
            at(Release, 300.0, 400.0, Travel::Moved),
        ]
    );
}

#[test]
fn epoch_change_and_escape_cancel_and_nothing_follows() {
    let mut mem = memory();
    route(
        &scene(Epoch([1, 1])),
        &mut mem,
        mouse(Phase::Down, 150.0, 120.0),
    );
    let replaced = scene(Epoch([1, 2]));
    let moved = route(&replaced, &mut mem, mouse(Phase::Move, 170.0, 125.0));
    assert_eq!(
        moved.messages(),
        [at(PointerPhase::Cancel, 50.0, 20.0, Travel::Still)],
        "the new screen's knob hears Cancel at the last reported point, never a Drag of the old gesture"
    );
    assert_eq!(
        route(&replaced, &mut mem, mouse(Phase::Up, 170.0, 125.0)),
        Routing::Passed
    );

    let laid = scene(Epoch::default());
    route(&laid, &mut mem, mouse(Phase::Down, 150.0, 120.0));
    let escape = InputEvent::Key {
        key: Key::Named(NamedKey::Escape),
        state: KeyState::Down,
        mods: Modifiers::default(),
    };
    let routed = route(&laid, &mut mem, escape);
    assert_eq!(
        routed,
        Routing::Handled(vec![at(PointerPhase::Cancel, 50.0, 20.0, Travel::Still)])
    );
    assert_eq!(
        route(&laid, &mut mem, mouse(Phase::Up, 150.0, 120.0)),
        Routing::Passed
    );
}

#[test]
fn local_coordinates_follow_the_scroll_offset_of_the_press() {
    // A 100x100 vertical area over 400 px of content; the knob sits at content y 200..240.
    let tree = scroll(
        WidgetId::key("list"),
        Axis::Vertical,
        Style::column(),
        column(vec![knob()]),
    );
    let content = Rect::new(0.0, 0.0, 100.0, 400.0);
    let knob = Rect::new(0.0, 200.0, 100.0, 240.0);
    let laid = laid_out(
        tree,
        &[Rect::new(0.0, 0.0, 100.0, 100.0), content, knob, knob],
    );
    let mut mem = memory();
    let router = Router::default();
    router.frame(&laid, &mut mem, Time::default());
    let wheel = InputEvent::Wheel {
        at: Point::new(50.0, 50.0),
        delta: Vec2::new(0.0, 150.0),
        unit: DeltaUnit::Pixels,
    };
    route(&laid, &mut mem, wheel);
    assert_eq!(mem.scroll_offset(WidgetId::key("list")).y, 150.0);
    // On screen the knob is at y 50..90: a press at y 60 is 10 px into it, not -140.
    let press = route(&laid, &mut mem, mouse(Phase::Down, 50.0, 60.0)).messages();
    assert_eq!(press, [at(PointerPhase::Press, 50.0, 10.0, Travel::Still)]);
}
