//! Text fields in the router: keys, text, IME, clipboard and pointer become [`Edit`]s for the field's reducer.
//!
//! The field's reducer (`EditorState::apply`) owns the text. This module owns nothing but the translation: it reads
//! the frame's [`FieldView`] and [`FieldGeometry`] off the laid-out node, resolves anything that needs line
//! geometry (a click, Up/Down, Home/End) into an absolute [`Edit::Select`], and leaves everything else relative so
//! several events in one frame compose. Clipboard traffic goes through [`UiMemory`]'s outbox, never straight to the
//! OS: the router is pure and the host owns the clipboard.
use crate::clipboard::{ClipboardRequest, PasteTicket};
use crate::router::{Claim, Router};
use crate::{AnimKey, Focus, Gesture, ImeEvent, Key, Modifiers, NamedKey, PointerEvent, UiMemory};
use kit_core::kurbo::{Point, Rect, Vec2};
use kit_core::{Time, WidgetId};
use kit_view::kit_text::{
    Caret, Dir, FieldGeometry, FieldView, LineEdge, Masking, Mode, Motion, Selection, Unit,
    Vertical, paragraph_at, word_at,
};
use kit_view::{Edit, Goal, LaidKind, LaidNode, LaidOut};
use std::rc::Rc;
use std::time::Duration;

/// Which modifier does what: the one thing about editing keys that differs by operating system.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum Platform {
    /// Cmd is the command key, Alt moves by word, Cmd+Arrow moves by line.
    Apple,
    /// Ctrl is the command key and moves by word.
    Other,
}
impl Platform {
    /// The platform the code was built for. A browser build reports `Other`; the web host overrides it from the
    /// user agent.
    pub fn native() -> Self {
        match cfg!(any(target_os = "macos", target_os = "ios")) {
            true => Platform::Apple,
            false => Platform::Other,
        }
    }
    fn command(self, mods: Modifiers) -> bool {
        match self {
            Platform::Apple => mods.meta,
            Platform::Other => mods.ctrl,
        }
    }
    fn by_word(self, mods: Modifiers) -> bool {
        match self {
            Platform::Apple => mods.alt,
            Platform::Other => mods.ctrl,
        }
    }
    fn by_line(self, mods: Modifiers) -> bool {
        match self {
            Platform::Apple => mods.meta,
            Platform::Other => false,
        }
    }
    /// Start/end of the whole text with Home/End and the vertical arrows.
    fn by_document(self, mods: Modifiers) -> bool {
        match self {
            Platform::Apple => mods.meta || mods.ctrl,
            Platform::Other => mods.ctrl,
        }
    }
}

/// What a drag grows by: the press that began it chose.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum Granularity {
    Char,
    Word,
    Line,
}

/// Consecutive presses on one field, for double and triple click.
#[derive(Copy, Clone, PartialEq, Debug)]
pub(crate) enum ClickRun {
    Nothing,
    Last {
        field: WidgetId,
        at: Point,
        time: Time,
        count: u8,
    },
}
impl ClickRun {
    /// Record a press and say which one in its run it is (1, 2 or 3; a fourth starts over).
    fn press(&mut self, field: WidgetId, at: Point, time: Time, within: Duration, slop: f64) -> u8 {
        let count = match *self {
            ClickRun::Last {
                field: f,
                at: a,
                time: t,
                count,
            } if f == field && (at - a).hypot() <= slop && time.since(t) <= within => count % 3 + 1,
            ClickRun::Last { .. } | ClickRun::Nothing => 1,
        };
        *self = ClickRun::Last {
            field,
            at,
            time,
            count,
        };
        count
    }
}

/// What the keyboard did in a focused field.
enum Action {
    Edit(Edit),
    Copy,
    Cut,
    Paste,
    /// Escape.
    Leave,
    /// Taken by the field but meaningless in it (Tab, Space: the text arrives as `InputEvent::Text`).
    Nothing,
}

/// A focused field's node, with the frame's copies of its state.
struct Field<'a, Msg> {
    id: WidgetId,
    rect: Rect,
    /// Add to a screen point to get the point in `LaidOut` coordinates (scroll offsets of the areas around it).
    shift: Vec2,
    view: &'a FieldView,
    geometry: &'a FieldGeometry,
    masking: Masking,
    inset: Vec2,
    on_edit: &'a Rc<dyn Fn(Edit) -> Msg>,
}

impl<'a, Msg> Field<'a, Msg> {
    fn of(node: &'a LaidNode<Msg>, shift: Vec2) -> Option<Self> {
        match &node.kind {
            LaidKind::TextField {
                id,
                view,
                geometry,
                on_edit,
                field,
                ..
            } => Some(Self {
                id: *id,
                rect: node.rect,
                shift,
                view,
                geometry,
                masking: field.masking,
                inset: field.inset,
                on_edit,
            }),
            LaidKind::Stack { .. }
            | LaidKind::Panel { .. }
            | LaidKind::Text { .. }
            | LaidKind::Image { .. }
            | LaidKind::Shape { .. }
            | LaidKind::Interactive { .. }
            | LaidKind::Scroll { .. }
            | LaidKind::Layer { .. }
            | LaidKind::World { .. }
            | LaidKind::Portal { .. } => None,
        }
    }
    /// A screen point as a point in the text's own space (origin at the text, before scroll).
    fn text_point(&self, screen: Point) -> Point {
        screen + self.shift - self.rect.origin().to_vec2() - self.inset
    }
    fn caret(&self) -> Caret {
        Caret {
            offset: self.view.selection.head,
            affinity: self.view.affinity,
        }
    }
    /// An absolute selection move to `to`; `extend` keeps the anchor.
    fn select(&self, to: Caret, goal: Goal, extend: bool) -> Edit {
        Edit::Select {
            rev: self.view.rev,
            selection: Selection {
                anchor: if extend {
                    self.view.selection.anchor
                } else {
                    to.offset
                },
                head: to.offset,
            },
            affinity: to.affinity,
            goal,
        }
    }
}

/// The field with `id`, wherever it sits, and the scroll shift to reach it.
fn locate<'a, Msg>(
    laid: &'a LaidOut<Msg>,
    memory: &UiMemory,
    id: WidgetId,
) -> Option<Field<'a, Msg>> {
    fn walk<'a, Msg>(
        node: &'a LaidNode<Msg>,
        memory: &UiMemory,
        id: WidgetId,
        shift: Vec2,
    ) -> Option<Field<'a, Msg>> {
        let inside = match &node.kind {
            LaidKind::Scroll { id: area, .. } => shift + memory.scroll_offset(*area),
            LaidKind::Stack { .. }
            | LaidKind::Panel { .. }
            | LaidKind::Text { .. }
            | LaidKind::Image { .. }
            | LaidKind::Shape { .. }
            | LaidKind::Interactive { .. }
            | LaidKind::TextField { .. }
            | LaidKind::Layer { .. }
            | LaidKind::World { .. }
            | LaidKind::Portal { .. } => shift,
        };
        match node.id() == Some(id) {
            true => Field::of(node, shift),
            false => node
                .children()
                .iter()
                .find_map(|child| walk(child, memory, id, inside)),
        }
    }
    walk(&laid.root, memory, id, Vec2::ZERO)
}

/// What the host should do for text entry right now.
#[derive(Clone, PartialEq, Debug)]
pub enum TextTarget {
    /// No field has focus: hide the soft keyboard and the IME.
    Unfocused,
    Field {
        /// The caret in screen px: where the IME candidate window and a hidden text agent belong.
        caret: Rect,
        mode: Mode,
        /// A password field: no suggestions, no autofill history.
        secret: bool,
    },
}

/// Where text entry is. Poll once per frame after routing.
pub fn text_target<Msg>(laid: &LaidOut<Msg>, memory: &UiMemory) -> TextTarget {
    match memory.focus() {
        Focus::Unfocused => TextTarget::Unfocused,
        Focus::On(id) => match locate(laid, memory, id) {
            None => TextTarget::Unfocused,
            Some(field) => TextTarget::Field {
                caret: (field.geometry.caret() + (field.rect.origin().to_vec2() + field.inset))
                    - field.shift,
                mode: field.view.mode,
                secret: field.masking == Masking::Bullets,
            },
        },
    }
}

/// A paste that starts now, for text the platform delivered without being asked (a touch menu's Paste, a context
/// menu): `None` when no field has focus.
pub fn paste_ticket<Msg>(laid: &LaidOut<Msg>, memory: &UiMemory) -> Option<PasteTicket> {
    match memory.focus() {
        Focus::Unfocused => None,
        Focus::On(id) => locate(laid, memory, id).map(|field| PasteTicket {
            field: id,
            rev: field.view.rev,
        }),
    }
}

impl Router {
    /// Text committed by the keyboard or an input method, a composition, or clipboard text that was asked for.
    pub(crate) fn text_event<Msg>(
        &self,
        laid: &LaidOut<Msg>,
        memory: &mut UiMemory,
        event: TextEvent,
        now: Time,
        out: &mut Vec<Msg>,
    ) -> Claim {
        let Some(field) = focused(laid, memory) else {
            return Claim::Free;
        };
        let edit = match event {
            TextEvent::Typed(text) => Some(Edit::Type(text)),
            TextEvent::Ime(ImeEvent::Preedit(text)) => Some(Edit::Preedit(text)),
            TextEvent::Ime(ImeEvent::Commit(text)) => Some(Edit::Type(text)),
            TextEvent::Ime(ImeEvent::Cancel) => Some(Edit::Preedit(String::new())),
            TextEvent::Pasted { ticket, text } => {
                (ticket.field == field.id).then_some(Edit::Paste {
                    rev: ticket.rev,
                    text,
                })
            }
        };
        match edit {
            Some(edit) => send(&field, memory, edit, now, out),
            None => Claim::Taken,
        }
    }

    /// A key pressed while a field has focus.
    pub(crate) fn field_key<Msg>(
        &self,
        laid: &LaidOut<Msg>,
        memory: &mut UiMemory,
        key: Key,
        mods: Modifiers,
        now: Time,
        out: &mut Vec<Msg>,
    ) -> Claim {
        let Some(field) = focused(laid, memory) else {
            return Claim::Free;
        };
        match action(&field, key, mods, self.config.platform) {
            Action::Edit(edit) => send(&field, memory, edit, now, out),
            Action::Copy => {
                copy(&field, memory);
                Claim::Taken
            }
            Action::Cut => {
                copy(&field, memory);
                send(
                    &field,
                    memory,
                    Edit::Delete(Dir::Back, Unit::Grapheme),
                    now,
                    out,
                )
            }
            Action::Paste => {
                let ticket = PasteTicket {
                    field: field.id,
                    rev: field.view.rev,
                };
                memory.clipboard.push(ClipboardRequest::Read(ticket));
                Claim::Taken
            }
            // keys: the field hears Escape, then (from `focus::tell`) Blur, so it can cancel without host help.
            Action::Leave => {
                send(&field, memory, Edit::Escape, now, out);
                memory.focus = Focus::Unfocused;
                Claim::Taken
            }
            Action::Nothing => Claim::Taken,
        }
    }

    /// A press has just given the field under it focus: place the caret (or select a word or line for the second
    /// and third press of a run) and start the drag that extends it.
    pub(crate) fn field_pressed<Msg>(
        &self,
        laid: &LaidOut<Msg>,
        memory: &mut UiMemory,
        ev: PointerEvent,
        now: Time,
        out: &mut Vec<Msg>,
    ) {
        let Some(field) = focused(laid, memory) else {
            return;
        };
        if !field.rect_contains(ev.at) {
            return;
        }
        let hit = field.geometry.hit(field.text_point(ev.at));
        let count = memory.clicks.press(
            field.id,
            ev.at,
            now,
            self.config.multi_click,
            self.config.slop,
        );
        let (range, by) = match count {
            2 => (word_at(&field.view.text, hit.offset), Granularity::Word),
            3 => (
                paragraph_at(&field.view.text, hit.offset),
                Granularity::Line,
            ),
            _ => (hit.offset..hit.offset, Granularity::Char),
        };
        let selection = Selection {
            anchor: range.start,
            head: range.end,
        };
        let edit = Edit::Select {
            rev: field.view.rev,
            selection,
            affinity: hit.affinity,
            goal: Goal::Free,
        };
        send(&field, memory, edit, now, out);
        if let Some(pointer) = memory.pointers.get_mut(&ev.id) {
            pointer.gesture = Gesture::Select {
                field: field.id,
                anchor: selection,
                by,
                epoch: laid.epoch,
            };
        }
    }

    /// The pointer moved while a press that began in a field is held: grow the selection from where it began.
    pub(crate) fn field_dragged<Msg>(
        &self,
        laid: &LaidOut<Msg>,
        memory: &mut UiMemory,
        ev: PointerEvent,
        drag: Selecting,
        now: Time,
        out: &mut Vec<Msg>,
    ) {
        let Selecting { field, anchor, by } = drag;
        let Some(field) = locate(laid, memory, field) else {
            return;
        };
        let hit = field.geometry.hit(field.text_point(ev.at));
        let text = &field.view.text;
        let reached = match by {
            Granularity::Char => hit.offset..hit.offset,
            Granularity::Word => word_at(text, hit.offset),
            Granularity::Line => paragraph_at(text, hit.offset),
        };
        let held = anchor.range();
        let selection = match reached.start < held.start {
            true => Selection {
                anchor: held.end,
                head: reached.start,
            },
            false => Selection {
                anchor: held.start,
                head: reached.end.max(held.end),
            },
        };
        let edit = Edit::Select {
            rev: field.view.rev,
            selection,
            affinity: hit.affinity,
            goal: Goal::Free,
        };
        send(&field, memory, edit, now, out);
    }
}

/// A held press that began in a field: what it grows from and by.
#[derive(Copy, Clone, PartialEq, Debug)]
pub(crate) struct Selecting {
    pub field: WidgetId,
    pub anchor: Selection,
    pub by: Granularity,
}

/// Input that reaches a focused field as text.
pub(crate) enum TextEvent {
    Typed(String),
    Ime(ImeEvent),
    Pasted { ticket: PasteTicket, text: String },
}

impl<Msg> Field<'_, Msg> {
    fn rect_contains(&self, screen: Point) -> bool {
        self.rect.contains(screen + self.shift)
    }
}

/// The focused field. One that left the screen, or whose id now names something else, loses focus.
fn focused<'a, Msg>(laid: &'a LaidOut<Msg>, memory: &mut UiMemory) -> Option<Field<'a, Msg>> {
    let Focus::On(id) = memory.focus else {
        return None;
    };
    let field = locate(laid, memory, id);
    if field.is_none() {
        memory.focus = Focus::Unfocused;
    }
    field
}

/// Hand an edit to the field's reducer; the caret stops blinking so it is visible while the player types.
fn send<Msg>(
    field: &Field<'_, Msg>,
    memory: &mut UiMemory,
    edit: Edit,
    now: Time,
    out: &mut Vec<Msg>,
) -> Claim {
    memory.anim.insert((field.id, AnimKey::Caret), now);
    out.push(Rc::clone(field.on_edit)(edit));
    Claim::Taken
}

/// The selection to the clipboard. A masked field never leaks its value.
fn copy<Msg>(field: &Field<'_, Msg>, memory: &mut UiMemory) {
    let selected = &field.view.text[field.view.selection.range()];
    match (field.masking, selected.is_empty()) {
        (Masking::Plain, false) => memory
            .clipboard
            .push(ClipboardRequest::Write(selected.to_owned())),
        (Masking::Plain, true) | (Masking::Bullets, _) => {}
    }
}

fn action<Msg>(field: &Field<'_, Msg>, key: Key, mods: Modifiers, platform: Platform) -> Action {
    let (view, extend) = (field.view, mods.shift);
    let command = platform.command(mods);
    let motion = |to: Motion| Action::Edit(Edit::Move { to, extend });
    let on_line = |edge: LineEdge| {
        let to = field.geometry.line_edge(field.caret(), edge);
        Action::Edit(field.select(to, Goal::Free, extend))
    };
    let vertical = |dir: Vertical| {
        let (to, goal) = field.geometry.vertical(field.caret(), view.goal, dir);
        Action::Edit(field.select(to, goal, extend))
    };
    match key {
        Key::Character(c) => match (command, c, mods.shift, platform) {
            (true, 'a', _, _) => Action::Edit(Edit::SelectAll),
            (true, 'c', _, _) => Action::Copy,
            (true, 'x', _, _) => Action::Cut,
            (true, 'v', _, _) => Action::Paste,
            (true, 'z', false, _) => Action::Edit(Edit::Undo),
            (true, 'z', true, _) | (true, 'y', _, Platform::Other) => Action::Edit(Edit::Redo),
            _ => Action::Nothing,
        },
        Key::Named(named) => match named {
            NamedKey::ArrowLeft => match (platform.by_line(mods), platform.by_word(mods)) {
                (true, _) => on_line(LineEdge::Start),
                (false, true) => motion(Motion::WordLeft),
                (false, false) => motion(Motion::Left),
            },
            NamedKey::ArrowRight => match (platform.by_line(mods), platform.by_word(mods)) {
                (true, _) => on_line(LineEdge::End),
                (false, true) => motion(Motion::WordRight),
                (false, false) => motion(Motion::Right),
            },
            NamedKey::ArrowUp => match (view.mode, platform.by_document(mods)) {
                (Mode::MultiLine, false) => vertical(Vertical::Up),
                (Mode::MultiLine, true) | (Mode::SingleLine, _) => motion(Motion::DocStart),
            },
            NamedKey::ArrowDown => match (view.mode, platform.by_document(mods)) {
                (Mode::MultiLine, false) => vertical(Vertical::Down),
                (Mode::MultiLine, true) | (Mode::SingleLine, _) => motion(Motion::DocEnd),
            },
            NamedKey::Home => match platform.by_document(mods) {
                true => motion(Motion::DocStart),
                false => on_line(LineEdge::Start),
            },
            NamedKey::End => match platform.by_document(mods) {
                true => motion(Motion::DocEnd),
                false => on_line(LineEdge::End),
            },
            NamedKey::Backspace => delete(Dir::Back, platform.by_word(mods)),
            NamedKey::Delete => delete(Dir::Forward, platform.by_word(mods)),
            NamedKey::Enter => match view.mode {
                Mode::SingleLine => Action::Edit(Edit::Submit),
                Mode::MultiLine => Action::Edit(Edit::Type("\n".into())),
            },
            NamedKey::Escape => Action::Leave,
            NamedKey::Tab | NamedKey::Space => Action::Nothing,
        },
    }
}

fn delete(dir: Dir, by_word: bool) -> Action {
    let unit = match by_word {
        true => Unit::Word,
        false => Unit::Grapheme,
    };
    Action::Edit(Edit::Delete(dir, unit))
}
