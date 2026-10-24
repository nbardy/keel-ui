//! Text field and area: goldens (placeholders, then filled with a focused caret and a wrapped area), and the
//! submit/stale-edit rules.
use crate::support::{Asked, Handle, TestApp};
use kit_core::{Key, NamedKey, Theme};
use kit_input::{ClipboardRequest, InputEvent, Modifiers};
use kit_state::{Cx, Ident, Intent, Life};
use kit_view::{Availability, Node, Role, column};
use kit_widgets::{Submitted, TextArea, TextField, text_area, text_field};

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
struct Id(&'static str);
impl Ident<TestApp> for Id {
    fn life(&self, _: &TestApp) -> Life {
        Life::Live
    }
}
impl From<Submitted<Id>> for Asked {
    fn from(s: Submitted<Id>) -> Self {
        Asked(format!("{} submitted {:?}", s.key.0, s.text))
    }
}

fn view(cx: &Cx<'_, TestApp>, theme: &Theme) -> Node<Intent<TestApp>> {
    let name = TextField {
        theme,
        key: Id("name"),
        label: "Ship name",
        placeholder: "Name your ship",
        availability: Availability::Enabled,
    };
    let port = TextField {
        theme,
        key: Id("port"),
        label: "Home port",
        placeholder: "Set at the harbour",
        availability: Availability::Disabled,
    };
    let log = TextArea {
        theme,
        key: Id("log"),
        label: "Log",
        placeholder: "Captain's log",
        availability: Availability::Enabled,
        max_rows: 3,
    };
    column([
        text_field(cx, name),
        text_field(cx, port),
        text_area(cx, log),
    ])
    .gap(8.0)
    .pad(8.0)
}

fn handle() -> Handle<TestApp, impl Fn(&Cx<'_, TestApp>, &Theme) -> Node<Intent<TestApp>>> {
    Handle::new(TestApp::new(), (300.0, 230.0), view)
}

#[test]
fn golden_empty_then_filled() {
    let mut h = handle();
    h.assert_golden("text_field_empty");
    h.click(Role::TextBox, "Log");
    h.type_text("Fair winds out of the harbour. Two sail sighted to the east, holding course for the cape at dusk.");
    h.click(Role::TextBox, "Ship name");
    h.type_text("Kestrel");
    h.assert_golden("text_field_filled");
}

/// Enter submits the text as of the Enter (built in the reducer, not captured when the view was built), and a paste
/// whose ticket names an older revision is refused and counted: applying it would overwrite text typed since the
/// paste was requested.
#[test]
fn enter_submits_the_current_text_and_a_stale_paste_is_refused() {
    let mut h = handle();
    h.click(Role::TextBox, "Ship name");
    h.type_text("ahoy");
    let ctrl = Modifiers {
        ctrl: true,
        ..Modifiers::default()
    };
    h.press_key_with(Key::character('v'), ctrl);
    let ticket = match h.take_clipboard().as_slice() {
        [ClipboardRequest::Read(ticket)] => *ticket,
        other => panic!("Ctrl+V asks the host to read the clipboard once, got {other:?}"),
    };
    h.type_text("!");
    h.input(InputEvent::Paste {
        ticket,
        text: "XX".into(),
    });
    assert_eq!(
        h.store().faults().len(),
        1,
        "the stale paste is refused and counted, not dropped silently"
    );
    h.press_key(Key::Named(NamedKey::Enter));
    assert_eq!(h.app().asked, [Asked("name submitted \"ahoy!\"".into())]);
}
