//! Combo box: goldens closed and open (a long, scrolling list), the choose/dismiss rules, and keyboard walking.
use crate::support::{Handle, TestApp};
use kit_core::kurbo::Point;
use kit_core::{Key, NamedKey, Theme};
use kit_state::{Act, Cx, Ident, Intent, Life};
use kit_view::{Node, Role, Toggle, row};
use kit_widgets::{Check, Checkbox, ComboBox, Press, checkbox, combo_box};

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
struct Id(&'static str);
impl Ident<TestApp> for Id {
    fn life(&self, _: &TestApp) -> Life {
        Life::Live
    }
}

const SHIPS: [&str; 12] = [
    "Sloop",
    "Cutter",
    "Brig",
    "Schooner",
    "Frigate",
    "Galleon",
    "Junk",
    "Dhow",
    "Xebec",
    "Caravel",
    "Carrack",
    "Man-o-war",
];

#[derive(Clone, Debug, PartialEq)]
enum Ev {
    Pick(usize),
    Flip,
}

/// The caller owns the chosen ship and a checkbox placed beside the combo (something a stray press could hit).
fn view(cx: &Cx<'_, TestApp>, theme: &Theme) -> Node<Intent<TestApp>> {
    let model = cx.state(
        &Id("model"),
        || (0_usize, false),
        |m: &mut (usize, bool), _: &Id, ev: Ev, _: &TestApp| {
            match ev {
                Ev::Pick(i) => m.0 = i,
                Ev::Flip => m.1 = !m.1,
            }
            Act::none()
        },
    );
    let choose = |i| model.send(Ev::Pick(i));
    let combo = ComboBox {
        theme,
        key: Id("class"),
        label: "Ship class",
        options: &SHIPS,
        selected: model.0,
        choose: &choose,
    };
    let check = if model.1 { Check::On } else { Check::Off };
    let beside = Checkbox {
        theme,
        key: Id("Behind"),
        label: "Behind",
        check,
        press: Press::Sends(model.send(Ev::Flip)),
    };
    row([combo_box(cx, combo), checkbox(cx, beside)])
        .gap(16.0)
        .pad(8.0)
}

fn handle() -> Handle<TestApp, impl Fn(&Cx<'_, TestApp>, &Theme) -> Node<Intent<TestApp>>> {
    Handle::new(TestApp::new(), (260.0, 270.0), view)
}

#[test]
fn golden_closed_and_open() {
    let mut h = handle();
    h.assert_golden("combo_box_closed");
    h.click(Role::ComboBox, "Ship class");
    h.assert_golden("combo_box_open");
}

/// Choosing must both set the caller's value and close the list (the intent is `All[choose, close]`; dropping the
/// close leaves a stale popup open). A press outside must close it WITHOUT choosing and without reaching the control
/// it landed on (click-outside is consumed): otherwise dismissing a popup toggles whatever is behind it.
#[test]
fn choosing_closes_and_a_press_outside_neither_chooses_nor_reaches_behind() {
    let mut h = handle();
    h.click(Role::ComboBox, "Ship class");
    assert_eq!(h.toggle_of(Role::ComboBox, "Ship class"), Toggle::On);
    h.click(Role::Option, "Brig");
    assert!(!h.shows(Role::Option, "Brig"), "choosing closes the list");
    assert_eq!(h.toggle_of(Role::ComboBox, "Ship class"), Toggle::Off);

    h.click(Role::ComboBox, "Ship class");
    assert_eq!(
        h.toggle_of(Role::Option, "Brig"),
        Toggle::On,
        "the list marks the chosen option"
    );
    h.click(Role::CheckBox, "Behind");
    assert!(
        !h.shows(Role::Option, "Brig"),
        "a press outside closes the list"
    );
    assert_eq!(
        h.toggle_of(Role::CheckBox, "Behind"),
        Toggle::Off,
        "the dismissing press never reaches behind"
    );
    h.click_at(Point::new(250.0, 260.0));
    h.click(Role::ComboBox, "Ship class");
    assert_eq!(
        h.toggle_of(Role::Option, "Brig"),
        Toggle::On,
        "nothing else was chosen"
    );
}

/// Keyboard: Tab focuses the combo, Enter opens, Down/Up walk the options (router focus), Enter chooses; Escape
/// closes without choosing. Guards the router's arrow-key focus step.
#[test]
fn arrows_walk_the_list_enter_chooses_escape_cancels() {
    let mut h = handle();
    let key = |n| Key::Named(n);
    h.press_key(key(NamedKey::Tab));
    h.press_key(key(NamedKey::Enter));
    assert!(
        h.shows(Role::Option, "Cutter"),
        "Enter on the focused combo opens it"
    );
    for n in [
        NamedKey::ArrowDown,
        NamedKey::ArrowDown,
        NamedKey::ArrowDown,
        NamedKey::ArrowUp,
    ] {
        h.press_key(key(n));
    }
    h.press_key(key(NamedKey::Enter));
    assert!(!h.shows(Role::Option, "Cutter"));
    h.click(Role::ComboBox, "Ship class");
    assert_eq!(
        h.toggle_of(Role::Option, "Cutter"),
        Toggle::On,
        "Down, Down, Down, Up then Enter chose the 2nd"
    );
    h.press_key(key(NamedKey::Escape));
    assert!(!h.shows(Role::Option, "Cutter"), "Escape closes");
}
