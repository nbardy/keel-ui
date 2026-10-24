use kit_core::kurbo::{Point, Rect, Size, Vec2};
use kit_core::peniko::{Brush, Color};
use kit_core::{Axis, Epoch, Time, Viewport, WidgetId};
use kit_input::*;
use kit_view::testing::laid_out;
use kit_view::*;

#[derive(Clone, Debug, PartialEq)]
enum Msg {
    Tap(&'static str),
    Drop { from: u64, to: &'static str },
    Hover(&'static str, bool),
    Edit(Edit),
}

const SLOT: u32 = 1;
const OTHER: u32 = 2;

fn id(name: &str) -> WidgetId {
    WidgetId::key(name)
}
fn rect(x: f64, y: f64) -> Rect {
    Rect::new(x, y, x + 40.0, y + 40.0)
}
fn solid() -> Node<Msg> {
    kit_view::rect(
        Style::column(),
        Brush::Solid(Color::from_rgba8(0, 0, 0, 255)),
    )
}
fn region(name: &str, on: Bindings<Msg>, at: Rect) -> (Node<Msg>, [Rect; 2]) {
    (interactive(id(name), on, A11y::Hidden, solid()), [at, at])
}
fn scene(parts: Vec<(Node<Msg>, [Rect; 2])>) -> LaidOut<Msg> {
    let mut rects = vec![Rect::new(0.0, 0.0, 1000.0, 1000.0)];
    let mut nodes = Vec::new();
    for (node, r) in parts {
        nodes.push(node);
        rects.extend(r);
    }
    laid_out(column(nodes), &rects)
}
fn ptr(pointer: u64, kind: PointerKind, phase: Phase, x: f64, y: f64) -> InputEvent {
    InputEvent::Pointer(PointerEvent {
        id: PointerId(pointer),
        kind,
        phase,
        at: Point::new(x, y),
        pressure: 1.0,
    })
}
fn mouse(phase: Phase, x: f64, y: f64) -> InputEvent {
    ptr(0, PointerKind::Mouse, phase, x, y)
}
fn memory() -> UiMemory {
    UiMemory::new(Viewport::new(Size::new(1000.0, 1000.0), 1.0))
}
fn send(laid: &LaidOut<Msg>, mem: &mut UiMemory, events: Vec<InputEvent>) -> Vec<Msg> {
    let router = Router::default();
    events
        .into_iter()
        .flat_map(|e| router.route(laid, mem, e, Time::default()).messages())
        .collect()
}

fn drag_scene(epoch: Epoch) -> LaidOut<Msg> {
    scene(vec![
        region(
            "src",
            Bindings::<Msg>::none().with_drag(DragPayload {
                kind: SLOT,
                value: 9,
            }),
            rect(0.0, 0.0),
        ),
        region(
            "dst",
            Bindings::<Msg>::none().with_drop(SLOT, |d| Msg::Drop {
                from: d.payload.value,
                to: "dst",
            }),
            rect(100.0, 0.0),
        ),
    ])
    .with_epoch(epoch)
}

#[test]
fn drag_that_crosses_an_epoch_change_emits_nothing_but_the_same_drag_without_one_drops() {
    let drag = |up_epoch: Epoch| {
        let mut mem = memory();
        let before = drag_scene(Epoch([1, 1]));
        send(
            &before,
            &mut mem,
            vec![
                mouse(Phase::Down, 10.0, 10.0),
                mouse(Phase::Move, 110.0, 10.0),
            ],
        );
        let after = drag_scene(up_epoch);
        send(&after, &mut mem, vec![mouse(Phase::Up, 110.0, 10.0)])
    };
    assert_eq!(
        drag(Epoch([1, 1])),
        vec![Msg::Drop { from: 9, to: "dst" }],
        "control: same epoch drops"
    );
    assert_eq!(drag(Epoch([1, 2])), vec![], "turn epoch changed mid-drag");
    assert_eq!(drag(Epoch([2, 1])), vec![], "screen epoch changed mid-drag");
}

#[test]
fn stale_drag_shows_no_ghost_and_stale_tap_does_not_fire() {
    let mut mem = memory();
    let tap = |epoch| {
        scene(vec![region(
            "b",
            Bindings::activate(Msg::Tap("b")),
            rect(0.0, 0.0),
        )])
        .with_epoch(epoch)
    };
    send(
        &tap(Epoch([0, 0])),
        &mut mem,
        vec![mouse(Phase::Down, 5.0, 5.0)],
    );
    assert_eq!(
        send(
            &tap(Epoch([0, 1])),
            &mut mem,
            vec![mouse(Phase::Up, 5.0, 5.0)]
        ),
        vec![]
    );

    let mut mem = memory();
    send(
        &drag_scene(Epoch([0, 0])),
        &mut mem,
        vec![mouse(Phase::Down, 5.0, 5.0), mouse(Phase::Move, 60.0, 5.0)],
    );
    assert_eq!(mem.live_drags(Epoch([0, 0])).len(), 1);
    assert!(mem.live_drags(Epoch([0, 1])).is_empty());
}

#[test]
fn drop_lands_only_on_drop_enabled_targets_that_accept_the_payload_kind() {
    let target_at = |x| rect(x, 0.0);
    let laid = scene(vec![
        region(
            "src",
            Bindings::<Msg>::none().with_drag(DragPayload {
                kind: SLOT,
                value: 5,
            }),
            rect(0.0, 100.0),
        ),
        region(
            "plain",
            Bindings::activate(Msg::Tap("plain")),
            target_at(100.0),
        ),
        region(
            "wrongkind",
            Bindings::<Msg>::none().with_drop(OTHER, |_| Msg::Drop {
                from: 0,
                to: "wrongkind",
            }),
            target_at(200.0),
        ),
        region(
            "good",
            Bindings::<Msg>::none().with_drop(SLOT, |d| Msg::Drop {
                from: d.payload.value,
                to: "good",
            }),
            target_at(300.0),
        ),
    ]);
    let drag_to = |x: f64| {
        let mut mem = memory();
        send(
            &laid,
            &mut mem,
            vec![
                mouse(Phase::Down, 10.0, 110.0),
                mouse(Phase::Move, x, 10.0),
                mouse(Phase::Up, x, 10.0),
            ],
        )
    };
    assert_eq!(
        drag_to(110.0),
        vec![],
        "activate-only region is not a drop target (and the tap must not fire)"
    );
    assert_eq!(drag_to(210.0), vec![], "kind mismatch");
    assert_eq!(drag_to(700.0), vec![], "empty space");
    assert_eq!(
        drag_to(310.0),
        vec![Msg::Drop {
            from: 5,
            to: "good"
        }]
    );
}

#[test]
fn capture_holds_across_moves_outside_the_source() {
    let laid = scene(vec![
        region(
            "a",
            Bindings::activate(Msg::Tap("a"))
                .with_hover(Msg::Hover("a", true), Msg::Hover("a", false)),
            rect(0.0, 0.0),
        ),
        region(
            "b",
            Bindings::activate(Msg::Tap("b"))
                .with_hover(Msg::Hover("b", true), Msg::Hover("b", false)),
            rect(100.0, 0.0),
        ),
    ]);
    let mut mem = memory();
    // Press a, wander over b, come back, release on a: a fires, b never hovers or fires.
    let msgs = send(
        &laid,
        &mut mem,
        vec![
            mouse(Phase::Move, 5.0, 5.0),
            mouse(Phase::Down, 5.0, 5.0),
            mouse(Phase::Move, 110.0, 5.0),
            mouse(Phase::Move, 500.0, 500.0),
            mouse(Phase::Move, 5.0, 5.0),
            mouse(Phase::Up, 5.0, 5.0),
        ],
    );
    assert_eq!(msgs, vec![Msg::Hover("a", true), Msg::Tap("a")]);
    // Press a, release over b: nothing fires (tap needs release on the pressed region).
    let msgs = send(
        &laid,
        &mut mem,
        vec![
            mouse(Phase::Down, 5.0, 5.0),
            mouse(Phase::Move, 110.0, 5.0),
            mouse(Phase::Up, 110.0, 5.0),
        ],
    );
    assert_eq!(
        msgs,
        vec![Msg::Hover("a", false), Msg::Hover("b", true)],
        "only hover on release, no taps"
    );
}

#[test]
fn pointers_are_independent_and_cancelling_one_leaves_the_other() {
    let laid = scene(vec![
        region("a", Bindings::activate(Msg::Tap("a")), rect(0.0, 0.0)),
        region("b", Bindings::activate(Msg::Tap("b")), rect(100.0, 0.0)),
        region(
            "src",
            Bindings::<Msg>::none().with_drag(DragPayload {
                kind: SLOT,
                value: 1,
            }),
            rect(0.0, 100.0),
        ),
        region(
            "dst",
            Bindings::<Msg>::none().with_drop(SLOT, |d| Msg::Drop {
                from: d.payload.value,
                to: "dst",
            }),
            rect(100.0, 100.0),
        ),
    ]);
    let touch = |p, phase, x, y| ptr(p, PointerKind::Touch, phase, x, y);
    let mut mem = memory();
    let msgs = send(
        &laid,
        &mut mem,
        vec![
            touch(1, Phase::Down, 10.0, 110.0),
            touch(2, Phase::Down, 110.0, 10.0),
            touch(1, Phase::Move, 110.0, 110.0), // finger 1 drags to dst
            touch(2, Phase::Up, 110.0, 10.0),    // finger 2 taps b meanwhile
            touch(1, Phase::Up, 110.0, 110.0),
        ],
    );
    assert_eq!(msgs, vec![Msg::Tap("b"), Msg::Drop { from: 1, to: "dst" }]);

    let msgs = send(
        &laid,
        &mut mem,
        vec![
            touch(1, Phase::Down, 10.0, 10.0),
            touch(2, Phase::Down, 110.0, 10.0),
            touch(1, Phase::Cancel, 10.0, 10.0),
            touch(1, Phase::Up, 10.0, 10.0),
            touch(2, Phase::Up, 110.0, 10.0),
        ],
    );
    assert_eq!(msgs, vec![Msg::Tap("b")]);
}

#[test]
fn disabled_regions_are_skipped_and_do_not_capture() {
    let laid = scene(vec![(
        disabled(id("d"), A11y::Hidden, solid()),
        [rect(0.0, 0.0); 2],
    )]);
    let mut mem = memory();
    assert_eq!(
        send(
            &laid,
            &mut mem,
            vec![mouse(Phase::Down, 5.0, 5.0), mouse(Phase::Up, 5.0, 5.0)]
        ),
        vec![]
    );
    assert!(!mem.is_pressed(id("d")));
}

fn list_scene() -> LaidOut<Msg> {
    // 100x100 viewport over 100x400 content holding a tappable row at the top.
    let viewport = Rect::new(0.0, 0.0, 100.0, 100.0);
    let content = Rect::new(0.0, 0.0, 100.0, 400.0);
    let row = interactive(
        id("row"),
        Bindings::activate(Msg::Tap("row")),
        A11y::Hidden,
        solid(),
    );
    let tree = scroll(
        id("list"),
        Axis::Vertical,
        Style::column(),
        column(vec![row]),
    );
    // scroll, content column, row, row shape
    laid_out(
        tree,
        &[
            viewport,
            content,
            Rect::new(0.0, 0.0, 100.0, 50.0),
            Rect::new(0.0, 0.0, 100.0, 50.0),
        ],
    )
}

#[test]
fn wheel_and_touch_pan_scroll_within_bounds_and_a_pan_is_not_a_tap() {
    let laid = list_scene();
    let mut mem = memory();
    send(
        &laid,
        &mut mem,
        vec![InputEvent::Wheel {
            at: Point::new(50.0, 50.0),
            delta: Vec2::new(0.0, 2.0),
            unit: DeltaUnit::Lines,
        }],
    );
    assert_eq!(mem.scroll_offset(id("list")), Vec2::new(0.0, 80.0));
    send(
        &laid,
        &mut mem,
        vec![InputEvent::Wheel {
            at: Point::new(50.0, 50.0),
            delta: Vec2::new(0.0, 99.0),
            unit: DeltaUnit::Pages,
        }],
    );
    assert_eq!(
        mem.scroll_offset(id("list")),
        Vec2::new(0.0, 300.0),
        "clamped to content - viewport"
    );
    send(
        &laid,
        &mut mem,
        vec![InputEvent::Wheel {
            at: Point::new(500.0, 500.0),
            delta: Vec2::new(0.0, -5.0),
            unit: DeltaUnit::Pixels,
        }],
    );
    assert_eq!(
        mem.scroll_offset(id("list")).y,
        300.0,
        "wheel outside the area is ignored"
    );

    let mut mem = memory();
    let msgs = send(
        &laid,
        &mut mem,
        vec![
            ptr(1, PointerKind::Touch, Phase::Down, 50.0, 40.0),
            ptr(1, PointerKind::Touch, Phase::Move, 50.0, 10.0),
            ptr(1, PointerKind::Touch, Phase::Up, 50.0, 10.0),
        ],
    );
    assert_eq!(
        msgs,
        vec![],
        "dragging the finger over a row scrolls instead of tapping"
    );
    assert_eq!(mem.scroll_offset(id("list")), Vec2::new(0.0, 30.0));
}

#[test]
fn text_field_edits_follow_focus() {
    let state = EditorState::single_line("a\u{e9}");
    let field = text_field(
        id("name"),
        "Name",
        &state,
        TextStyle::new(12.0, Color::from_rgba8(0, 0, 0, 255)),
        Msg::Edit,
    );
    let laid = laid_out(
        column(vec![field]),
        &[Rect::new(0.0, 0.0, 200.0, 200.0), rect(0.0, 0.0)],
    );
    let mut mem = memory();
    assert_eq!(
        send(&laid, &mut mem, vec![InputEvent::Text("x".into())]),
        vec![],
        "no focus, no edit"
    );
    let named = |k| InputEvent::Key {
        key: Key::Named(k),
        state: KeyState::Down,
        mods: Modifiers::default(),
    };
    let msgs = send(
        &laid,
        &mut mem,
        vec![
            mouse(Phase::Down, 5.0, 5.0),
            mouse(Phase::Up, 5.0, 5.0),
            named(NamedKey::ArrowLeft),
            InputEvent::Text("z".into()),
        ],
    );
    assert_eq!(mem.focus(), Focus::On(id("name")));
    assert_eq!(
        msgs[0],
        Msg::Edit(Edit::Focus),
        "the field hears it gained focus first"
    );
    assert!(
        matches!(msgs[1], Msg::Edit(Edit::Select { .. })),
        "the press places the caret"
    );
    assert_eq!(
        msgs[2..],
        [
            Msg::Edit(Edit::Move {
                to: Motion::Left,
                extend: false
            }),
            Msg::Edit(Edit::Type("z".into()))
        ]
    );
    // Clicking elsewhere blurs, and the field hears it (a component commits or cancels on it).
    assert_eq!(
        send(&laid, &mut mem, vec![mouse(Phase::Down, 500.0, 500.0)]),
        [Msg::Edit(Edit::Blur)]
    );
    assert_eq!(mem.focus(), Focus::Unfocused);
}

fn route(laid: &LaidOut<Msg>, mem: &mut UiMemory, event: InputEvent) -> Routing<Msg> {
    Router::default().route(laid, mem, event, Time::default())
}
fn key_down(c: char, mods: Modifiers) -> InputEvent {
    InputEvent::Key {
        key: Key::character(c),
        state: KeyState::Down,
        mods,
    }
}

/// The host must not re-ask "did the kit take this press": a press on a control is taken from Down to Up, the Up
/// activates, and a press on empty space is `Passed` throughout so the world behind keeps its camera drag.
#[test]
fn routing_tells_the_host_who_owns_each_event_of_a_press() {
    let laid = scene(vec![region(
        "go",
        Bindings::activate(Msg::Tap("go")),
        rect(0.0, 0.0),
    )]);
    let mut mem = memory();
    assert_eq!(
        route(&laid, &mut mem, mouse(Phase::Down, 5.0, 5.0)),
        Routing::Captured
    );
    assert_eq!(
        route(&laid, &mut mem, mouse(Phase::Move, 300.0, 300.0)),
        Routing::Captured,
        "a held press owns its moves even outside the control"
    );
    assert_eq!(
        route(&laid, &mut mem, mouse(Phase::Up, 5.0, 5.0)),
        Routing::Handled(vec![Msg::Tap("go")])
    );
    assert_eq!(
        route(&laid, &mut mem, mouse(Phase::Move, 5.0, 5.0)),
        Routing::Passed,
        "hover is not exclusive"
    );
    assert_eq!(
        route(&laid, &mut mem, mouse(Phase::Down, 500.0, 500.0)),
        Routing::Passed
    );
    assert_eq!(
        route(&laid, &mut mem, mouse(Phase::Up, 500.0, 500.0)),
        Routing::Passed
    );
}

/// Regression guard for J-web-2: `.hotkey` is a binding the router owns. Before it, the "1/2/3" badges were
/// decoration and the web host could not route a key at all.
#[test]
fn a_shortcut_activates_only_an_enabled_control_and_never_while_a_field_has_focus() {
    let bound = |name, key: char| {
        region(
            name,
            Bindings::activate(Msg::Tap(name)).with_shortcut(Key::character(key)),
            rect(0.0, 0.0),
        )
    };
    let disabled_one = (
        Node::Interactive {
            identity: id("off").into(),
            on: Bindings::activate(Msg::Tap("off")).with_shortcut(Key::character('2')),
            a11y: A11y::Hidden,
            tooltip: Tooltip::None,
            availability: Availability::Disabled,
            child: Box::new(solid()),
        },
        [rect(100.0, 0.0), rect(100.0, 0.0)],
    );
    let laid = scene(vec![bound("one", '1'), disabled_one]);
    let mut mem = memory();
    let plain = Modifiers::default();
    assert_eq!(
        route(&laid, &mut mem, key_down('1', plain)),
        Routing::Handled(vec![Msg::Tap("one")])
    );
    assert_eq!(
        route(&laid, &mut mem, key_down('M', plain)),
        Routing::Passed,
        "an unbound key reaches the world"
    );
    assert_eq!(
        route(&laid, &mut mem, key_down('2', plain)),
        Routing::Passed,
        "a disabled control's key is not an activation"
    );
    let ctrl = Modifiers {
        ctrl: true,
        ..plain
    };
    assert_eq!(
        route(&laid, &mut mem, key_down('1', ctrl)),
        Routing::Passed,
        "Ctrl+1 is the browser's tab switch, not the control's key"
    );

    // The same key, with a text field focused: typing a "1" must not fire the control.
    let field = text_field(
        id("name"),
        "Name",
        &EditorState::single_line(""),
        TextStyle::new(12.0, Color::from_rgba8(0, 0, 0, 255)),
        Msg::Edit,
    );
    let laid = laid_out(
        column(vec![
            interactive(
                id("one"),
                Bindings::activate(Msg::Tap("one")).with_shortcut(Key::character('1')),
                A11y::Hidden,
                solid(),
            ),
            field,
        ]),
        &[
            Rect::new(0.0, 0.0, 400.0, 400.0),
            rect(0.0, 0.0),
            rect(0.0, 0.0),
            Rect::new(0.0, 100.0, 200.0, 140.0),
        ],
    );
    let mut mem = memory();
    route(&laid, &mut mem, mouse(Phase::Down, 5.0, 110.0));
    assert_eq!(mem.focus(), Focus::On(id("name")));
    assert_eq!(
        route(&laid, &mut mem, key_down('1', plain)),
        Routing::Captured,
        "a focused field owns the keyboard; the shortcut must not fire"
    );
    assert_eq!(
        route(&laid, &mut mem, InputEvent::Text("1".into())),
        Routing::Handled(vec![Msg::Edit(Edit::Type("1".into()))])
    );
}

#[test]
fn two_controls_bound_to_one_key_are_rejected_when_laid_out() {
    let twin = |name| {
        interactive(
            id(name),
            Bindings::activate(Msg::Tap("x")).with_shortcut(Key::character('2')),
            A11y::Hidden,
            solid(),
        )
    };
    let tree = column(vec![twin("a"), twin("b")]);
    let rects = vec![rect(0.0, 0.0); tree.count()];
    let placed: Vec<_> = rects
        .iter()
        .map(|&rect| Placed {
            rect,
            shown: Shown::Visible,
        })
        .collect();
    let error = LaidOut::assemble(
        tree,
        Viewport::new(Size::new(100.0, 100.0), 1.0),
        &placed,
        &mut kit_view::testing::text_system(),
    )
    .err();
    assert_eq!(
        error,
        Some(LayoutError::DuplicateShortcut {
            key: Key::character('2')
        })
    );
}
