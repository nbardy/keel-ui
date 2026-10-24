//! Web text entry, the pure half: what a hidden `<textarea>` reports, as the kit's input vocabulary.
//!
//! A GPU canvas has no text-entry machinery. While a kit text field has focus the host parks a textarea at the caret and
//! focuses it, so the platform's keyboard, IME candidate window and clipboard all work; what happens in it comes out
//! of the DOM as keydown, input, composition and paste events. [`Composer`] diffs those into [`Entry`] values and
//! [`entry_events`] makes kit events of them. No DOM types here: the wasm half (`textarea.rs`) only reads the events.
//!
//! The diffing is adapted from the Keel game's browser host, which adapted it in part from eframe 0.36.2
//! `crates/eframe/src/web/text_agent.rs` (<https://github.com/emilk/egui>, MIT OR Apache-2.0, (c) Emil Ernerfeldt,
//! Rerun Technologies; see `THIRD_PARTY.md`). Kept from eframe: the element parked at the caret, the
//! `input`/`compositionend` diffing against the last text seen, and the keyCode-229 and keyCode-0 workarounds for Android
//! Gboard and iOS Korean. Changed: egui events became kit `Entry`s, a `<textarea>` replaced the `<input>`, the element
//! is emptied after every commit so it never accumulates text, and Android's surrounding-text deletion events are not
//! ported.
use crate::convert::key_from_dom;
use kit_input::{
    ClipboardRead, ImeEvent, InputEvent, Key, KeyState, Modifiers, NamedKey, PasteTicket,
};

/// What the hidden element reported.
#[derive(Clone, PartialEq, Debug)]
pub enum Entry {
    Key {
        key: Key,
        mods: Modifiers,
    },
    Text(String),
    Ime(ImeEvent),
    /// Text the browser pasted into the element (a shortcut, or the touch menu's Paste).
    Paste(String),
}

/// What one DOM event made of the element: entries to route, and whether to empty the element afterwards.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Reported {
    pub entries: Vec<Entry>,
    pub empty_element: bool,
}

/// The diffing state between DOM events.
#[derive(Clone, Debug, Default)]
pub struct Composer {
    /// The committed part of the element's text that the current composition sits after.
    committed: String,
    /// The last keydown was an input method's (keyCode 229 on Android, 0 on iOS Korean): a `deleteContentBackward`
    /// that follows is a Backspace the keydown could not name.
    unnamed_key: bool,
}

/// What to do with a keydown.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum Keydown {
    /// The input method owns it.
    Consumed,
    Forward,
}

impl Composer {
    /// A `keydown` on the element. `composing` is `isComposing`; `key_code` the legacy `keyCode`, the only place the
    /// "an input method handled this" signal is reliable on Android and iOS.
    pub fn keydown(&mut self, composing: bool, key_code: u32) -> Keydown {
        self.unnamed_key = matches!(key_code, 229 | 0);
        match composing || self.unnamed_key {
            true => Keydown::Consumed,
            false => Keydown::Forward,
        }
    }

    pub fn keyup(&mut self) {
        self.unnamed_key = false;
    }

    /// An `input` event: `value` is the element's text, `composing` its `isComposing`, `kind` its `inputType`.
    pub fn input(&mut self, value: &str, composing: bool, kind: &str) -> Reported {
        let typed = matches!(kind, "insertText" | "insertReplacementText");
        match (composing || typed, kind, self.unnamed_key) {
            (false, "deleteContentBackward", true) => Reported {
                entries: vec![Entry::Key {
                    key: Key::Named(NamedKey::Backspace),
                    mods: Modifiers::default(),
                }],
                empty_element: true,
            },
            // Deletions and moves arrive as keydowns; nothing here is text.
            (false, _, _) => self.emptied(),
            (true, _, _) => {
                let shared = common_prefix(value, &self.committed);
                let tail: String = value.chars().skip(shared).collect();
                match composing {
                    true => {
                        self.committed = value.chars().take(shared).collect();
                        Reported {
                            entries: vec![Entry::Ime(ImeEvent::Preedit(tail))],
                            empty_element: false,
                        }
                    }
                    false => {
                        self.committed.clear();
                        Reported {
                            entries: vec![Entry::Text(tail)],
                            empty_element: true,
                        }
                    }
                }
            }
        }
    }

    /// `compositionend`: whatever the element holds beyond the committed part is the commit.
    pub fn composition_end(&mut self, value: &str) -> Reported {
        let commit: String = value.chars().skip(self.committed.chars().count()).collect();
        self.committed.clear();
        Reported {
            entries: vec![Entry::Ime(ImeEvent::Commit(commit))],
            empty_element: true,
        }
    }

    fn emptied(&mut self) -> Reported {
        self.committed.clear();
        Reported {
            entries: Vec::new(),
            empty_element: true,
        }
    }
}

fn common_prefix(a: &str, b: &str) -> usize {
    a.chars().zip(b.chars()).take_while(|(a, b)| a == b).count()
}

/// The kit key for a `keydown` on the element that should reach the router, or `None` when the text will arrive as an
/// `input` event instead: a plain printable key (Shift allowed) or Space. Chords stay keys: Ctrl+C is a command, not text.
pub fn key_for(dom_key: &str, mods: Modifiers) -> Option<Key> {
    let key = key_from_dom(dom_key)?;
    let command = mods.ctrl || mods.meta || mods.alt;
    match (key, command) {
        (Key::Character(_) | Key::Named(NamedKey::Space), false) => None,
        (Key::Character(_) | Key::Named(_), _) => Some(key),
    }
}

/// Whether `key` with `mods` is the paste shortcut (Ctrl+V, or Cmd+V on Apple). The browser answers it with a `paste`
/// event on the element, which is the only way to read the clipboard without a permission prompt.
pub fn is_paste_chord(key: Key, mods: Modifiers) -> bool {
    (mods.ctrl || mods.meta) && key == Key::Character('v')
}

/// Where a paste is. The key and the text of one paste reach the host separately and in a fixed order that the kit does
/// not share: the browser delivers the `paste` event (with the text) right after the keydown, but the kit only routes
/// the keydown at the next frame, and only then asks for the text (`ClipboardRequest::Read`, under a ticket naming the
/// field and its text revision at that moment). Answering with the ticket of the *last frame's* focused field would be
/// stale whenever an earlier event in the same batch edited the field (select all, Backspace, paste), and the field
/// refuses a stale paste. So the text waits for the router's ticket.
#[derive(Clone, PartialEq, Debug, Default)]
pub enum Paste {
    #[default]
    Idle,
    /// The paste shortcut was pressed; its `paste` event has not arrived.
    Chord,
    /// The text arrived; the router has not asked for it yet.
    Text(String),
    /// The router asked, under this ticket; the text has not arrived yet.
    Asked(PasteTicket),
}

impl Paste {
    /// A key entry reached the host: a paste shortcut starts a new paste, and drops text a previous shortcut left
    /// unclaimed (the router may refuse a paste, for instance into a read-only field).
    fn key(&mut self, key: Key, mods: Modifiers) {
        if is_paste_chord(key, mods) {
            *self = Paste::Chord;
        }
    }

    /// The `paste` event's text. `unasked` is the focused field's ticket as of the last frame, for text the browser
    /// pasted with no shortcut (a touch menu's Paste): no field, nothing to paste into, nothing sent.
    fn text(&mut self, text: String, unasked: Option<PasteTicket>) -> Vec<InputEvent> {
        match std::mem::take(self) {
            Paste::Asked(ticket) => vec![InputEvent::Paste { ticket, text }],
            Paste::Chord | Paste::Text(_) => {
                *self = Paste::Text(text);
                Vec::new()
            }
            Paste::Idle => unasked
                .map(|ticket| InputEvent::Paste { ticket, text })
                .into_iter()
                .collect(),
        }
    }

    /// The router asked for the text under `ticket` (`Clipboard::read`): the text if it is already here, else the
    /// ticket waits for it.
    pub fn read(&mut self, ticket: PasteTicket) -> ClipboardRead {
        match std::mem::take(self) {
            Paste::Text(text) => ClipboardRead::Ready(text),
            Paste::Idle | Paste::Chord | Paste::Asked(_) => {
                *self = Paste::Asked(ticket);
                ClipboardRead::Later
            }
        }
    }
}

/// The kit events for one entry, updating the paste state. Empty text types nothing.
pub fn entry_events(
    entry: Entry,
    paste: &mut Paste,
    unasked: Option<PasteTicket>,
) -> Vec<InputEvent> {
    match entry {
        Entry::Key { key, mods } => {
            paste.key(key, mods);
            vec![InputEvent::Key {
                key,
                state: KeyState::Down,
                mods,
            }]
        }
        Entry::Text(text) if text.is_empty() => Vec::new(),
        Entry::Text(text) => vec![InputEvent::Text(text)],
        Entry::Ime(ime) => vec![InputEvent::Ime(ime)],
        Entry::Paste(text) => paste.text(text, unasked),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kit_core::WidgetId;
    use kit_state::TextValue;

    fn ticket(field: &str) -> PasteTicket {
        PasteTicket {
            field: WidgetId::key(field),
            rev: TextValue::new("").rev(),
        }
    }

    /// Typing is text, and the element is emptied after each commit: the next character is again the whole value.
    /// Without the empty, "a" then "b" would report "a" then "ab" tails of a growing string.
    #[test]
    fn typing_is_text_and_empties_the_element_so_it_never_accumulates() {
        let mut composer = Composer::default();
        let reported = composer.input("a", false, "insertText");
        assert_eq!(reported.entries, [Entry::Text("a".into())]);
        assert!(reported.empty_element);
        assert_eq!(
            composer.input("b", false, "insertText").entries,
            [Entry::Text("b".into())]
        );
    }

    /// Pinyin: "n", "ni" are preedits shown inline, never inserted; the commit arrives with `compositionend`. Emptying
    /// the element mid-composition would abort it.
    #[test]
    fn a_composition_is_preedit_until_compositionend_commits_it() {
        let mut composer = Composer::default();
        let first = composer.input("n", true, "insertCompositionText");
        assert_eq!(first.entries, [Entry::Ime(ImeEvent::Preedit("n".into()))]);
        assert!(
            !first.empty_element,
            "emptying mid-composition would abort it"
        );
        let second = composer.input("ni", true, "insertCompositionText");
        assert_eq!(second.entries, [Entry::Ime(ImeEvent::Preedit("ni".into()))]);
        let end = composer.composition_end("\u{4f60}");
        assert_eq!(
            end.entries,
            [Entry::Ime(ImeEvent::Commit("\u{4f60}".into()))]
        );
        assert!(end.empty_element);
    }

    /// Text typed before a composition starts stays outside it: only what follows the committed prefix is the preedit,
    /// and the commit is what the element holds beyond that prefix.
    #[test]
    fn a_composition_after_committed_text_reports_only_its_own_tail() {
        let mut composer = Composer::default();
        composer.input("ab", true, "insertCompositionText");
        // The user's input method committed "ab" and started "c": the element holds "abc".
        composer.committed = "ab".into();
        let preedit = composer.input("abc", true, "insertCompositionText");
        assert_eq!(preedit.entries, [Entry::Ime(ImeEvent::Preedit("c".into()))]);
        let end = composer.composition_end("abcd");
        assert_eq!(end.entries, [Entry::Ime(ImeEvent::Commit("cd".into()))]);
    }

    /// Android Gboard with suggestions showing sends keyCode 229 for Backspace; only the input event says what it was.
    /// Once the key is released a later deletion event is not a Backspace.
    #[test]
    fn a_backspace_the_keydown_could_not_name_is_recovered_from_the_input_event() {
        let mut composer = Composer::default();
        assert_eq!(composer.keydown(false, 229), Keydown::Consumed);
        let reported = composer.input("", false, "deleteContentBackward");
        assert_eq!(
            reported.entries,
            [Entry::Key {
                key: Key::Named(NamedKey::Backspace),
                mods: Modifiers::default()
            }]
        );
        composer.keyup();
        assert!(
            composer
                .input("", false, "deleteContentBackward")
                .entries
                .is_empty()
        );
        assert_eq!(
            composer.keydown(false, 0),
            Keydown::Consumed,
            "iOS Korean sends keyCode 0 for the same"
        );
        composer.keyup();
        assert_eq!(composer.keydown(false, 8), Keydown::Forward);
        assert_eq!(
            composer.keydown(true, 65),
            Keydown::Consumed,
            "keys during composition are the IME's"
        );
    }

    /// Printable keys wait for their text (the `input` event); chords and named keys are commands and go as keys. If a
    /// plain `a` also went as a key, the field would see "a" twice.
    #[test]
    fn printable_keys_wait_for_their_text_but_chords_are_commands() {
        let none = Modifiers::default();
        let ctrl = Modifiers { ctrl: true, ..none };
        let shift = Modifiers {
            shift: true,
            ..none
        };
        assert_eq!(key_for("a", none), None);
        assert_eq!(key_for("A", shift), None);
        assert_eq!(key_for(" ", none), None);
        assert_eq!(key_for("a", ctrl), Some(Key::Character('a')));
        assert_eq!(
            key_for("Backspace", none),
            Some(Key::Named(NamedKey::Backspace))
        );
        assert_eq!(key_for("F5", none), None);
    }

    fn chord() -> Entry {
        let ctrl = Modifiers {
            ctrl: true,
            ..Modifiers::default()
        };
        Entry::Key {
            key: Key::Character('v'),
            mods: ctrl,
        }
    }

    /// Ctrl+V in a field the earlier events of the same frame edited (select all, Backspace). The browser's `paste`
    /// event arrives BEFORE the frame that routes the keydown, so the text must wait for the router's own ticket (fresh
    /// for the edited field); answering at once with the last frame's ticket was refused as stale in the browser.
    #[test]
    fn a_paste_waits_for_the_ticket_the_router_asks_with() {
        let (stale, fresh) = (ticket("last frame"), ticket("asked now"));
        let mut paste = Paste::default();
        assert_eq!(entry_events(chord(), &mut paste, Some(stale)).len(), 1);
        assert!(entry_events(Entry::Paste("x".into()), &mut paste, Some(stale)).is_empty());
        // The frame routes the keydown; the field asks.
        assert_eq!(paste.read(fresh), ClipboardRead::Ready("x".into()));
        assert_eq!(paste, Paste::Idle, "one paste answers once");
    }

    /// The other order (a frame ran between the keydown and the `paste` event): the router's ticket waits for the text.
    #[test]
    fn a_ticket_waits_for_the_text_when_the_frame_came_first() {
        let fresh = ticket("asked");
        let mut paste = Paste::default();
        entry_events(chord(), &mut paste, None);
        assert_eq!(paste.read(fresh), ClipboardRead::Later);
        let sent = entry_events(Entry::Paste("y".into()), &mut paste, None);
        assert_eq!(
            sent,
            [InputEvent::Paste {
                ticket: fresh,
                text: "y".into()
            }]
        );
    }

    /// Text pasted with no shortcut (a touch menu's Paste) goes to the focused field under its current ticket; with no
    /// field it goes nowhere. Text a refused shortcut left behind must not leak into the next paste.
    #[test]
    fn unasked_pastes_use_the_focused_field_and_unclaimed_text_is_dropped() {
        let focused = ticket("focused");
        let mut paste = Paste::default();
        let sent = entry_events(Entry::Paste("z".into()), &mut paste, Some(focused));
        assert_eq!(
            sent,
            [InputEvent::Paste {
                ticket: focused,
                text: "z".into()
            }]
        );
        assert!(entry_events(Entry::Paste("w".into()), &mut paste, None).is_empty());

        // A shortcut whose text the router never asked for (a read-only field), then a second shortcut.
        entry_events(chord(), &mut paste, None);
        entry_events(Entry::Paste("old".into()), &mut paste, None);
        entry_events(chord(), &mut paste, None);
        assert_eq!(
            paste.read(ticket("second")),
            ClipboardRead::Later,
            "the old text is gone"
        );
    }
}
