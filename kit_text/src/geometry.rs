//! A field's shaped lines as plain data: hit-testing, caret, selection and line movement without a text system.
//!
//! [`FieldGeometry::build`] reads a shaped cosmic-text `Buffer` once per frame (layout runs and glyph clusters) and
//! keeps what editing needs. Every public offset is a byte offset into the field's VALUE, always on a grapheme
//! boundary: the composition shown inline and the bullets of a masked field are mapped away inside ([`Shaped`]), so
//! the router and the reducer never see shaped-string offsets. Coordinates are in view space: relative to the text
//! origin, with the single-line horizontal scroll already applied.
use crate::editor::{Composition, FieldView, Goal, Mode};
use cosmic_text::{Affinity, Buffer};
use kit_core::kurbo::{Point, Rect};
use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;

/// How the value is drawn.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum Masking {
    Plain,
    /// Password / seed phrase: one bullet per grapheme.
    Bullets,
}

const BULLET: char = '\u{2022}';
const BULLET_LEN: usize = BULLET.len_utf8();

/// The string that is shaped: the value (masked) with the composition inline at the selection, which the
/// composition replaces while it lasts. Maps shaped offsets to value offsets and back.
#[derive(Clone, PartialEq, Debug)]
pub struct Shaped {
    text: String,
    /// Shaped bytes before the composition.
    before: usize,
    /// Shaped bytes of the composition.
    composed: usize,
    /// The value range the composition stands in for (empty at the end of the value when idle).
    gap: Range<usize>,
    mask: Mask,
}

#[derive(Clone, PartialEq, Debug)]
enum Mask {
    Verbatim,
    /// Value offset of each grapheme, then the length: bullet `i` stands for grapheme `i`.
    Bullets(Vec<usize>),
}

impl Shaped {
    pub fn new(view: &FieldView, masking: Masking) -> Self {
        let (gap, preedit) = match &view.composition {
            Composition::Idle => (view.text.len()..view.text.len(), ""),
            Composition::Preedit(text) => (view.selection.range(), text.as_str()),
        };
        let mask = match masking {
            Masking::Plain => Mask::Verbatim,
            Masking::Bullets => Mask::Bullets(grapheme_starts(&view.text)),
        };
        let shape = |part: &str| match masking {
            Masking::Plain => part.to_owned(),
            Masking::Bullets => part.graphemes(true).map(|_| BULLET).collect(),
        };
        let (head, tail) = (shape(&view.text[..gap.start]), shape(&view.text[gap.end..]));
        Self {
            before: head.len(),
            composed: preedit.len(),
            text: format!("{head}{preedit}{tail}"),
            gap,
            mask,
        }
    }
    /// What to shape.
    pub fn text(&self) -> &str {
        &self.text
    }
    fn composition(&self) -> Range<usize> {
        self.before..self.before + self.composed
    }
    fn to_value(&self, shaped: usize) -> usize {
        let after = self.before + self.composed;
        match (shaped <= self.before, shaped < after) {
            (true, _) => self.unmask(0, shaped),
            (false, true) => self.gap.start,
            (false, false) => self.unmask(self.gap.end, shaped - after),
        }
    }
    /// Value offset `bytes` shaped bytes after value offset `from`.
    fn unmask(&self, from: usize, bytes: usize) -> usize {
        match &self.mask {
            Mask::Verbatim => from + bytes,
            Mask::Bullets(starts) => {
                let first = starts.partition_point(|s| *s < from);
                starts[(first + bytes / BULLET_LEN).min(starts.len() - 1)]
            }
        }
    }
    fn to_shaped(&self, value: usize) -> usize {
        let after = self.before + self.composed;
        match (value <= self.gap.start, value >= self.gap.end) {
            (true, _) => self.mask(0, value),
            (false, true) => after + self.mask(self.gap.end, value),
            (false, false) => self.before, // inside the replaced selection: at the composition
        }
    }
    /// Shaped bytes of the value between `from` and `to`.
    fn mask(&self, from: usize, to: usize) -> usize {
        match &self.mask {
            Mask::Verbatim => to - from,
            Mask::Bullets(starts) => {
                (starts.partition_point(|s| *s < to) - starts.partition_point(|s| *s < from))
                    * BULLET_LEN
            }
        }
    }
}

fn grapheme_starts(text: &str) -> Vec<usize> {
    text.grapheme_indices(true)
        .map(|(start, _)| start)
        .chain([text.len()])
        .collect()
}

/// A caret position: a boundary can be the end of one wrapped line and the start of the next.
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct Caret {
    pub offset: usize,
    pub affinity: Affinity,
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum Vertical {
    Up,
    Down,
}
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum LineEdge {
    Start,
    End,
}

/// One shaped glyph cluster: shaped byte range and horizontal extent.
#[derive(Clone, PartialEq, Debug)]
pub struct Cluster {
    pub start: usize,
    pub end: usize,
    pub x: f32,
    pub w: f32,
    pub rtl: bool,
}
impl Cluster {
    fn leading(&self) -> (usize, f32) {
        match self.rtl {
            false => (self.start, self.x),
            true => (self.end, self.x + self.w),
        }
    }
    fn trailing(&self) -> (usize, f32) {
        match self.rtl {
            false => (self.end, self.x + self.w),
            true => (self.start, self.x),
        }
    }
}

/// One visual line (a wrapped piece of a paragraph, or a whole short paragraph).
#[derive(Clone, PartialEq, Debug)]
pub struct VisualLine {
    pub top: f32,
    pub height: f32,
    pub start: usize,
    pub end: usize,
    /// The line break (or the end of the text) follows `end`, rather than a soft wrap.
    pub ends_paragraph: bool,
    pub clusters: Vec<Cluster>,
}

#[derive(Clone, PartialEq, Debug)]
pub struct FieldGeometry {
    shown: Shaped,
    lines: Vec<VisualLine>,
    /// Value grapheme boundaries and the length: every offset handed out is one of these.
    boundaries: Vec<usize>,
    scroll_x: f32,
    caret: Rect,
}

impl FieldGeometry {
    /// Read the shaped `buffer` of `shown`. `view_width` is the room the text has; a single-line field scrolls
    /// horizontally just enough to keep its caret inside it.
    pub fn build(buffer: &Buffer, shown: Shaped, view: &FieldView, view_width: f32) -> Self {
        let paragraph_starts: Vec<usize> = std::iter::once(0)
            .chain(shown.text.match_indices('\n').map(|(at, _)| at + 1))
            .collect();
        let runs: Vec<_> = buffer.layout_runs().collect();
        let lines = runs
            .iter()
            .enumerate()
            .map(|(i, run)| {
                let base = paragraph_starts[run.line_i.min(paragraph_starts.len() - 1)];
                let clusters: Vec<Cluster> = run
                    .glyphs
                    .iter()
                    .map(|g| Cluster {
                        start: base + g.start,
                        end: base + g.end,
                        x: g.x,
                        w: g.w,
                        rtl: g.level.is_rtl(),
                    })
                    .collect();
                VisualLine {
                    top: run.line_top,
                    height: run.line_height,
                    start: clusters.iter().map(|c| c.start).min().unwrap_or(base),
                    end: clusters.iter().map(|c| c.end).max().unwrap_or(base),
                    ends_paragraph: runs.get(i + 1).is_none_or(|n| n.line_i != run.line_i),
                    clusters,
                }
            })
            .collect();
        Self::from_lines(shown, lines, view, view_width)
    }

    /// Geometry over lines that were not read from a buffer (tests, and the empty buffer).
    pub fn from_lines(
        shown: Shaped,
        mut lines: Vec<VisualLine>,
        view: &FieldView,
        view_width: f32,
    ) -> Self {
        if lines.is_empty() {
            lines.push(VisualLine {
                top: 0.0,
                height: 0.0,
                start: 0,
                end: 0,
                ends_paragraph: true,
                clusters: Vec::new(),
            });
        }
        let mut geometry = Self {
            boundaries: grapheme_starts(&view.text),
            shown,
            lines,
            scroll_x: 0.0,
            caret: Rect::ZERO,
        };
        let caret_at = match view.composition {
            Composition::Idle => geometry.shown.to_shaped(view.selection.head),
            Composition::Preedit(_) => geometry.shown.composition().end,
        };
        let (line, x) = geometry.place(caret_at, view.affinity);
        geometry.scroll_x = match view.mode {
            Mode::SingleLine => (x + CARET_ROOM - view_width).max(0.0),
            Mode::MultiLine => 0.0,
        };
        geometry.caret = geometry.bar(line, x);
        geometry
    }

    /// Where the text is scrolled to, logical px.
    pub fn scroll_x(&self) -> f32 {
        self.scroll_x
    }
    /// The caret as drawn: a zero-width, line-tall rect.
    pub fn caret(&self) -> Rect {
        self.caret
    }

    /// The value offset under `p` (clamped to the text), with the side of a soft wrap it was on.
    pub fn hit(&self, p: Point) -> Caret {
        let line = self
            .lines
            .iter()
            .rposition(|l| l.top <= p.y as f32)
            .unwrap_or(0);
        let line = &self.lines[line];
        let x = p.x as f32 + self.scroll_x;
        let shaped = match line.clusters.iter().find(|c| x < c.x + c.w) {
            None => line.clusters.last().map_or(line.start, |c| c.trailing().0),
            Some(c) if x < c.x + c.w / 2.0 => c.leading().0,
            Some(c) => c.trailing().0,
        };
        let affinity = match shaped == line.end && !line.ends_paragraph {
            true => Affinity::Before,
            false => Affinity::After,
        };
        Caret {
            offset: self.value_at(shaped),
            affinity,
        }
    }

    /// The caret for `at`, as a rect.
    pub fn caret_at(&self, at: Caret) -> Rect {
        let (line, x) = self.place(self.shown.to_shaped(at.offset), at.affinity);
        self.bar(line, x)
    }

    /// Up or down one visual line, keeping the column in `goal` (or the current one). At the first line Up goes to
    /// the start of the text, at the last Down to its end.
    pub fn vertical(&self, from: Caret, goal: Goal, dir: Vertical) -> (Caret, Goal) {
        let (line, x) = self.place(self.shown.to_shaped(from.offset), from.affinity);
        let column = match goal {
            Goal::Column(c) => c,
            Goal::Free => x,
        };
        let target = match dir {
            Vertical::Up => line.checked_sub(1),
            Vertical::Down => Some(line + 1).filter(|l| *l < self.lines.len()),
        };
        match (target, dir) {
            (Some(t), _) => {
                let line = &self.lines[t];
                let at = Point::new(
                    f64::from(column - self.scroll_x),
                    f64::from(line.top + line.height / 2.0),
                );
                (self.hit(at), Goal::Column(column))
            }
            (None, Vertical::Up) => (self.edge_of_text(0), Goal::Free),
            (None, Vertical::Down) => (self.edge_of_text(self.boundaries.len() - 1), Goal::Free),
        }
    }

    /// Start or end of the visual line holding `from` (Home / End).
    pub fn line_edge(&self, from: Caret, edge: LineEdge) -> Caret {
        let (line, _) = self.place(self.shown.to_shaped(from.offset), from.affinity);
        let line = &self.lines[line];
        match edge {
            LineEdge::Start => Caret {
                offset: self.value_at(line.start),
                affinity: Affinity::After,
            },
            LineEdge::End => Caret {
                offset: self.value_at(line.end),
                affinity: match line.ends_paragraph {
                    true => Affinity::After,
                    false => Affinity::Before,
                },
            },
        }
    }

    /// Highlight rects of a value range, one per visual line (and a stub where the range includes a line break).
    pub fn selection_rects(&self, range: Range<usize>) -> Vec<Rect> {
        self.rects(self.shown.to_shaped(range.start)..self.shown.to_shaped(range.end))
    }

    /// Where the composition is drawn, for its underline.
    pub fn composition_rects(&self) -> Vec<Rect> {
        self.rects(self.shown.composition())
    }

    fn rects(&self, range: Range<usize>) -> Vec<Rect> {
        let mut out = Vec::new();
        for line in &self.lines {
            let mut spans: Vec<(f32, f32)> = line
                .clusters
                .iter()
                .filter(|c| c.start < range.end && c.end > range.start)
                .map(|c| (c.x, c.x + c.w))
                .collect();
            spans.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut merged: Vec<(f32, f32)> = Vec::new();
            for (x0, x1) in spans {
                match merged.last_mut() {
                    Some(last) if x0 <= last.1 + 0.5 => last.1 = last.1.max(x1),
                    Some(_) | None => merged.push((x0, x1)),
                }
            }
            let breaks_here = line.ends_paragraph
                && line.end >= range.start
                && line.end < range.end
                && line.end < self.shown.text.len();
            if breaks_here {
                let edge = line.clusters.iter().map(|c| c.x + c.w).fold(0.0, f32::max);
                merged.push((edge, edge + line.height * LINE_BREAK_STUB));
            }
            out.extend(merged.into_iter().map(|(x0, x1)| {
                Rect::new(
                    f64::from(x0 - self.scroll_x),
                    f64::from(line.top),
                    f64::from(x1 - self.scroll_x),
                    f64::from(line.top + line.height),
                )
            }));
        }
        out
    }

    /// Text-space x of shaped offset `at` and the line it is on. A boundary shared by two soft-wrapped lines belongs
    /// to the first for `Before` and to the second for `After`.
    fn place(&self, at: usize, affinity: Affinity) -> (usize, f32) {
        let line = self
            .lines
            .iter()
            .position(|l| {
                l.start <= at
                    && (at < l.end
                        || (at == l.end && (l.ends_paragraph || affinity == Affinity::Before)))
            })
            .unwrap_or(self.lines.len() - 1);
        let clusters = &self.lines[line].clusters;
        let after = clusters
            .iter()
            .find(|c| c.start == at)
            .map(|c| c.leading().1);
        let before = clusters
            .iter()
            .find(|c| c.end == at)
            .map(|c| c.trailing().1);
        let inside = || {
            clusters
                .iter()
                .find(|c| c.start < at && at < c.end)
                .map(|c| {
                    let along = (at - c.start) as f32 / (c.end - c.start) as f32;
                    c.x + c.w * if c.rtl { 1.0 - along } else { along }
                })
        };
        let x = match affinity {
            Affinity::After => after.or(before),
            Affinity::Before => before.or(after),
        };
        (line, x.or_else(inside).unwrap_or(0.0))
    }

    fn bar(&self, line: usize, x: f32) -> Rect {
        let line = &self.lines[line];
        let x = f64::from(x - self.scroll_x);
        Rect::new(x, f64::from(line.top), x, f64::from(line.top + line.height))
    }

    fn edge_of_text(&self, boundary: usize) -> Caret {
        Caret {
            offset: self.boundaries[boundary],
            affinity: Affinity::After,
        }
    }

    /// Value offset of a shaped offset, snapped back to a grapheme boundary: a shaper may cluster finer than
    /// graphemes (a combining mark that did not compose), and a caret must never sit inside one.
    fn value_at(&self, shaped: usize) -> usize {
        let value = self.shown.to_value(shaped);
        let below = self.boundaries.partition_point(|b| *b <= value);
        self.boundaries[below.saturating_sub(1)]
    }
}

/// The caret keeps this much room to the right of the visible text.
const CARET_ROOM: f32 = 2.0;
/// Width of the stub drawn for a selected line break, in line heights.
const LINE_BREAK_STUB: f32 = 0.3;
