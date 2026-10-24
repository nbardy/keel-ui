//! Checkbox: golden of the three checks plus a disabled one, and the focus rule for Space.
use crate::support::{Handle, TestApp};
use kit_core::{Key, NamedKey, Theme};
use kit_state::{Act, Cx, Ident, Intent, Life};
use kit_view::{Node, Role, Toggle, column};
use kit_widgets::{Check, Checkbox, Press, checkbox};

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
struct Id(&'static str);
impl Ident<TestApp> for Id {
    fn life(&self, _: &TestApp) -> Life {
        Life::Live
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Flip(usize);

/// The caller owns the values (two flags in its own keyed state); each checkbox gets the intent that flips one.
fn view(cx: &Cx<'_, TestApp>, theme: &Theme) -> Node<Intent<TestApp>> {
    let flags = cx.state(
        &Id("prefs"),
        || [false, false],
        |f: &mut [bool; 2], _: &Id, Flip(i), _: &TestApp| {
            f[i] = !f[i];
            Act::none()
        },
    );
    let check = |on: bool| if on { Check::On } else { Check::Off };
    column([
        checkbox(
            cx,
            Checkbox {
                theme,
                key: Id("Sound"),
                label: "Sound",
                check: check(flags[0]),
                press: Press::Sends(flags.send(Flip(0))),
            },
        ),
        checkbox(
            cx,
            Checkbox {
                theme,
                key: Id("Music"),
                label: "Music",
                check: check(flags[1]),
                press: Press::Sends(flags.send(Flip(1))),
            },
        ),
        checkbox(
            cx,
            Checkbox {
                theme,
                key: Id("All crew"),
                label: "All crew",
                check: Check::Mixed,
                press: Press::Sends(flags.send(Flip(0))),
            },
        ),
        checkbox(
            cx,
            Checkbox {
                theme,
                key: Id("Locked"),
                label: "Locked",
                check: Check::On,
                press: Press::Disabled,
            },
        ),
    ])
    .pad(8.0)
    .gap(4.0)
}

fn toggles(
    h: &Handle<TestApp, impl Fn(&Cx<'_, TestApp>, &Theme) -> Node<Intent<TestApp>>>,
) -> [Toggle; 2] {
    [
        h.toggle_of(Role::CheckBox, "Sound"),
        h.toggle_of(Role::CheckBox, "Music"),
    ]
}

#[test]
fn golden_unchecked_checked_mixed_and_disabled() {
    let mut h = Handle::new(TestApp::new(), (160.0, 120.0), view);
    h.click(Role::CheckBox, "Music");
    h.assert_golden("checkbox_states");
}

/// Space must flip only the checkbox that has keyboard focus. A checkbox that bound Space as a hotkey (instead of
/// relying on focus) would flip with nothing focused, and every checkbox on the page would flip together.
#[test]
fn space_toggles_only_the_focused_checkbox() {
    let mut h = Handle::new(TestApp::new(), (160.0, 120.0), view);
    let space = Key::Named(NamedKey::Space);
    h.press_key(space);
    assert_eq!(
        toggles(&h),
        [Toggle::Off, Toggle::Off],
        "Space with nothing focused flips nothing"
    );
    h.press_key(Key::Named(NamedKey::Tab));
    h.press_key(Key::Named(NamedKey::Tab));
    h.press_key(space);
    assert_eq!(
        toggles(&h),
        [Toggle::Off, Toggle::On],
        "only the focused one (Music) flips"
    );
    // Tab skips the disabled one: from "All crew" the next stop wraps to "Sound".
    h.press_key(Key::Named(NamedKey::Tab));
    h.press_key(Key::Named(NamedKey::Tab));
    h.press_key(space);
    assert_eq!(
        toggles(&h),
        [Toggle::On, Toggle::On],
        "the disabled checkbox never takes focus"
    );
}
