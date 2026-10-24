//! Scroll physics through the real router on explicit rects: coasting, nested hand-off, stick-to-end, reveal, thumb.
use core::time::Duration;
use kit_core::kurbo::{Point, Rect, Size, Vec2};
use kit_core::peniko::{Brush, Color};
use kit_core::{Axis, Time, Viewport, WidgetId};
use kit_input::*;
use kit_view::testing::laid_out;
use kit_view::*;

fn id(name: &str) -> WidgetId {
    WidgetId::key(name)
}
fn at(ms: u64) -> Time {
    Time(Duration::from_millis(ms))
}
fn block() -> Node<()> {
    rect(
        Style::column(),
        Brush::Solid(Color::from_rgba8(0, 0, 0, 255)),
    )
}
fn memory() -> UiMemory {
    UiMemory::new(Viewport::new(Size::new(1000.0, 1000.0), 1.0))
}
/// A 100x100 vertical area over `height` px of content: area, content column, block.
fn list(spec: ScrollSpec, height: f64) -> LaidOut<()> {
    let tree = scroll_area(id("list"), spec, Style::column(), column(vec![block()]));
    let content = Rect::new(0.0, 0.0, 100.0, height);
    laid_out(tree, &[Rect::new(0.0, 0.0, 100.0, 100.0), content, content])
}
fn touch(phase: Phase, y: f64) -> InputEvent {
    InputEvent::Pointer(PointerEvent {
        id: PointerId(1),
        kind: PointerKind::Touch,
        phase,
        at: Point::new(50.0, y),
        pressure: 1.0,
        button: kit_input::PointerButton::Primary,
    })
}
fn wheel(y: f64, dy: f64) -> InputEvent {
    InputEvent::Wheel {
        at: Point::new(50.0, y),
        delta: Vec2::new(0.0, dy),
        unit: DeltaUnit::Pixels,
    }
}
/// A finger flick: down at y=90, then `moves` steps of `step` px up, 10 ms apart, released where it stopped.
fn flick(laid: &LaidOut<()>, mem: &mut UiMemory, step: f64, moves: u64) -> u64 {
    let router = Router::default();
    router.route(laid, mem, touch(Phase::Down, 90.0), at(0));
    for k in 1..=moves {
        router.route(
            laid,
            mem,
            touch(Phase::Move, 90.0 - step * k as f64),
            at(10 * k),
        );
    }
    let end = 10 * moves;
    let last = 90.0 - step * moves as f64;
    router.route(laid, mem, touch(Phase::Up, last), at(end));
    end
}
/// Offsets at each 16 ms frame from `start` for `frames` frames.
fn coast(laid: &LaidOut<()>, mem: &mut UiMemory, start: u64, frames: u64) -> Vec<f64> {
    (0..frames)
        .map(|f| {
            Router::default().frame(laid, mem, at(start + 16 * f));
            mem.scroll_offset(id("list")).y
        })
        .collect()
}

#[test]
fn momentum_decays_and_never_passes_the_content_edge() {
    // Fast flick (~2000 px/s) over 1900 px of travel: it coasts into the edge and stops exactly there.
    let laid = list(ScrollSpec::new(Axis::Vertical), 2000.0);
    let mut mem = memory();
    let end = flick(&laid, &mut mem, 20.0, 5);
    let released = mem.scroll_offset(id("list")).y;
    assert_eq!(released, 100.0);
    // ~1.4 s to the edge, with ~600 px/s still in hand when it gets there.
    let path = coast(&laid, &mut mem, end, 100);
    assert!(path.iter().all(|y| *y <= 1900.0), "never past the edge");
    assert!(
        path[10] > released + 200.0,
        "the release velocity carries on: {}",
        path[10]
    );
    assert_eq!(*path.last().unwrap(), 1900.0, "the edge stops it");
    // The edge spends the velocity: more rows arriving (infinite scroll) must not restart the old fling.
    let longer = list(ScrollSpec::new(Axis::Vertical), 3000.0);
    let after = coast(&longer, &mut mem, end + 1600, 10);
    assert!(after.iter().all(|y| *y == 1900.0), "{after:?}");

    // Gentle flick (~400 px/s, ~80 px of coast): decelerates and comes to rest short of the edge.
    let mut mem = memory();
    let end = flick(&laid, &mut mem, 4.0, 5);
    let path = coast(&laid, &mut mem, end, 120);
    let steps: Vec<f64> = path.windows(2).map(|w| w[1] - w[0]).collect();
    assert!(steps.iter().all(|s| *s >= 0.0), "coasting never reverses");
    assert!(
        steps.windows(2).all(|w| w[1] <= w[0] + 1e-9),
        "each frame moves no further than the last (friction): {steps:?}"
    );
    let rest = *path.last().unwrap();
    assert!(
        rest > 20.0 + 40.0 && rest < 200.0,
        "rests after a short coast: {rest}"
    );
    assert_eq!(path[path.len() - 2], rest, "and stays at rest");

    // A finger that stops before lifting throws nothing (samples older than 0.1 s are dropped).
    let mut mem = memory();
    let router = Router::default();
    router.route(&laid, &mut mem, touch(Phase::Down, 90.0), at(0));
    router.route(&laid, &mut mem, touch(Phase::Move, 40.0), at(10));
    router.route(&laid, &mut mem, touch(Phase::Up, 40.0), at(400));
    let path = coast(&laid, &mut mem, 400, 30);
    assert!(
        path.iter().all(|y| *y == 50.0),
        "no fling after a pause: {path:?}"
    );
}

#[test]
fn nested_areas_hand_the_remainder_outward_at_the_inner_edge() {
    // outer 100 tall over 400; inner 50 tall over 100, at the top of the outer content.
    let inner = scroll(
        id("inner"),
        Axis::Vertical,
        Style::column(),
        column(vec![block()]),
    );
    let outer = scroll(
        id("outer"),
        Axis::Vertical,
        Style::column(),
        column(vec![inner]),
    );
    let laid = laid_out(
        outer,
        &[
            Rect::new(0.0, 0.0, 100.0, 100.0),
            Rect::new(0.0, 0.0, 100.0, 400.0),
            Rect::new(0.0, 0.0, 100.0, 50.0),
            Rect::new(0.0, 0.0, 100.0, 100.0),
            Rect::new(0.0, 0.0, 100.0, 100.0),
        ],
    );
    let offsets = |mem: &UiMemory| {
        (
            mem.scroll_offset(id("inner")).y,
            mem.scroll_offset(id("outer")).y,
        )
    };
    let router = Router::default();
    // Regression: the router used to give the whole wheel delta to the inner area, so at its edge the outer list
    // never moved while the pointer was over the inner one.
    let mut mem = memory();
    let taken = router.route(&laid, &mut mem, wheel(25.0, 80.0), at(0));
    assert!(taken.is_taken());
    assert_eq!(offsets(&mem), (50.0, 30.0));
    router.route(&laid, &mut mem, wheel(25.0, 500.0), at(1));
    assert_eq!(
        offsets(&mem),
        (50.0, 300.0),
        "inner at its edge: all of it goes outward"
    );
    // The inner area is now off screen above (outer scrolled 300); scrolling back starts with what is under the
    // pointer: the outer content.
    router.route(&laid, &mut mem, wheel(25.0, -10.0), at(2));
    assert_eq!(offsets(&mem), (50.0, 290.0));

    // A finger drag hands off the same way.
    let mut mem = memory();
    router.route(&laid, &mut mem, touch(Phase::Down, 45.0), at(0));
    router.route(&laid, &mut mem, touch(Phase::Move, 45.0 - 80.0), at(10));
    assert_eq!(offsets(&mem), (50.0, 30.0));
}

#[test]
fn stick_to_bottom_follows_growth_until_the_user_scrolls_up() {
    let spec = ScrollSpec::new(Axis::Vertical).stick_to_end();
    let router = Router::default();
    let mut mem = memory();
    let y = |mem: &UiMemory| mem.scroll_offset(id("list")).y;

    router.frame(&list(spec, 300.0), &mut mem, at(0));
    assert_eq!(y(&mem), 200.0, "a chat opens at its end");
    router.frame(&list(spec, 500.0), &mut mem, at(16));
    assert_eq!(y(&mem), 400.0, "pinned: a new message scrolls it into view");

    let laid = list(spec, 500.0);
    router.route(&laid, &mut mem, wheel(50.0, -50.0), at(20));
    router.frame(&list(spec, 600.0), &mut mem, at(32));
    assert_eq!(
        y(&mem),
        350.0,
        "scrolled up to read: growth does not yank it down"
    );
    router.frame(&list(spec, 700.0), &mut mem, at(48));
    assert_eq!(y(&mem), 350.0);

    let laid = list(spec, 700.0);
    router.route(&laid, &mut mem, wheel(50.0, 1000.0), at(50));
    assert_eq!(y(&mem), 600.0);
    router.frame(&list(spec, 800.0), &mut mem, at(64));
    assert_eq!(y(&mem), 700.0, "back at the end: pinned again");
}

#[test]
fn a_reveal_request_fires_once_and_then_lets_the_user_scroll_away() {
    // Content 1000 tall; the child keyed by data sits at 500..520, below the 100 px viewport.
    let target = interactive(
        WidgetId::of(&"target"),
        Bindings::activate(()),
        A11y::Hidden,
        block(),
    );
    let spec = ScrollSpec::new(Axis::Vertical).reveal_child(&"target", RevealAlign::Nearest);
    let tree = scroll_area(
        id("list"),
        spec,
        Style::column(),
        column(vec![target, block()]),
    );
    let rects = [
        Rect::new(0.0, 0.0, 100.0, 100.0),
        Rect::new(0.0, 0.0, 100.0, 1000.0),
        Rect::new(0.0, 500.0, 100.0, 520.0),
        Rect::new(0.0, 500.0, 100.0, 520.0),
        Rect::new(0.0, 520.0, 100.0, 1000.0),
    ];
    let laid = laid_out(tree, &rects);
    let router = Router::default();
    let mut mem = memory();
    for f in 0..60 {
        router.frame(&laid, &mut mem, at(16 * f));
    }
    assert_eq!(
        mem.scroll_offset(id("list")).y,
        420.0,
        "Nearest: the child's bottom meets the viewport's"
    );
    router.route(&laid, &mut mem, wheel(50.0, -300.0), at(1000));
    for f in 0..30 {
        router.frame(&laid, &mut mem, at(1000 + 16 * f));
    }
    assert_eq!(
        mem.scroll_offset(id("list")).y,
        120.0,
        "the same request does not fight the user"
    );
}

#[test]
fn dragging_the_thumb_maps_the_bar_onto_the_whole_content() {
    let laid = list(ScrollSpec::new(Axis::Vertical), 500.0);
    let router = Router::default();
    let mut mem = memory();
    router.frame(&laid, &mut mem, at(0));
    let mouse = |phase, y| {
        InputEvent::Pointer(PointerEvent {
            id: PointerId(0),
            kind: PointerKind::Mouse,
            phase,
            at: Point::new(96.0, y),
            pressure: 1.0,
            button: kit_input::PointerButton::Primary,
        })
    };
    // The thumb is 20 px of the 100 px bar (viewport/content), at the top; grab it 5 px down.
    let down = router.route(&laid, &mut mem, mouse(Phase::Down, 5.0), at(1));
    assert!(
        down.is_taken(),
        "a press on the bar is the bar's, not the content's"
    );
    assert_eq!(
        mem.scroll_offset(id("list")).y,
        0.0,
        "grabbing the thumb does not move it"
    );
    router.route(&laid, &mut mem, mouse(Phase::Move, 45.0), at(2));
    assert_eq!(
        mem.scroll_offset(id("list")).y,
        200.0,
        "half the thumb's travel is half the max offset"
    );
    router.route(&laid, &mut mem, mouse(Phase::Move, 300.0), at(3));
    assert_eq!(mem.scroll_offset(id("list")).y, 400.0, "clamped at the end");
}
