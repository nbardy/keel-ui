//! Modal focus scope, caller-owned dismissal, and visible dialog states.
use kit_core::{Key, NamedKey, Theme, WidgetId};
use kit_state::{Act, Cx, Ident, Intent, Life};
use kit_test::{Asked, Handle, TestApp};
use kit_view::{A11y, Bindings, Node, Role, TextStyle, Toggle, column, interactive, nothing, text};
use kit_widgets::modal::{Action, Modal, modal, primary_action_id};

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
struct Id(&'static str);
impl Ident<TestApp> for Id {
    fn life(&self, _: &TestApp) -> Life {
        Life::Live
    }
}

fn view(cx: &Cx<'_, TestApp>, theme: &Theme) -> Node<Intent<TestApp>> {
    let open = cx.state(
        &Id("open"),
        || true,
        |o: &mut bool, _: &Id, _: (), _: &TestApp| {
            *o = false;
            Act::ask(Asked("caller dismissed".into()))
        },
    );
    let words = |s| {
        text(
            s,
            TextStyle::new(theme.type_scale.body as f32, theme.colors.text),
        )
    };
    let behind = interactive(
        WidgetId::of(&Id("behind")),
        Bindings::activate(Intent::Ask(Asked("behind".into()))),
        A11y::Labeled {
            role: Role::Button,
            label: "Behind".into(),
            toggle: Toggle::NotToggle,
        },
        words("Behind").pad(theme.spacing.md),
    );
    let dialog = match *open {
        true => modal(Modal {
            theme,
            key: Id("dialog"),
            title: "Leave the harbour?",
            body: words("Your crew is ready to sail."),
            primary: Action {
                label: "Sail",
                intent: Intent::Ask(Asked("sail".into())),
            },
            actions: vec![Action {
                label: "Review",
                intent: Intent::Ask(Asked("review".into())),
            }],
            dismiss: open.send(()),
        }),
        false => nothing(),
    };
    column([behind, dialog])
}

fn handle() -> Handle<TestApp, impl Fn(&Cx<'_, TestApp>, &Theme) -> Node<Intent<TestApp>>> {
    Handle::new(TestApp::new(), (420.0, 240.0), view)
}

#[test]
fn golden_open_and_keyboard_focused() {
    let mut h = handle();
    h.assert_golden("modal_open");
    h.press_key(Key::Named(NamedKey::Tab));
    h.assert_golden("modal_focused");
}

/// With a focusable control behind the dialog, wrapping must stay within its three actions.
#[test]
fn tab_cycles_within_dialog_and_escape_emits_callers_dismiss_intent() {
    let mut h = handle();
    let key = |n| Key::Named(n);
    h.press_key(key(NamedKey::Tab));
    h.press_key(key(NamedKey::Enter));
    h.press_key(key(NamedKey::Tab));
    h.press_key(key(NamedKey::Enter));
    h.press_key(key(NamedKey::Tab)); // Close
    h.press_key(key(NamedKey::Tab)); // wraps to Sail, never Behind
    h.press_key(key(NamedKey::Enter));
    h.press_key_with(
        key(NamedKey::Tab),
        kit_input::Modifiers {
            shift: true,
            ..Default::default()
        },
    );
    h.press_key(key(NamedKey::Tab)); // reverse wrap also stays inside
    h.press_key(key(NamedKey::Enter));
    assert_eq!(
        h.app().asked,
        [
            Asked("sail".into()),
            Asked("review".into()),
            Asked("sail".into()),
            Asked("sail".into())
        ]
    );
    h.press_key(key(NamedKey::Escape));
    assert_eq!(
        h.app().asked.last(),
        Some(&Asked("caller dismissed".into()))
    );
    assert!(!h.shows(Role::Button, "Sail"), "caller removed the modal");
}

#[test]
fn close_and_backdrop_send_same_dismissal_without_activating_behind() {
    let mut h = handle();
    h.click(Role::Button, "Close");
    assert_eq!(h.app().asked, [Asked("caller dismissed".into())]);
    let mut h = handle();
    h.click(Role::Button, "Behind");
    assert_eq!(h.app().asked, [Asked("caller dismissed".into())]);
    assert!(!h.shows(Role::Button, "Sail"));
}

/// Local host integration: initial focus is only expressible after layout in today's kit.
#[test]
fn host_initial_focus_on_primary_action_activates_it_without_tab() {
    use kit_core::{Time, Viewport};
    use kit_input::{FocusOutcome, InputEvent, KeyState, Router, UiMemory};
    use kit_layout::TaffyLayout;
    use kit_state::{Store, dispatch};
    use kit_view::{NoWorld, testing::text_system};
    let (mut app, mut store, theme) = (TestApp::new(), Store::new(), kit_test::theme());
    let viewport = Viewport::new(kit_core::kurbo::Size::new(420.0, 240.0), 1.0);
    let laid = TaffyLayout::new()
        .layout(
            view(&Cx::new(&app, &store), &theme),
            viewport,
            &mut text_system(),
            &NoWorld,
        )
        .expect("modal lays out");
    let (router, mut memory) = (Router::default(), UiMemory::new(viewport));
    assert_eq!(
        router.request_focus(
            &laid,
            &mut memory,
            primary_action_id(&Id("dialog")),
            Time::default()
        ),
        FocusOutcome::Focused
    );
    let event = InputEvent::Key {
        key: Key::Named(NamedKey::Enter),
        state: KeyState::Down,
        mods: Default::default(),
    };
    for intent in router
        .route(&laid, &mut memory, event, Time::default())
        .messages()
    {
        dispatch(&mut app, &mut store, intent);
    }
    assert_eq!(app.asked, [Asked("sail".into())]);
}
