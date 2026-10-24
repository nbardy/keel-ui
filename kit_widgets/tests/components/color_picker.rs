//! HSV conversion, typed parse failures, popup/editor behavior and captured pointer picking.
use kit_core::kurbo::Point;
use kit_core::{Key, MotionPref, NamedKey, Theme, Time};
use kit_input::{InputEvent, Modifiers, Phase, PointerEvent, PointerId, PointerKind};
use kit_state::{Cx, Host, Ident, Intent, Life};
use kit_test::Handle;
use kit_view::{Node, Role, Toggle};
use kit_widgets::color_picker::{ColorChanged, ColorPicker, HexError, Hsv};
use kit_widgets::color_picker::{color_picker, hsv_to_rgb, parse_hex, rgb_to_hsv};

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
struct Id;
impl Ident<App> for Id {
    fn life(&self, _: &App) -> Life {
        Life::Live
    }
}
struct App {
    color: Hsv,
    changes: Vec<Result<Hsv, HexError>>,
}
impl Host for App {
    type Ask = ColorChanged<Id>;
    fn now(&self) -> Time {
        Time::default()
    }
    fn motion(&self) -> MotionPref {
        MotionPref::Full
    }
    fn ask(&mut self, ask: Self::Ask) {
        if let Ok(color) = ask.value {
            self.color = color;
        }
        self.changes.push(ask.value);
    }
}
fn app() -> App {
    App {
        color: [0.58, 0.65, 0.8],
        changes: vec![],
    }
}
fn props<'a>(theme: &'a Theme, app: &App) -> ColorPicker<'a, Id> {
    ColorPicker {
        theme,
        key: Id,
        label: "Sail color",
        value: app.color,
    }
}
fn view(cx: &Cx<'_, App>, theme: &Theme) -> Node<Intent<App>> {
    color_picker(cx, props(theme, cx.app())).pad(8.0)
}
fn handle() -> Handle<App, impl Fn(&Cx<'_, App>, &Theme) -> Node<Intent<App>>> {
    Handle::new(app(), (320.0, 410.0), view)
}
#[test]
fn golden_closed_and_open() {
    let mut h = handle();
    h.assert_golden("color_picker_closed");
    h.click(Role::Button, "Sail color");
    h.assert_golden("color_picker_open");
}
#[test]
fn hsv_rgb_round_trip_stays_within_one_thousandth() {
    for r in 0..=10 {
        for g in 0..=10 {
            for b in 0..=10 {
                let rgb = [r, g, b].map(|v| v as f32 / 10.0);
                let actual = hsv_to_rgb(rgb_to_hsv(rgb));
                for (actual, expected) in actual.into_iter().zip(rgb) {
                    assert!((actual - expected).abs() < 1e-3, "{rgb:?} became {actual}");
                }
            }
        }
    }
    for sector in 0..=6 {
        let hsv = [sector as f32 / 6.0, 0.73, 0.91];
        let back = rgb_to_hsv(hsv_to_rgb(hsv));
        let hue_error = (hsv[0] - back[0]).abs();
        assert!(hue_error.min(1.0 - hue_error) < 1e-3);
        assert!((hsv[1] - back[1]).abs() < 1e-3 && (hsv[2] - back[2]).abs() < 1e-3);
    }
    assert_eq!(hsv_to_rgb([1.0, 1.0, 1.0]), [1.0, 0.0, 0.0]);
}
#[test]
fn invalid_hex_is_typed_and_never_changes_the_controlled_color() {
    assert_eq!(parse_hex("#123"), Err(HexError::Length));
    assert_eq!(parse_hex("#gg0000"), Err(HexError::Digit));
    assert_eq!(parse_hex("é1234"), Err(HexError::Digit));
    assert_eq!(parse_hex("+12345"), Err(HexError::Digit));
    let mut h = handle();
    let original = h.app().color;
    h.click(Role::Button, "Sail color");
    h.click(Role::TextBox, "Hex");
    h.press_key_with(
        Key::character('a'),
        Modifiers {
            ctrl: true,
            ..Modifiers::default()
        },
    );
    h.type_text("#gg0000");
    h.press_key(Key::Named(NamedKey::Enter));
    assert_eq!(h.app().changes, [Err(HexError::Digit)]);
    assert_eq!(h.app().color, original);
    h.assert_golden("color_picker_invalid_hex");
    h.press_key_with(
        Key::character('a'),
        Modifiers {
            ctrl: true,
            ..Modifiers::default()
        },
    );
    h.type_text("#12aBeF");
    h.press_key(Key::Named(NamedKey::Enter));
    assert_eq!(h.app().color, parse_hex("12abef").unwrap());
    assert_eq!(h.toggle_of(Role::Button, "Sail color"), Toggle::On);
    // A focused editor consumes the first Escape to leave editing; the second dismisses its popup.
    h.press_key(Key::Named(NamedKey::Escape));
    h.press_key(Key::Named(NamedKey::Escape));
    assert!(!h.shows(Role::TextBox, "Hex"));
}
fn drag(
    h: &mut Handle<App, impl Fn(&Cx<'_, App>, &Theme) -> Node<Intent<App>>>,
    path: &[(f64, f64)],
) {
    let last = path.len() - 1;
    for (i, &(x, y)) in path.iter().enumerate() {
        let phase = match i {
            0 => Phase::Down,
            i if i == last => Phase::Up,
            _ => Phase::Move,
        };
        h.input(InputEvent::Pointer(PointerEvent {
            id: PointerId(1),
            kind: PointerKind::Mouse,
            phase,
            at: Point::new(x, y),
            pressure: 1.0,
            button: kit_input::PointerButton::Primary,
        }));
    }
}
// Popup starts at (8,40): the 288 px square spans (16,48)..(304,336), the hue bar y 342..362 (theme spacing and
// pixel-aligned reference samples).
#[test]
fn drag_beyond_the_square_clamps_edges_and_retains_hue_through_gray() {
    let mut h = handle();
    h.click(Role::Button, "Sail color");
    // Up and left beyond the square: saturation 0 (gray), value 1. Hue is untouched.
    drag(&mut h, &[(100.0, 100.0), (-50.0, -80.0), (-60.0, -90.0)]);
    assert_eq!(h.app().color, [0.58, 0.0, 1.0]);
    // Far right of the hue bar while gray: hue clamps to 1 and saturation/value stay.
    drag(&mut h, &[(30.0, 350.0), (900.0, 500.0), (900.0, 500.0)]);
    assert_eq!(h.app().color, [1.0, 0.0, 1.0]);
}
#[test]
fn dragging_the_hue_bar_preserves_saturation_and_value() {
    let mut h = handle();
    h.click(Role::Button, "Sail color");
    drag(&mut h, &[(30.0, 350.0), (220.0, 350.0), (220.0, 350.0)]);
    assert_eq!(
        h.app().changes.len(),
        3,
        "press, drag and release each pick"
    );
    assert!((h.app().color[0] - (220.0 - 16.0) / 288.0).abs() < 1e-6);
    assert_eq!(&h.app().color[1..], &[0.65, 0.8]);
    h.click_at(Point::new(315.0, 405.0));
    assert_eq!(h.toggle_of(Role::Button, "Sail color"), Toggle::Off);
}
