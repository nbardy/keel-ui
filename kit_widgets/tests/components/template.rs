//! Template component test: one golden and one behaviour test that catches a real bug. Copy as `<name>.rs`.
use kit_state::{Cx, Ident, Life};
use kit_test::{Handle, TestApp};
use kit_view::{Node, Role, Toggle};
use kit_widgets::template::{Tally, tally};

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
struct Id(&'static str);
impl Ident<TestApp> for Id {
    fn life(&self, _: &TestApp) -> Life {
        Life::Live
    }
}

fn view(cx: &Cx<'_, TestApp>, theme: &kit_core::Theme) -> Node<kit_state::Intent<TestApp>> {
    tally(
        cx,
        Tally {
            theme,
            key: Id("presses"),
            label: "Presses",
            limit: 3,
        },
    )
}

#[test]
fn golden_unpressed_and_pressed() {
    let mut h = Handle::new(TestApp::new(), (160.0, 48.0), view);
    h.assert_golden("template_tally_idle");
    h.click(Role::Button, "Presses");
    h.assert_golden("template_tally_lit");
}

/// Regression guard for the wrap rule (`% limit`): the third press must return to the unlit state, and a typo in a
/// label must fail loudly instead of passing as a no-op.
#[test]
fn presses_light_it_and_the_limit_wraps_it_back() {
    let mut h = Handle::new(TestApp::new(), (160.0, 48.0), view);
    let seen: Vec<Toggle> = (0..3)
        .map(|_| {
            h.click(Role::Button, "Presses");
            h.toggle_of(Role::Button, "Presses")
        })
        .collect();
    assert_eq!(seen, [Toggle::On, Toggle::On, Toggle::Off]);
}

#[test]
#[should_panic(expected = "no Button named \"Pressed\"")]
fn clicking_a_control_the_screen_does_not_show_panics() {
    Handle::new(TestApp::new(), (160.0, 48.0), view).click(Role::Button, "Pressed");
}
