//! keys: focused-control key bindings (`Bindings::on_key`), modifier-aware presses (`Bindings::activate_with`),
//! field focus events (`Edit::Focus/Blur/Escape`) and focus claims (`FocusClaim`, applied by `Router::frame`).
use kit_core::kurbo::{Point, Rect, Size};
use kit_core::peniko::{Brush, Color};
use kit_core::{Time, Viewport, WidgetId};
use kit_input::*;
use kit_view::testing::laid_out;
use kit_view::*;

#[derive(Clone, Debug, PartialEq)]
enum Msg {
    Key(&'static str, Key),
    Tap(&'static str),
    Click { shift: bool, meta: bool },
    Edit(&'static str, Edit),
}

fn id(name: &str) -> WidgetId {
    WidgetId::key(name)
}
fn memory() -> UiMemory {
    UiMemory::new(Viewport::new(Size::new(1000.0, 1000.0), 1.0))
}
fn solid() -> Node<Msg> {
    kit_view::rect(
        Style::column(),
        Brush::Solid(Color::from_rgba8(0, 0, 0, 255)),
    )
}
/// A control that takes ArrowLeft/ArrowRight and passes every other key.
fn arrows(name: &'static str) -> Node<Msg> {
    let on = Bindings::none().on_key(move |k: KeyPress| match k.key {
        Key::Named(NamedKey::ArrowLeft | NamedKey::ArrowRight) => {
            KeyAnswer::Handled(Msg::Key(name, k.key))
        }
        Key::Named(_) | Key::Character(_) => KeyAnswer::Pass,
    });
    interactive(id(name), on, A11y::Hidden, solid())
}
fn field(name: &'static str) -> Node<Msg> {
    let style = TextStyle::new(12.0, Color::from_rgba8(0, 0, 0, 255));
    text_field(
        id(name),
        name,
        &EditorState::single_line("ab"),
        style,
        move |e| Msg::Edit(name, e),
    )
}
/// Each node a 40 px square in a column (interactive nodes take two rects: the node and its child).
fn scene(nodes: Vec<(Node<Msg>, usize)>) -> LaidOut<Msg> {
    let mut rects = vec![Rect::new(0.0, 0.0, 1000.0, 1000.0)];
    let mut built = Vec::new();
    for (i, (node, count)) in nodes.into_iter().enumerate() {
        let y = 50.0 * i as f64;
        rects.extend(std::iter::repeat_n(
            Rect::new(0.0, y, 40.0, y + 40.0),
            count,
        ));
        built.push(node);
    }
    laid_out(column(built), &rects)
}
fn key(k: Key, mods: Modifiers) -> InputEvent {
    InputEvent::Key {
        key: k,
        state: KeyState::Down,
        mods,
    }
}
fn named(k: NamedKey) -> InputEvent {
    key(Key::Named(k), Modifiers::default())
}
fn mouse(phase: Phase, x: f64, y: f64) -> InputEvent {
    InputEvent::Pointer(PointerEvent {
        id: PointerId(0),
        kind: PointerKind::Mouse,
        phase,
        at: Point::new(x, y),
        pressure: 1.0,
        button: kit_input::PointerButton::Primary,
    })
}
fn route(laid: &LaidOut<Msg>, mem: &mut UiMemory, event: InputEvent) -> Routing<Msg> {
    Router::default().route(laid, mem, event, Time::default())
}

/// A key binding is the focused control's alone: the other control binding the same keys hears nothing, and with
/// nothing focused the arrows stay the world's (camera keys), as they were before key bindings existed.
#[test]
fn keys_reach_only_the_focused_control() {
    let laid = scene(vec![(arrows("left"), 2), (arrows("right"), 2)]);
    let mut mem = memory();
    assert_eq!(
        route(&laid, &mut mem, named(NamedKey::ArrowRight)),
        Routing::Passed,
        "nothing focused"
    );
    route(&laid, &mut mem, named(NamedKey::Tab));
    route(&laid, &mut mem, named(NamedKey::Tab));
    assert_eq!(mem.focus(), Focus::On(id("right")));
    assert_eq!(
        route(&laid, &mut mem, named(NamedKey::ArrowLeft)).messages(),
        [Msg::Key("right", Key::Named(NamedKey::ArrowLeft))]
    );
    assert_eq!(
        mem.focus(),
        Focus::On(id("right")),
        "a handled key does not move focus"
    );
}

/// `Pass` is "not my key": ArrowUp still walks focus and a character still reaches a hotkey bound elsewhere, so a
/// control with key bindings never swallows the game's shortcuts.
#[test]
fn a_passed_key_falls_through_to_focus_walking_and_hotkeys() {
    let fire = interactive(
        id("fire"),
        Bindings::activate(Msg::Tap("fire")).with_shortcut(Key::character('f')),
        A11y::Hidden,
        solid(),
    );
    let laid = scene(vec![(arrows("a"), 2), (arrows("b"), 2), (fire, 2)]);
    let mut mem = memory();
    route(&laid, &mut mem, named(NamedKey::Tab));
    route(&laid, &mut mem, named(NamedKey::Tab));
    assert_eq!(mem.focus(), Focus::On(id("b")));
    assert_eq!(
        route(
            &laid,
            &mut mem,
            key(Key::character('f'), Modifiers::default())
        )
        .messages(),
        [Msg::Tap("fire")]
    );
    assert_eq!(
        route(&laid, &mut mem, named(NamedKey::ArrowUp)),
        Routing::Captured
    );
    assert_eq!(
        mem.focus(),
        Focus::On(id("a")),
        "a passed ArrowUp walks focus back"
    );
}

/// Shift-click and cmd-click: the modifiers the host reported (or the last key event carried) reach the press, and
/// Enter from the keyboard builds its message from the key's own modifiers.
#[test]
fn modifiers_arrive_on_click_and_on_enter() {
    let on = Bindings::activate_with(|m: Modifiers| Msg::Click {
        shift: m.shift,
        meta: m.meta,
    });
    let laid = scene(vec![(interactive(id("row"), on, A11y::Hidden, solid()), 2)]);
    let mut mem = memory();
    let shift = Modifiers {
        shift: true,
        ..Modifiers::default()
    };
    let click = |mem: &mut UiMemory| {
        route(&laid, mem, mouse(Phase::Down, 5.0, 5.0));
        route(&laid, mem, mouse(Phase::Up, 5.0, 5.0)).messages()
    };
    assert_eq!(
        click(&mut mem),
        [Msg::Click {
            shift: false,
            meta: false
        }]
    );
    route(&laid, &mut mem, InputEvent::Modifiers(shift));
    assert_eq!(
        click(&mut mem),
        [Msg::Click {
            shift: true,
            meta: false
        }]
    );
    route(&laid, &mut mem, InputEvent::Modifiers(Modifiers::default()));
    route(&laid, &mut mem, named(NamedKey::Tab));
    let meta = Modifiers {
        meta: true,
        ..Modifiers::default()
    };
    assert_eq!(
        route(&laid, &mut mem, key(Key::Named(NamedKey::Enter), meta)).messages(),
        [Msg::Click {
            shift: false,
            meta: true
        }]
    );
}

/// A field hears Escape and then Blur (so a component can cancel without host help), and Tab tells the field it
/// left Blur and the one it reached Focus.
#[test]
fn a_field_hears_focus_blur_and_escape() {
    let laid = scene(vec![(field("one"), 1), (field("two"), 1)]);
    let mut mem = memory();
    assert_eq!(
        route(&laid, &mut mem, named(NamedKey::Tab)).messages(),
        [Msg::Edit("one", Edit::Focus)]
    );
    assert_eq!(
        route(&laid, &mut mem, named(NamedKey::Tab)).messages(),
        [Msg::Edit("one", Edit::Blur), Msg::Edit("two", Edit::Focus)]
    );
    assert_eq!(
        route(&laid, &mut mem, named(NamedKey::Escape)).messages(),
        [Msg::Edit("two", Edit::Escape), Msg::Edit("two", Edit::Blur)]
    );
    assert_eq!(mem.focus(), Focus::Unfocused);
}

/// A claim focuses its node once per ticket: the player can Tab away and the same ticket does not pull focus back
/// (a level-triggered claim would fight the player every frame); a new ticket claims again. The field hears the
/// claimed focus as `Edit::Focus` on the next routed event.
#[test]
fn a_focus_claim_applies_once_per_ticket() {
    let router = Router::default();
    let claimed = |ticket: u8| {
        scene(vec![
            (arrows("a"), 2),
            (field("name").claim_focus(&ticket), 1),
        ])
    };
    let mut mem = memory();
    let laid = claimed(1);
    router.frame(&laid, &mut mem, Time::default());
    assert_eq!(mem.focus(), Focus::On(id("name")));
    assert_eq!(
        route(&laid, &mut mem, named(NamedKey::Tab)).messages(),
        [
            Msg::Edit("name", Edit::Focus),
            Msg::Edit("name", Edit::Blur)
        ]
    );
    router.frame(&laid, &mut mem, Time::default());
    assert_eq!(
        mem.focus(),
        Focus::On(id("a")),
        "the same ticket does not claim twice"
    );
    let laid = claimed(2);
    router.frame(&laid, &mut mem, Time::default());
    assert_eq!(
        mem.focus(),
        Focus::On(id("name")),
        "a new ticket claims again"
    );
}
