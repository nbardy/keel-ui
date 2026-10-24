//! A text field end to end: real keys, pointer, IME and clipboard events through the real router, the real
//! `EditorState` reducer and real shaped text, re-laid-out after every event like a frame loop does.
use kit_core::kurbo::{Point, Rect, Size};
use kit_core::peniko::Color;
use kit_core::{Time, Viewport, WidgetId};
use kit_input::*;
use kit_view::testing::laid_out;
use kit_view::*;
use std::time::Duration;

fn id() -> WidgetId {
    WidgetId::key("field")
}

struct Rig {
    state: EditorState,
    mem: UiMemory,
    router: Router,
    now: Time,
    masked: bool,
    /// Every message that reached the app, in order.
    seen: Vec<Edit>,
}

impl Rig {
    fn new(state: EditorState, platform: Platform) -> Self {
        let mut router = Router::default();
        router.config.platform = platform;
        Self {
            state,
            mem: UiMemory::new(Viewport::new(Size::new(400.0, 300.0), 1.0)),
            router,
            now: Time::default(),
            masked: false,
            seen: Vec::new(),
        }
    }
    fn single(text: &str) -> Self {
        Self::new(EditorState::single_line(text), Platform::Other)
    }
    fn area(text: &str) -> Self {
        Self::new(EditorState::multi_line(text), Platform::Other)
    }
    fn laid(&self) -> LaidOut<Edit> {
        let mut field = text_field(
            id(),
            "Name",
            &self.state,
            TextStyle::new(16.0, Color::BLACK),
            |edit| edit,
        );
        if let Node::TextField { field, .. } = &mut field {
            field.masking = match self.masked {
                true => Masking::Bullets,
                false => Masking::Plain,
            };
        }
        let height = match self.state.mode() {
            Mode::SingleLine => 30.0,
            Mode::MultiLine => 120.0,
        };
        laid_out(
            column(vec![field]),
            &[
                Rect::new(0.0, 0.0, 400.0, 300.0),
                Rect::new(0.0, 0.0, 200.0, height),
            ],
        )
    }
    fn send(&mut self, event: InputEvent) -> Routing<Edit> {
        self.now = Time(self.now.0 + Duration::from_millis(100));
        let laid = self.laid();
        let routing = self.router.route(&laid, &mut self.mem, event, self.now);
        for edit in routing.clone().messages() {
            self.seen.push(edit.clone());
            self.state.apply(edit);
        }
        routing
    }
    /// Screen position of the caret at value offset `at`.
    fn x_of(&self, at: usize) -> f64 {
        match &self.laid().find(id()).expect("field").kind {
            LaidKind::TextField { geometry, .. } => {
                let caret = geometry.caret_at(Caret {
                    offset: at,
                    affinity: Affinity::After,
                });
                caret.x0 + 4.0
            }
            _ => unreachable!("the field is a text field"),
        }
    }
    fn mouse(&mut self, phase: Phase, x: f64, y: f64) {
        self.send(InputEvent::Pointer(PointerEvent {
            id: PointerId(0),
            kind: PointerKind::Mouse,
            phase,
            at: Point::new(x, y),
            pressure: 1.0,
            button: kit_input::PointerButton::Primary,
        }));
    }
    fn click(&mut self, x: f64, y: f64) {
        self.mouse(Phase::Down, x, y);
        self.mouse(Phase::Up, x, y);
    }
    fn key(&mut self, key: Key, mods: Modifiers) -> Routing<Edit> {
        self.send(InputEvent::Key {
            key,
            state: KeyState::Down,
            mods,
        })
    }
    fn named(&mut self, key: NamedKey) {
        self.key(Key::Named(key), Modifiers::default());
    }
    fn chord(&mut self, c: char, mods: Modifiers) {
        self.key(Key::character(c), mods);
    }
    fn typed(&mut self, text: &str) {
        self.send(InputEvent::Text(text.into()));
    }
}

const CTRL: Modifiers = Modifiers {
    shift: false,
    ctrl: true,
    alt: false,
    meta: false,
};
const SHIFT: Modifiers = Modifiers {
    shift: true,
    ctrl: false,
    alt: false,
    meta: false,
};

#[test]
fn a_click_places_the_caret_between_glyphs_and_a_drag_selects() {
    let mut rig = Rig::single("hello world");
    let y = 10.0;
    rig.click(rig.x_of(5), y);
    assert_eq!(rig.state.value().selection(), Selection::caret(5));
    rig.mouse(Phase::Down, rig.x_of(2), y);
    rig.mouse(Phase::Move, rig.x_of(7), y);
    rig.mouse(Phase::Up, rig.x_of(7), y);
    assert_eq!(rig.state.value().selected(), "llo w");
    // Dragging back past the start grows the other way.
    rig.mouse(Phase::Down, rig.x_of(6), y);
    rig.mouse(Phase::Move, rig.x_of(1), y);
    assert_eq!(rig.state.value().selected(), "ello ");
}

#[test]
fn double_click_selects_a_word_and_triple_click_a_line() {
    let mut rig = Rig::area("one two\nthree four");
    let (x, y) = (rig.x_of(10), 10.0 + 20.0 + 4.0);
    rig.click(x, y);
    rig.click(x, y);
    assert_eq!(rig.state.value().selected(), "three");
    rig.click(x, y);
    assert_eq!(
        rig.state.value().selected(),
        "three four",
        "the whole line, not the break"
    );
    rig.click(x, y);
    assert_eq!(
        rig.state.value().selection().range().len(),
        0,
        "a fourth press starts a new run: a caret"
    );
}

#[test]
fn a_second_press_after_the_double_click_interval_is_a_new_click() {
    let mut rig = Rig::single("one two");
    let (x, y) = (rig.x_of(5), 10.0);
    rig.click(x, y);
    rig.now = Time(rig.now.0 + Duration::from_secs(2));
    rig.click(x, y);
    assert!(rig.state.value().selected().is_empty());
}

#[test]
fn typing_editing_and_undo_work_through_the_keyboard() {
    let mut rig = Rig::single("");
    rig.click(10.0, 10.0);
    rig.typed("hello world");
    rig.key(Key::Named(NamedKey::ArrowLeft), CTRL);
    rig.key(Key::Named(NamedKey::ArrowRight), SHIFT);
    assert_eq!(rig.state.value().selected(), "w");
    rig.typed("W");
    assert_eq!(
        rig.state.text(),
        "hello World",
        "shift-extend then type replaces the selection"
    );
    rig.key(Key::Named(NamedKey::Backspace), CTRL);
    assert_eq!(
        rig.state.text(),
        "hello orld",
        "Ctrl+Backspace takes the word before the caret"
    );
    rig.chord('z', CTRL);
    assert_eq!(rig.state.text(), "hello World");
    rig.key(
        Key::character('z'),
        Modifiers {
            shift: true,
            ..CTRL
        },
    );
    assert_eq!(rig.state.text(), "hello orld");
    rig.chord('a', CTRL);
    assert_eq!(rig.state.value().selected(), "hello orld");
}

#[test]
fn enter_submits_a_line_and_breaks_an_area() {
    let mut line = Rig::single("x");
    line.click(10.0, 10.0);
    line.named(NamedKey::Enter);
    assert_eq!(line.seen.last(), Some(&Edit::Submit));
    assert_eq!(line.state.text(), "x");
    let mut area = Rig::area("x");
    area.click(10.0, 10.0);
    area.named(NamedKey::Enter);
    assert_eq!(area.state.text(), "x\n");
}

#[test]
fn up_from_the_first_line_goes_to_the_start_and_down_from_the_last_to_the_end() {
    let mut rig = Rig::area("abcdef\nxy");
    rig.click(rig.x_of(3), 10.0);
    rig.named(NamedKey::ArrowUp);
    assert_eq!(rig.state.value().selection().head, 0);
    rig.named(NamedKey::ArrowDown);
    rig.named(NamedKey::ArrowDown);
    assert_eq!(rig.state.value().selection().head, 9);
}

#[test]
fn up_and_down_keep_the_column_across_a_short_line() {
    let mut rig = Rig::area("abcdef\nxy\nabcdef");
    rig.click(rig.x_of(4), 10.0);
    assert_eq!(rig.state.value().selection().head, 4);
    rig.named(NamedKey::ArrowDown);
    assert_eq!(
        rig.state.value().selection().head,
        9,
        "the short line's end"
    );
    rig.named(NamedKey::ArrowDown);
    assert_eq!(
        rig.state.value().selection().head,
        14,
        "back at column 4 on the long line"
    );
    rig.named(NamedKey::ArrowUp);
    rig.named(NamedKey::ArrowUp);
    assert_eq!(rig.state.value().selection().head, 4);
}

#[test]
fn apple_uses_option_for_words_and_command_for_lines() {
    let mut rig = Rig::new(EditorState::single_line("one two"), Platform::Apple);
    rig.click(rig.x_of(7), 10.0);
    rig.key(
        Key::Named(NamedKey::ArrowLeft),
        Modifiers {
            alt: true,
            ..Modifiers::default()
        },
    );
    assert_eq!(rig.state.value().selection().head, 4);
    rig.key(
        Key::Named(NamedKey::ArrowLeft),
        Modifiers {
            meta: true,
            ..Modifiers::default()
        },
    );
    assert_eq!(rig.state.value().selection().head, 0);
    rig.key(
        Key::character('a'),
        Modifiers {
            meta: true,
            ..Modifiers::default()
        },
    );
    assert_eq!(rig.state.value().selected(), "one two");
}

struct FakeClipboard {
    held: String,
    answers_later: bool,
}
impl Clipboard for FakeClipboard {
    fn write(&mut self, text: &str) -> Result<(), ClipboardError> {
        self.held = text.to_owned();
        Ok(())
    }
    fn read(&mut self, _: PasteTicket) -> Result<ClipboardRead, ClipboardError> {
        match self.answers_later {
            true => Ok(ClipboardRead::Later),
            false => Ok(ClipboardRead::Ready(self.held.clone())),
        }
    }
}

#[test]
fn copy_cut_and_paste_go_through_the_clipboard_outbox() {
    let mut rig = Rig::single("hello world");
    let mut clipboard = FakeClipboard {
        held: String::new(),
        answers_later: false,
    };
    rig.click(10.0, 10.0);
    rig.chord('a', CTRL);
    rig.chord('c', CTRL);
    let served = service(rig.mem.take_clipboard(), &mut clipboard);
    assert!(served.failed.is_empty());
    assert_eq!(clipboard.held, "hello world");
    assert_eq!(rig.state.text(), "hello world", "copy changes nothing");

    rig.chord('x', CTRL);
    service(rig.mem.take_clipboard(), &mut clipboard);
    assert_eq!(rig.state.text(), "");

    rig.chord('v', CTRL);
    for event in service(rig.mem.take_clipboard(), &mut clipboard).events {
        rig.send(event);
    }
    assert_eq!(rig.state.text(), "hello world");
    rig.chord('z', CTRL);
    assert_eq!(rig.state.text(), "", "undo after paste restores the text");
}

/// A browser answers the read later. Text typed in between makes the answer stale: it is refused, not inserted at
/// offsets that now mean something else.
#[test]
fn a_late_paste_for_changed_text_is_refused() {
    let mut rig = Rig::single("ab");
    let mut clipboard = FakeClipboard {
        held: "PASTED".into(),
        answers_later: true,
    };
    rig.click(190.0, 10.0);
    rig.chord('v', CTRL);
    let requests = rig.mem.take_clipboard();
    let ClipboardRequest::Read(ticket) = requests[0].clone() else {
        panic!("a paste asks to read");
    };
    assert!(
        service(requests, &mut clipboard).events.is_empty(),
        "the answer is not ready"
    );
    rig.typed("!");
    rig.send(InputEvent::Paste {
        ticket,
        text: "PASTED".into(),
    });
    assert_eq!(rig.state.text(), "ab!");
    // Asked again with the current text, it lands.
    rig.chord('v', CTRL);
    let ClipboardRequest::Read(fresh) = rig.mem.take_clipboard()[0].clone() else {
        panic!("a paste asks to read");
    };
    rig.send(InputEvent::Paste {
        ticket: fresh,
        text: "PASTED".into(),
    });
    assert_eq!(rig.state.text(), "ab!PASTED");
}

#[test]
fn a_password_field_never_copies() {
    let mut rig = Rig::single("secret");
    rig.masked = true;
    rig.click(10.0, 10.0);
    rig.chord('a', CTRL);
    rig.chord('c', CTRL);
    rig.chord('x', CTRL);
    assert!(rig.mem.take_clipboard().is_empty());
}

#[test]
fn a_composition_stays_out_of_the_text_until_the_input_method_commits() {
    let mut rig = Rig::single("hi ");
    rig.click(100.0, 10.0);
    let rev = rig.state.value().rev();
    rig.send(InputEvent::Ime(ImeEvent::Preedit("ni".into())));
    assert_eq!(rig.state.composition(), &Composition::Preedit("ni".into()));
    assert_eq!((rig.state.text(), rig.state.value().rev()), ("hi ", rev));
    let laid = rig.laid();
    let LaidKind::TextField { geometry, .. } = &laid.find(id()).expect("field").kind else {
        unreachable!("the field is a text field");
    };
    assert_eq!(geometry.composition_rects().len(), 1, "underlined inline");
    rig.send(InputEvent::Ime(ImeEvent::Commit("\u{4f60}".into())));
    assert_eq!(rig.state.text(), "hi \u{4f60}");
    assert_eq!(rig.state.composition(), &Composition::Idle);
}

#[test]
fn the_host_is_told_where_text_entry_is() {
    let mut rig = Rig::single("abc");
    assert_eq!(text_target(&rig.laid(), &rig.mem), TextTarget::Unfocused);
    rig.click(10.0, 10.0);
    let TextTarget::Field {
        caret,
        mode,
        secret,
    } = text_target(&rig.laid(), &rig.mem)
    else {
        panic!("a focused field is a target");
    };
    assert_eq!((mode, secret), (Mode::SingleLine, false));
    assert!(
        Rect::new(0.0, 0.0, 200.0, 30.0).contains(caret.center()),
        "inside the field: {caret:?}"
    );
}
