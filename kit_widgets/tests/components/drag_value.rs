//! Real pointer drags and clicks, typing, Enter, Escape and Tab: no host duties remain (the field claims focus and
//! hears Escape/Blur itself).
use kit_core::kurbo::Point;
use kit_core::{Key, MotionPref, NamedKey, Theme, Time};
use kit_input::{InputEvent, Modifiers, Phase, PointerEvent, PointerId, PointerKind};
use kit_state::{Cx, Host, Ident, Intent, Life};
use kit_test::Handle;
use kit_view::{Node, Role, column};
use kit_widgets::drag_value::{DragValue, DragValueChanged, TypedError, drag_value};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Id;
struct App {
    value: f64,
    changes: Vec<Result<f64, TypedError>>,
}
impl Ident<App> for Id {
    fn life(&self, _: &App) -> Life {
        Life::Live
    }
}
impl Host for App {
    type Ask = DragValueChanged<Id>;
    fn now(&self) -> Time {
        Time::default()
    }
    fn motion(&self) -> MotionPref {
        MotionPref::Full
    }
    fn ask(&mut self, ask: Self::Ask) {
        if let Ok(value) = ask.value {
            self.value = value;
        }
        self.changes.push(ask.value);
    }
}
fn props<'a>(cx: &Cx<'_, App>, theme: &'a Theme) -> DragValue<'a, Id> {
    DragValue {
        theme,
        key: Id,
        label: "Speed",
        value: cx.app().value,
        speed: 0.5,
        range: -10.0..=10.0,
    }
}
fn view(cx: &Cx<'_, App>, theme: &Theme) -> Node<Intent<App>> {
    column([drag_value(cx, props(cx, theme))]).pad(theme.spacing.md)
}
type View = fn(&Cx<'_, App>, &Theme) -> Node<Intent<App>>;
fn handle() -> Handle<App, View> {
    Handle::new(
        App {
            value: 4.0,
            changes: Vec::new(),
        },
        (200.0, 80.0),
        view,
    )
}
fn key(h: &mut Handle<App, View>, n: NamedKey) {
    h.press_key(Key::Named(n));
}
/// The button's top-left is (8, 8) (theme `md` padding); (14, 14) is inside it.
fn mouse(h: &mut Handle<App, View>, phase: Phase, x: f64, y: f64) {
    h.input(InputEvent::Pointer(PointerEvent {
        id: PointerId(1),
        kind: PointerKind::Mouse,
        phase,
        at: Point::new(x, y),
        pressure: 1.0,
    }));
}
/// A click (no travel) opens the text field, as in egui, and the field claims focus: typing goes straight in.
fn begin(h: &mut Handle<App, View>) {
    h.click(Role::Button, "Speed");
    select_all(h);
}
fn select_all(h: &mut Handle<App, View>) {
    h.press_key_with(
        Key::character('a'),
        Modifiers {
            ctrl: true,
            ..Modifiers::default()
        },
    );
}
#[test]
fn golden_idle_and_text_entry() {
    let mut h = handle();
    h.assert_golden("drag_value_idle");
    begin(&mut h);
    h.type_text("7.5");
    h.assert_golden("drag_value_editing");
}
#[test]
fn typed_invalid_text_is_rejected_without_changing_value() {
    let mut h = handle();
    begin(&mut h);
    for (typed, error) in [
        ("oops", TypedError::InvalidNumber),
        ("NaN", TypedError::NotFinite),
        ("inf", TypedError::NotFinite),
    ] {
        select_all(&mut h);
        h.type_text(typed);
        key(&mut h, NamedKey::Enter);
        assert_eq!(h.app().value, 4.0);
        assert_eq!(h.app().changes.last(), Some(&Err(error)));
        assert!(h.shows(Role::TextBox, "Speed"));
    }
    select_all(&mut h);
    h.type_text(" −1 2.5 ");
    key(&mut h, NamedKey::Enter);
    assert_eq!(
        h.app().value,
        -10.0,
        "egui parser normalizes whitespace/minus; typed values clamp"
    );
    assert!(!h.shows(Role::TextBox, "Speed"));
}
#[test]
fn horizontal_drag_clamps_both_ends_and_preserves_fractional_motion() {
    let mut h = handle();
    mouse(&mut h, Phase::Down, 14.0, 14.0);
    // Far beyond the button on both sides: the pointer stays captured and the value clamps (speed 0.5 per px).
    mouse(&mut h, Phase::Move, 300.0, 14.0);
    assert_eq!(h.app().value, 10.0);
    mouse(&mut h, Phase::Move, -300.0, 14.0);
    assert_eq!(h.app().value, -10.0);
    // Measured from the press, not accumulated per event: back to 26.75 px left of it is 4 - 13.375 exactly.
    mouse(&mut h, Phase::Move, 14.0 - 26.75, 14.0);
    assert_eq!(h.app().value, -9.375);
    mouse(&mut h, Phase::Up, 14.0 - 26.75, 14.0);
    assert!(!h.shows(Role::TextBox, "Speed"), "a drag is not a click");
}
/// Escape reaches the field's own reducer (before field events the router swallowed it and a host had to dispatch a
/// cancel): uncommitted text is dropped and nothing is reported.
#[test]
fn escape_cancels_uncommitted_text() {
    let mut h = handle();
    begin(&mut h);
    h.type_text("9");
    key(&mut h, NamedKey::Escape);
    assert_eq!(h.app().value, 4.0);
    assert!(h.app().changes.is_empty());
    assert!(!h.shows(Role::TextBox, "Speed"));
}
/// Leaving the field (a press elsewhere) commits, as egui's `lost_focus` does; the click alone focused it (no host
/// focus request).
#[test]
fn leaving_the_field_commits() {
    let mut h = handle();
    begin(&mut h);
    h.type_text("2.5");
    h.click_at(Point::new(190.0, 70.0));
    assert_eq!(h.app().changes, [Ok(2.5)]);
    assert!(!h.shows(Role::TextBox, "Speed"));
}
