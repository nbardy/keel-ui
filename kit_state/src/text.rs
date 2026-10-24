//! `TextValue`: editable text as a pure value (text, selection, revision, undo), usable inside `cx.state`. It knows
//! nothing of fonts or layout; a text field binds it to a shaping editor and sends edits as events.
//!
//! Offsets are UTF-8 byte offsets that always sit on extended-grapheme boundaries, so a caret never splits `é`
//! (`e` + U+0301) or `👍🏽`. An edit names the revision it was computed against; an edit for an older text is refused
//! as `Stale` rather than applied at offsets that now mean something else.
use core::ops::Range;
use std::collections::VecDeque;
use unicode_segmentation::UnicodeSegmentation;

/// Undo steps kept; the oldest is dropped beyond this (a long session must not grow memory without bound).
pub const UNDO_DEPTH: usize = 100;

/// Text revision: bumped by every change of the text (including undo/redo), never by a selection change.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Rev(u64);

/// `anchor` is where the selection began, `head` is the caret. Equal means a caret with nothing selected.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Selection {
    pub anchor: usize,
    pub head: usize,
}
impl Selection {
    pub fn caret(at: usize) -> Self {
        Self {
            anchor: at,
            head: at,
        }
    }
    pub fn range(&self) -> Range<usize> {
        self.anchor.min(self.head)..self.anchor.max(self.head)
    }
}

/// What became of an edit.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Applied {
    Ok(Rev),
    /// Computed against an older text: refused, the caller re-derives it against `current`.
    Stale {
        current: Rev,
    },
    /// An offset is past the end or inside a grapheme (or a UTF-8 sequence), or a range is reversed.
    Misplaced,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Dir {
    Back,
    Forward,
}

/// What undo/redo did.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum History {
    Moved(Rev),
    Empty,
}

#[derive(Clone, Debug)]
struct Snap {
    text: String,
    selection: Selection,
}

/// Whether the next typed text joins the current undo step.
#[derive(Copy, Clone, Debug)]
enum Run {
    Broken,
    /// Typing that ended at `end`; `space` if its last char was whitespace (the next word starts a new step).
    Typing {
        end: usize,
        space: bool,
    },
}

#[derive(Clone, Debug)]
pub struct TextValue {
    text: String,
    selection: Selection,
    rev: Rev,
    undo: VecDeque<Snap>,
    redo: Vec<Snap>,
    run: Run,
}

impl TextValue {
    /// The caret starts at the end.
    pub fn new(text: impl Into<String>) -> Self {
        let text = text.into();
        Self {
            selection: Selection::caret(text.len()),
            text,
            rev: Rev(0),
            undo: VecDeque::new(),
            redo: Vec::new(),
            run: Run::Broken,
        }
    }
    pub fn text(&self) -> &str {
        &self.text
    }
    pub fn selection(&self) -> Selection {
        self.selection
    }
    pub fn rev(&self) -> Rev {
        self.rev
    }
    pub fn selected(&self) -> &str {
        &self.text[self.selection.range()]
    }

    /// Replace `range` with `insert` (paste, IME commit, an edit from elsewhere): one undo step. The selection is
    /// carried through: before the range it stays, after it shifts, inside it lands at the end of `insert`.
    pub fn splice(&mut self, rev: Rev, range: Range<usize>, insert: &str) -> Applied {
        match self.admit(rev, &[range.start, range.end]) {
            Err(refused) => refused,
            Ok(()) if range.start > range.end => Applied::Misplaced,
            Ok(()) => {
                self.checkpoint();
                let carry = |at: usize| match (at <= range.start, at >= range.end) {
                    (true, _) => at,
                    (false, true) => at - range.len() + insert.len(),
                    (false, false) => range.start + insert.len(),
                };
                let (anchor, head) = (carry(self.selection.anchor), carry(self.selection.head));
                self.text.replace_range(range, insert);
                self.run = Run::Broken;
                Applied::Ok(self.place(anchor, head))
            }
        }
    }

    /// Typing: replace the selection with `typed`, caret after it. Consecutive typing at the caret is one undo step
    /// until a word starts after whitespace, the caret moves, or any other edit happens.
    pub fn type_text(&mut self, rev: Rev, typed: &str) -> Applied {
        match self.admit(rev, &[]) {
            Err(refused) => refused,
            Ok(()) => {
                let range = self.selection.range();
                let joins = match self.run {
                    Run::Typing { end, space } => {
                        range.is_empty()
                            && end == range.start
                            && !(space && !typed.starts_with(char::is_whitespace))
                    }
                    Run::Broken => false,
                };
                match joins {
                    true => self.redo.clear(),
                    false => self.checkpoint(),
                }
                self.text.replace_range(range.clone(), typed);
                let caret = range.start + typed.len();
                let rev = self.place(caret, caret);
                self.run = Run::Typing {
                    end: self.selection.head,
                    space: typed.ends_with(char::is_whitespace),
                };
                Applied::Ok(rev)
            }
        }
    }

    /// Backspace / Delete: the selection if any, else one grapheme in `dir`. At the edge there is nothing to delete
    /// and the text (and its revision) stay as they are.
    pub fn delete(&mut self, rev: Rev, dir: Dir) -> Applied {
        match self.admit(rev, &[]) {
            Err(refused) => refused,
            Ok(()) => {
                let head = self.selection.head;
                let range = match (self.selection.range().is_empty(), dir) {
                    (false, Dir::Back | Dir::Forward) => self.selection.range(),
                    (true, Dir::Back) => self.prev_boundary(head)..head,
                    (true, Dir::Forward) => head..self.next_boundary(head),
                };
                match range.is_empty() {
                    true => Applied::Ok(self.rev),
                    false => {
                        self.checkpoint();
                        self.text.replace_range(range.clone(), "");
                        self.run = Run::Broken;
                        Applied::Ok(self.place(range.start, range.start))
                    }
                }
            }
        }
    }

    /// Move the caret or selection (a click, a drag, arrow keys computed against `rev`'s layout).
    pub fn select(&mut self, rev: Rev, selection: Selection) -> Applied {
        match self.admit(rev, &[selection.anchor, selection.head]) {
            Err(refused) => refused,
            Ok(()) => {
                self.selection = selection;
                self.run = Run::Broken;
                Applied::Ok(self.rev)
            }
        }
    }
    pub fn select_all(&mut self) {
        self.selection = Selection {
            anchor: 0,
            head: self.text.len(),
        };
        self.run = Run::Broken;
    }
    /// Select the word (or the run of spaces or punctuation) at `at`; a double-click.
    pub fn select_word(&mut self, rev: Rev, at: usize) -> Applied {
        match self.admit(rev, &[at]) {
            Err(refused) => refused,
            Ok(()) => {
                let word = self
                    .text
                    .split_word_bound_indices()
                    .rfind(|(start, _)| *start <= at)
                    .map_or(at..at, |(start, word)| start..start + word.len()); // empty text: no word, a caret
                self.select(
                    rev,
                    Selection {
                        anchor: word.start,
                        head: word.end,
                    },
                )
            }
        }
    }

    pub fn undo(&mut self) -> History {
        match self.undo.pop_back() {
            None => History::Empty,
            Some(snap) => {
                self.redo.push(self.snapshot());
                self.restore(snap)
            }
        }
    }
    pub fn redo(&mut self) -> History {
        match self.redo.pop() {
            None => History::Empty,
            Some(snap) => {
                self.push_undo(self.snapshot());
                self.restore(snap)
            }
        }
    }

    fn admit(&self, rev: Rev, offsets: &[usize]) -> Result<(), Applied> {
        match (
            rev == self.rev,
            offsets.iter().all(|at| self.is_boundary(*at)),
        ) {
            (false, _) => Err(Applied::Stale { current: self.rev }),
            (true, false) => Err(Applied::Misplaced),
            (true, true) => Ok(()),
        }
    }
    fn is_boundary(&self, at: usize) -> bool {
        at == self.text.len() || self.boundaries().any(|b| b == at)
    }
    fn boundaries(&self) -> impl Iterator<Item = usize> + '_ {
        self.text.grapheme_indices(true).map(|(at, _)| at)
    }
    fn prev_boundary(&self, at: usize) -> usize {
        self.boundaries()
            .take_while(|b| *b < at)
            .last()
            .unwrap_or(0)
    }
    fn next_boundary(&self, at: usize) -> usize {
        self.boundaries()
            .find(|b| *b > at)
            .unwrap_or(self.text.len())
    }
    /// Set the selection after a text change, then bump the revision. An edit can merge graphemes (a combining mark
    /// typed before `e`, or inserted right after a caret), so an offset that was a boundary may now sit inside a
    /// cluster: it moves forward to the next boundary, which keeps the invariant every other method relies on.
    fn place(&mut self, anchor: usize, head: usize) -> Rev {
        let snap = |at: usize| match self.is_boundary(at) {
            true => at,
            false => self.next_boundary(at),
        };
        self.selection = Selection {
            anchor: snap(anchor),
            head: snap(head),
        };
        self.rev = Rev(self.rev.0 + 1);
        self.rev
    }
    fn snapshot(&self) -> Snap {
        Snap {
            text: self.text.clone(),
            selection: self.selection,
        }
    }
    /// Start a new undo step before an edit; a new edit forgets what was undone.
    fn checkpoint(&mut self) {
        self.push_undo(self.snapshot());
        self.redo.clear();
    }
    fn push_undo(&mut self, snap: Snap) {
        if self.undo.len() == UNDO_DEPTH {
            self.undo.pop_front();
        }
        self.undo.push_back(snap);
    }
    fn restore(&mut self, snap: Snap) -> History {
        self.text = snap.text;
        self.run = Run::Broken;
        History::Moved(self.place(snap.selection.anchor, snap.selection.head))
    }
}
