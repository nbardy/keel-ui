//! The text-editing binding: what a field does with the edits the router derives from keys, pointer and IME.
//!
//! Choice, recorded because it is easy to undo by accident: `kit_state::TextValue` stays the only owner of the text,
//! the selection, the revision and the undo history. cosmic-text's `Editor` is deliberately NOT used: it owns the
//! text inside its `Buffer` and keeps its own cursor, selection and undo, which would be a second source of truth
//! that has to be synchronised every frame. What cosmic-text provides here is shaping: [`crate::FieldGeometry`] reads
//! the shaped `Buffer` (layout runs and glyph clusters) for hit-testing, caret, selection and line movement.
//!
//! [`EditorState`] is what a field keeps in `cx.state`: the `TextValue` plus the view-only state that must survive
//! between frames but is not text (composition, sticky column, caret side). [`Edit`] is the one event type a field's
//! reducer receives; [`EditorState::apply`] is the whole reducer.
use cosmic_text::Affinity;
use kit_state::{Applied, Dir, History, Rev, Selection, TextValue};
use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum Mode {
    /// One line; Enter submits; line breaks in pasted text become spaces.
    SingleLine,
    /// Wraps at the field width; Enter inserts a line break.
    MultiLine,
}

/// IME composition. The preedit is shown inline and underlined but is not text until the platform commits it.
#[derive(Clone, Eq, PartialEq, Debug, Default)]
pub enum Composition {
    #[default]
    Idle,
    Preedit(String),
}

/// The column vertical movement tries to return to, so Up then Down over a short line lands where it started.
#[derive(Copy, Clone, PartialEq, Debug)]
pub enum Goal {
    Free,
    /// Text-space x of the column.
    Column(f32),
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum Unit {
    Grapheme,
    Word,
}

/// Motions that need only the text. Motions that need line geometry (up, down, line start and end, pointer) arrive
/// already resolved as [`Edit::Select`].
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum Motion {
    Left,
    Right,
    WordLeft,
    WordRight,
    DocStart,
    DocEnd,
}

/// Everything a field can be told. Relative edits (`Type`, `Delete`, `Move`, `Undo`) apply to the live state, so
/// several of them in one frame compose. Absolute edits (`Select`, `Paste`) name the revision they were computed
/// against and are refused as `Stale` if the text has changed since.
#[derive(Clone, PartialEq, Debug)]
pub enum Edit {
    /// Typing and IME commit: replaces the selection.
    Type(String),
    /// Backspace/Delete: the selection if any, else one grapheme or word.
    Delete(Dir, Unit),
    Move {
        to: Motion,
        extend: bool,
    },
    /// A pointer or line-geometry move, resolved by the router against the laid-out field.
    Select {
        rev: Rev,
        selection: Selection,
        affinity: Affinity,
        goal: Goal,
    },
    SelectAll,
    Undo,
    Redo,
    /// Clipboard text that arrived for the revision the paste was requested at.
    Paste {
        rev: Rev,
        text: String,
    },
    /// The IME's current composition; empty abandons it.
    Preedit(String),
    /// Enter in a single-line field. The state does not change; the app reacts to it.
    Submit,
    /// The field gained keyboard focus (a press, Tab, or a focus claim). The state does not change.
    Focus,
    /// The field lost keyboard focus, for any reason (Tab, a press elsewhere, Escape, another claim). The state
    /// does not change; a component may commit or cancel on it.
    Blur,
    /// Escape while the field had focus. Focus then leaves it, so a `Blur` follows. The state does not change.
    Escape,
}

/// A frame's read-only copy of a field's state, carried by the node so the router and painter need no access to
/// `cx.state`.
#[derive(Clone, PartialEq, Debug)]
pub struct FieldView {
    pub text: String,
    pub selection: Selection,
    pub rev: Rev,
    pub mode: Mode,
    pub composition: Composition,
    pub affinity: Affinity,
    pub goal: Goal,
}

#[derive(Clone, Debug)]
pub struct EditorState {
    value: TextValue,
    mode: Mode,
    composition: Composition,
    affinity: Affinity,
    goal: Goal,
}

impl EditorState {
    pub fn new(text: impl Into<String>, mode: Mode) -> Self {
        Self {
            value: TextValue::new(text),
            mode,
            composition: Composition::Idle,
            affinity: Affinity::After,
            goal: Goal::Free,
        }
    }
    pub fn single_line(text: impl Into<String>) -> Self {
        Self::new(text, Mode::SingleLine)
    }
    pub fn multi_line(text: impl Into<String>) -> Self {
        Self::new(text, Mode::MultiLine)
    }
    /// The text, selection, revision and undo history. Never contains a composition.
    pub fn value(&self) -> &TextValue {
        &self.value
    }
    pub fn text(&self) -> &str {
        self.value.text()
    }
    pub fn mode(&self) -> Mode {
        self.mode
    }
    pub fn composition(&self) -> &Composition {
        &self.composition
    }
    pub fn view(&self) -> FieldView {
        FieldView {
            text: self.value.text().to_owned(),
            selection: self.value.selection(),
            rev: self.value.rev(),
            mode: self.mode,
            composition: self.composition.clone(),
            affinity: self.affinity,
            goal: self.goal,
        }
    }

    /// The reducer. `Ok` also means "nothing to do" (undo with an empty history, deleting at the edge).
    pub fn apply(&mut self, edit: Edit) -> Applied {
        match edit {
            Edit::Preedit(text) => {
                self.composition = match self.clean(&text).as_str() {
                    "" => Composition::Idle,
                    cleaned => Composition::Preedit(cleaned.to_owned()),
                };
                Applied::Ok(self.value.rev())
            }
            Edit::Select {
                rev,
                selection,
                affinity,
                goal,
            } => {
                let applied = self.value.select(rev, selection);
                self.settle(applied, affinity, goal)
            }
            Edit::Type(text) => {
                let applied = match self.clean(&text).as_str() {
                    "" => Applied::Ok(self.value.rev()),
                    cleaned => self.value.type_text(self.value.rev(), cleaned),
                };
                self.settle(applied, Affinity::After, Goal::Free)
            }
            Edit::Paste { rev, text } => {
                let range = self.value.selection().range();
                let applied = match self.clean(&text).as_str() {
                    // Nothing to paste still reports a stale request as stale.
                    "" => self.value.select(rev, self.value.selection()),
                    cleaned => self.value.splice(rev, range, cleaned),
                };
                self.settle(applied, Affinity::After, Goal::Free)
            }
            Edit::Delete(dir, Unit::Grapheme) => {
                let applied = self.value.delete(self.value.rev(), dir);
                self.settle(applied, Affinity::After, Goal::Free)
            }
            Edit::Delete(dir, Unit::Word) => {
                let (rev, selection, text) =
                    (self.value.rev(), self.value.selection(), self.value.text());
                let range = match (selection.range(), dir) {
                    (selected, _) if !selected.is_empty() => selected,
                    (_, Dir::Back) => word_left(text, selection.head)..selection.head,
                    (_, Dir::Forward) => selection.head..word_right(text, selection.head),
                };
                let applied = match range.is_empty() {
                    true => Applied::Ok(rev),
                    false => self.value.splice(rev, range, ""),
                };
                self.settle(applied, Affinity::After, Goal::Free)
            }
            Edit::Move { to, extend } => {
                let selection = self.value.selection();
                let head = motion_head(self.value.text(), selection, to, extend);
                let moved = Selection {
                    anchor: if extend { selection.anchor } else { head },
                    head,
                };
                let applied = self.value.select(self.value.rev(), moved);
                self.settle(applied, Affinity::After, Goal::Free)
            }
            Edit::SelectAll => {
                self.value.select_all();
                self.settle(Applied::Ok(self.value.rev()), Affinity::After, Goal::Free)
            }
            Edit::Undo => {
                let moved = self.value.undo();
                self.settle(history(moved, &self.value), Affinity::After, Goal::Free)
            }
            Edit::Redo => {
                let moved = self.value.redo();
                self.settle(history(moved, &self.value), Affinity::After, Goal::Free)
            }
            Edit::Submit | Edit::Focus | Edit::Blur | Edit::Escape => Applied::Ok(self.value.rev()),
        }
    }

    /// An edit that was accepted ends any composition and sets where the caret sits; a refused one changes nothing.
    fn settle(&mut self, applied: Applied, affinity: Affinity, goal: Goal) -> Applied {
        match applied {
            Applied::Ok(_) => {
                self.composition = Composition::Idle;
                self.affinity = affinity;
                self.goal = goal;
            }
            Applied::Stale { .. } | Applied::Misplaced => {}
        }
        applied
    }

    /// Typed and pasted text as the field can hold it: one kind of line break, no control characters, and in a
    /// single-line field no line breaks at all (a pasted paragraph becomes one line, not a Submit).
    fn clean(&self, text: &str) -> String {
        let unified = text.replace("\r\n", "\n").replace('\r', "\n");
        let break_as = match self.mode {
            Mode::SingleLine => ' ',
            Mode::MultiLine => '\n',
        };
        unified
            .chars()
            .filter(|c| *c == '\n' || !c.is_control())
            .map(|c| if c == '\n' { break_as } else { c })
            .collect()
    }
}

fn history(moved: History, value: &TextValue) -> Applied {
    match moved {
        History::Moved(rev) => Applied::Ok(rev),
        History::Empty => Applied::Ok(value.rev()),
    }
}

/// Where the caret goes. A plain Left/Right over a selection collapses it to that edge, as every platform does.
fn motion_head(text: &str, selection: Selection, to: Motion, extend: bool) -> usize {
    let range = selection.range();
    let collapsing = !extend && !range.is_empty();
    match to {
        Motion::Left if collapsing => range.start,
        Motion::Right if collapsing => range.end,
        Motion::Left => prev_grapheme(text, selection.head),
        Motion::Right => next_grapheme(text, selection.head),
        Motion::WordLeft => word_left(text, selection.head),
        Motion::WordRight => word_right(text, selection.head),
        Motion::DocStart => 0,
        Motion::DocEnd => text.len(),
    }
}

pub fn prev_grapheme(text: &str, at: usize) -> usize {
    text.grapheme_indices(true)
        .map(|(start, _)| start)
        .take_while(|start| *start < at)
        .last()
        .unwrap_or(0)
}

pub fn next_grapheme(text: &str, at: usize) -> usize {
    text.grapheme_indices(true)
        .map(|(start, _)| start)
        .find(|start| *start > at)
        .unwrap_or(text.len())
}

/// A word is a run that is not only whitespace and ASCII punctuation (so CJK and emoji count as words).
fn is_word(segment: &str) -> bool {
    !segment
        .chars()
        .all(|c| c.is_whitespace() || c.is_ascii_punctuation())
}

/// Start of the word at or before `at`, skipping separators first.
pub fn word_left(text: &str, at: usize) -> usize {
    text.split_word_bound_indices()
        .filter(|(start, segment)| *start < at && is_word(segment))
        .map(|(start, _)| start)
        .next_back()
        .unwrap_or(0)
}

/// End of the word at or after `at`, skipping separators first.
pub fn word_right(text: &str, at: usize) -> usize {
    text.split_word_bound_indices()
        .filter(|(start, segment)| start + segment.len() > at && is_word(segment))
        .map(|(start, segment)| start + segment.len())
        .next()
        .unwrap_or(text.len())
}

/// The word, or run of separators, containing `at` (double-click).
pub fn word_at(text: &str, at: usize) -> Range<usize> {
    text.split_word_bound_indices()
        .rfind(|(start, _)| *start <= at)
        .map_or(at..at, |(start, segment)| start..start + segment.len())
}

/// The line between two line breaks containing `at`, without the break (triple-click).
pub fn paragraph_at(text: &str, at: usize) -> Range<usize> {
    let start = text[..at].rfind('\n').map_or(0, |i| i + 1);
    let end = text[at..].find('\n').map_or(text.len(), |i| at + i);
    start..end
}
