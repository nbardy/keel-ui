//! Portals, focus and the component-facing interaction reads, through the real router on laid-out trees.
use kit_core::kurbo::{Point, Rect, Size, Vec2};
use kit_core::peniko::{Brush, Color};
use kit_core::{Time, Viewport, WidgetId};
use kit_input::*;
use kit_state::{Drag, Interaction};
use kit_view::testing::laid_out;
use kit_view::*;

#[derive(Clone, Debug, PartialEq)]
enum Msg {
    Tap(&'static str),
    Close(&'static str),
}

fn id(name: &str) -> WidgetId {
    WidgetId::key(name)
}
fn at(x: f64, y: f64, w: f64, h: f64) -> Rect {
    Rect::new(x, y, x + w, y + h)
}
fn solid() -> Node<Msg> {
    kit_view::rect(
        Style::column(),
        Brush::Solid(Color::from_rgba8(0, 0, 0, 255)),
    )
}

/// A subtree and its pre-order rects, so scenes are written with explicit positions.
struct Part {
    node: Node<Msg>,
    rects: Vec<Rect>,
}
fn button(name: &'static str, rect: Rect) -> Part {
    Part {
        node: interactive(
            id(name),
            Bindings::activate(Msg::Tap(name)),
            A11y::Hidden,
            solid(),
        ),
        rects: vec![rect, rect],
    }
}
fn disabled_button(name: &'static str, rect: Rect) -> Part {
    Part {
        node: disabled(id(name), A11y::Hidden, solid()),
        rects: vec![rect, rect],
    }
}
/// A box holding `parts` (their rects are their own).
fn panel_of(rect: Rect, parts: Vec<Part>) -> Part {
    let mut rects = vec![rect];
    let mut nodes = Vec::new();
    for p in parts {
        rects.extend(p.rects);
        nodes.push(p.node);
    }
    Part {
        node: column(nodes),
        rects,
    }
}
fn portal_of(layer: Layer, dismissal: Dismissal<Msg>, content: Part) -> Part {
    let anchor = PortalAnchor::Viewport(Anchor {
        h: AnchorH::Start,
        v: AnchorV::Start,
        offset: Vec2::ZERO,
        safe_area: SafeArea::Respect,
    });
    // A portal's rect is its child's.
    let mut rects = vec![content.rects[0]];
    rects.extend(content.rects);
    Part {
        node: Node::Portal {
            layer,
            anchor,
            dismissal,
            child: Box::new(content.node),
        },
        rects,
    }
}
fn screen(parts: Vec<Part>) -> LaidOut<Msg> {
    let root = panel_of(at(0.0, 0.0, 1000.0, 1000.0), parts);
    laid_out(root.node, &root.rects)
}

fn memory() -> UiMemory {
    UiMemory::new(Viewport::new(Size::new(1000.0, 1000.0), 1.0))
}
fn pointer(phase: Phase, x: f64, y: f64) -> InputEvent {
    InputEvent::Pointer(PointerEvent {
        id: PointerId(0),
        kind: PointerKind::Mouse,
        phase,
        at: Point::new(x, y),
        pressure: 1.0,
    })
}
fn key(named: NamedKey, shift: bool) -> InputEvent {
    InputEvent::Key {
        key: Key::Named(named),
        state: KeyState::Down,
        mods: Modifiers {
            shift,
            ..Modifiers::default()
        },
    }
}
fn route(laid: &LaidOut<Msg>, mem: &mut UiMemory, event: InputEvent) -> Routing<Msg> {
    Router::default().route(laid, mem, event, Time::default())
}
fn click(laid: &LaidOut<Msg>, mem: &mut UiMemory, x: f64, y: f64) -> Vec<Msg> {
    let mut sent = route(laid, mem, pointer(Phase::Down, x, y)).messages();
    sent.extend(route(laid, mem, pointer(Phase::Up, x, y)).messages());
    sent
}

/// Base button at the top-left; a dismissible popup (one button) far from it.
fn popup_scene() -> LaidOut<Msg> {
    let popup = portal_of(
        Layer::Popup,
        Dismissal::Intent(Msg::Close("popup")),
        panel_of(
            at(300.0, 300.0, 100.0, 100.0),
            vec![button("inside", at(310.0, 310.0, 40.0, 40.0))],
        ),
    );
    screen(vec![button("base", at(0.0, 0.0, 100.0, 40.0)), popup])
}

/// A press outside a popup closes it exactly once and is spent: the base button under the pointer is not pressed
/// (a menu that dismissed AND activated whatever was behind it would fire unrelated commands from a dismissal click).
#[test]
fn click_outside_dismisses_once_and_does_not_reach_the_base_layer() {
    let laid = popup_scene();
    let mut mem = memory();
    let down = route(&laid, &mut mem, pointer(Phase::Down, 20.0, 20.0));
    assert_eq!(down, Routing::Handled(vec![Msg::Close("popup")]));
    let up = route(&laid, &mut mem, pointer(Phase::Up, 20.0, 20.0));
    assert_eq!(
        up,
        Routing::Passed,
        "the release dismisses nothing and starts nothing"
    );
    // A press inside the popup is the popup's: it activates, and nothing is dismissed.
    assert_eq!(
        click(&laid, &mut mem, 320.0, 320.0),
        vec![Msg::Tap("inside")]
    );
}

#[test]
fn escape_sends_the_topmost_dismiss_intent() {
    let laid = popup_scene();
    let mut mem = memory();
    let routed = route(&laid, &mut mem, key(NamedKey::Escape, false));
    assert_eq!(routed, Routing::Handled(vec![Msg::Close("popup")]));
}

/// Layers, not tree order, decide who is hit: a toast button written BEFORE a popup in the tree still wins where they
/// overlap, because Toast paints above Popup.
#[test]
fn the_higher_layer_wins_a_press_where_portals_overlap() {
    let toast = portal_of(
        Layer::Toast,
        Dismissal::Never,
        panel_of(
            at(100.0, 100.0, 100.0, 100.0),
            vec![button("toast", at(100.0, 100.0, 100.0, 100.0))],
        ),
    );
    let popup = portal_of(
        Layer::Popup,
        Dismissal::Never,
        panel_of(
            at(100.0, 100.0, 100.0, 100.0),
            vec![button("popup", at(100.0, 100.0, 100.0, 100.0))],
        ),
    );
    let laid = screen(vec![toast, popup]);
    assert_eq!(
        click(&laid, &mut memory(), 150.0, 150.0),
        vec![Msg::Tap("toast")]
    );
}

fn modal_scene() -> LaidOut<Msg> {
    let modal = portal_of(
        Layer::Modal,
        Dismissal::Never,
        panel_of(
            at(300.0, 300.0, 200.0, 100.0),
            vec![
                button("m1", at(300.0, 300.0, 40.0, 40.0)),
                button("m2", at(360.0, 300.0, 40.0, 40.0)),
            ],
        ),
    );
    screen(vec![
        button("a", at(0.0, 0.0, 40.0, 40.0)),
        button("b", at(60.0, 0.0, 40.0, 40.0)),
        modal,
    ])
}

fn focused(mem: &UiMemory) -> Focus {
    mem.focus()
}

/// A modal owns input: a press on the screen behind it does nothing (and is not the world's either), and Tab cycles
/// the modal's own controls instead of walking out into the blocked screen.
#[test]
fn a_modal_blocks_the_base_and_tab_stays_inside_it() {
    let laid = modal_scene();
    let mut mem = memory();
    let blocked = route(&laid, &mut mem, pointer(Phase::Down, 20.0, 20.0));
    assert_eq!(blocked, Routing::Captured);
    assert_eq!(
        route(&laid, &mut mem, pointer(Phase::Up, 20.0, 20.0)),
        Routing::Captured
    );

    let visited: Vec<Focus> = (0..4)
        .map(|_| {
            route(&laid, &mut mem, key(NamedKey::Tab, false));
            focused(&mem)
        })
        .collect();
    let (m1, m2) = (Focus::On(id("m1")), Focus::On(id("m2")));
    assert_eq!(visited, [m1, m2, m1, m2]);
    // Shift-Tab runs the same ring backwards.
    route(&laid, &mut mem, key(NamedKey::Tab, true));
    assert_eq!(focused(&mem), m1);
    // The modal's own control works.
    assert_eq!(click(&laid, &mut mem, 310.0, 310.0), vec![Msg::Tap("m1")]);
}

/// Without a modal, Tab walks the whole screen in tree order, skips what cannot be pressed, and Enter activates.
#[test]
fn tab_visits_enabled_controls_in_tree_order_and_enter_activates() {
    let laid = screen(vec![
        button("a", at(0.0, 0.0, 40.0, 40.0)),
        disabled_button("off", at(50.0, 0.0, 40.0, 40.0)),
        button("b", at(100.0, 0.0, 40.0, 40.0)),
    ]);
    let mut mem = memory();
    let mut order = Vec::new();
    for _ in 0..3 {
        route(&laid, &mut mem, key(NamedKey::Tab, false));
        order.push(focused(&mem));
    }
    assert_eq!(
        order,
        [Focus::On(id("a")), Focus::On(id("b")), Focus::On(id("a"))]
    );
    route(&laid, &mut mem, key(NamedKey::Tab, true));
    assert_eq!(focused(&mem), Focus::On(id("b")));
    let enter = route(&laid, &mut mem, key(NamedKey::Enter, false));
    assert_eq!(enter, Routing::Handled(vec![Msg::Tap("b")]));
}

/// A host's focus request for a key the tree does not hold is reported and counted, not dropped on the floor, and does
/// not disturb the focus that was there.
#[test]
fn a_focus_request_for_a_missing_key_is_counted() {
    let laid = screen(vec![button("a", at(0.0, 0.0, 40.0, 40.0))]);
    let mut mem = memory();
    let router = Router::default();
    assert_eq!(
        router.request_focus(&laid, &mut mem, id("a"), Time::default()),
        FocusOutcome::Focused
    );
    assert_eq!(mem.focus_misses(), 0);
    assert_eq!(
        router.request_focus(&laid, &mut mem, id("nope"), Time::default()),
        FocusOutcome::Missing
    );
    assert_eq!(mem.focus_misses(), 1);
    assert_eq!(focused(&mem), Focus::On(id("a")));
}

/// `Interaction::drag` for components: `total` runs from where the press began, `delta` from where the pointer was
/// when the host last closed a frame, and a frame with no movement reads a zero delta (not the previous one).
#[test]
fn drag_total_and_delta_follow_the_pointer_across_frames() {
    let src = Part {
        node: interactive(
            id("src"),
            Bindings::none().with_drag(DragPayload { kind: 1, value: 0 }),
            A11y::Hidden,
            solid(),
        ),
        rects: vec![at(0.0, 0.0, 40.0, 40.0), at(0.0, 0.0, 40.0, 40.0)],
    };
    let laid = screen(vec![src]);
    let mut mem = memory();
    assert_eq!(mem.drag(id("src")), Drag::Idle);
    route(&laid, &mut mem, pointer(Phase::Down, 10.0, 10.0));
    route(&laid, &mut mem, pointer(Phase::Move, 20.0, 10.0)); // past the 6 px slop: the drag begins
    assert_eq!(
        mem.drag(id("src")),
        Drag::Held {
            delta: Vec2::new(10.0, 0.0),
            total: Vec2::new(10.0, 0.0)
        }
    );
    mem.end_frame();
    route(&laid, &mut mem, pointer(Phase::Move, 35.0, 25.0));
    assert_eq!(
        mem.drag(id("src")),
        Drag::Held {
            delta: Vec2::new(15.0, 15.0),
            total: Vec2::new(25.0, 15.0)
        }
    );
    mem.end_frame();
    assert_eq!(
        mem.drag(id("src")),
        Drag::Held {
            delta: Vec2::ZERO,
            total: Vec2::new(25.0, 15.0)
        }
    );
    route(&laid, &mut mem, pointer(Phase::Up, 35.0, 25.0));
    assert_eq!(mem.drag(id("src")), Drag::Idle);
}
