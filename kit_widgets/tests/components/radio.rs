//! Radio: controlled values, disabled options and existing router keyboard navigation.
use crate::support::{Asked, Handle, TestApp};
use kit_core::{Key, NamedKey, Theme};
use kit_state::{Cx, Ident, Intent, Life};
use kit_view::{Availability, Node, Role, Toggle};
use kit_widgets::radio::{Radio, RadioOption, radio};

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
struct Id;
impl Ident<TestApp> for Id {
    fn life(&self, _: &TestApp) -> Life {
        Life::Live
    }
}
const OPTIONS: [RadioOption<'static, u8>; 4] = [
    RadioOption {
        value: 10,
        label: "Sloop",
        availability: Availability::Enabled,
    },
    RadioOption {
        value: 20,
        label: "Brig (locked)",
        availability: Availability::Disabled,
    },
    RadioOption {
        value: 30,
        label: "Frigate",
        availability: Availability::Enabled,
    },
    RadioOption {
        value: 40,
        label: "Galleon",
        availability: Availability::Enabled,
    },
];
fn view(cx: &Cx<'_, TestApp>, theme: &Theme) -> Node<Intent<TestApp>> {
    let selected = match cx.app().asked.last() {
        Some(Asked(value)) => value.parse::<u8>().expect("radio test value"),
        None => 10,
    };
    let choose = |value: &u8| Intent::Ask(Asked(value.to_string()));
    radio(
        cx,
        Radio {
            theme,
            key: Id,
            options: &OPTIONS,
            selected: &selected,
            choose: &choose,
        },
    )
    .pad(theme.spacing.sm)
}
fn key(
    h: &mut Handle<TestApp, impl Fn(&Cx<'_, TestApp>, &Theme) -> Node<Intent<TestApp>>>,
    key: NamedKey,
) {
    h.press_key(Key::Named(key));
}
#[test]
fn golden_idle_and_selected() {
    let mut h = Handle::new(TestApp::new(), (210.0, 150.0), view);
    h.assert_golden("radio_idle");
    h.click(Role::Option, "Frigate");
    h.assert_golden("radio_selected");
}
#[test]
fn arrow_focus_skips_disabled_and_space_selects_the_focused_value() {
    let mut h = Handle::new(TestApp::new(), (210.0, 150.0), view);
    key(&mut h, NamedKey::Space);
    assert!(h.app().asked.is_empty(), "unfocused Space does not select");
    key(&mut h, NamedKey::Tab);
    key(&mut h, NamedKey::ArrowDown);
    key(&mut h, NamedKey::Space);
    assert_eq!(
        h.app().asked,
        [Asked("30".into())],
        "Down skips the disabled Brig"
    );
    assert_eq!(h.toggle_of(Role::Option, "Sloop"), Toggle::Off);
    assert_eq!(h.toggle_of(Role::Option, "Frigate"), Toggle::On);
    key(&mut h, NamedKey::ArrowUp);
    key(&mut h, NamedKey::Space);
    assert_eq!(
        h.app().asked.last(),
        Some(&Asked("10".into())),
        "Up also skips Brig"
    );
    key(&mut h, NamedKey::ArrowUp);
    key(&mut h, NamedKey::Space);
    assert_eq!(
        h.app().asked.last(),
        Some(&Asked("40".into())),
        "Up wraps to the last option"
    );
}
#[test]
fn disabled_click_is_refused_and_click_selects_without_toggling_off() {
    let mut h = Handle::new(TestApp::new(), (210.0, 150.0), view);
    h.click(Role::Option, "Brig (locked)");
    assert!(h.app().asked.is_empty());
    h.click(Role::Option, "Frigate");
    h.click(Role::Option, "Frigate");
    assert_eq!(h.app().asked, [Asked("30".into()), Asked("30".into())]);
    assert_eq!(h.toggle_of(Role::Option, "Frigate"), Toggle::On);
    assert_eq!(h.toggle_of(Role::Option, "Brig (locked)"), Toggle::Off);
}
#[test]
#[should_panic(expected = "radio: selected value must match exactly one option")]
fn invalid_selection_is_rejected() {
    Handle::new(TestApp::new(), (210.0, 150.0), |cx, theme| {
        radio(
            cx,
            Radio {
                theme,
                key: Id,
                options: &OPTIONS,
                selected: &99,
                choose: &|_| Intent::All(vec![]),
            },
        )
    });
}
#[test]
#[should_panic(expected = "radio: selected value must match exactly one option")]
fn duplicate_values_are_rejected() {
    Handle::new(TestApp::new(), (210.0, 150.0), |cx, theme| {
        let options = [
            RadioOption {
                value: 10,
                label: "One",
                availability: Availability::Enabled,
            },
            RadioOption {
                value: 10,
                label: "Two",
                availability: Availability::Enabled,
            },
        ];
        radio(
            cx,
            Radio {
                theme,
                key: Id,
                options: &options,
                selected: &10,
                choose: &|_| Intent::All(vec![]),
            },
        )
    });
}
