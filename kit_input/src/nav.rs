//! Spatial navigation: `NavAction::Move` picks the nearest focusable control in a direction from laid-out rects.
//!
//! The rule (egui's and the web's spatial navigation are the same idea):
//! 1. Candidates are the focusable controls of the topmost scope (a modal's subtree alone while one is open), each
//!    at its on-screen rect: the laid rect less the offsets of the scroll areas around it, so a row scrolled out of
//!    view still has a position, and moving to it scrolls it in (`scroll::reveal_focused`). Disabled controls and
//!    nodes with nothing to do on a press are not focusable, so they are not candidates.
//! 2. A candidate counts only if its centre is strictly past the focused control's centre in the direction moved.
//! 3. Its score is `major + CROSS_WEIGHT * minor`: `major` is the gap between the facing edges along the direction
//!    (zero where the rects overlap on that axis), `minor` is how far the centres are apart across it. The lowest
//!    score wins; a tie goes to the control earlier in Tab order. The cross weight above one makes a control straight
//!    ahead beat a nearer one off to the side, which is what makes a grid walk by rows and columns.
//! 4. No candidate means the player is at an edge: focus stays (nothing wraps; Next/Previous wrap, as Tab does).
//! 5. From nowhere (nothing focused, or focus on something that left the screen) Down and Right take the first
//!    control in Tab order and Up and Left the last: the same entry points as Tab and Shift-Tab.
use crate::focus::{self, Focused};
use crate::router::Claim;
use crate::{Focus, NavAction, NavDirection, Router, UiMemory};
use kit_core::kurbo::{Rect, Vec2};
use kit_core::{Time, WidgetId};
use kit_view::{LaidKind, LaidNode, LaidOut};

/// How much a centre's offset across the direction costs, against the gap along it. See rule 3.
const CROSS_WEIGHT: f64 = 2.0;

#[derive(Copy, Clone, Debug)]
pub(crate) struct Target {
    pub id: WidgetId,
    pub rect: Rect,
}

/// What a directional move came to.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub(crate) enum Step {
    /// Nothing in scope can take focus: the world may have the event.
    Nowhere,
    /// The focused control is at an edge in that direction.
    Stay,
    To(WidgetId),
}

/// The scope's focusable controls in Tab order with their on-screen rects.
pub(crate) fn targets<Msg>(laid: &LaidOut<Msg>, memory: &UiMemory) -> Vec<Target> {
    let wanted = focus::order(laid);
    let mut out = Vec::new();
    visit(focus::scope(laid), Vec2::ZERO, memory, &wanted, &mut out);
    out
}

/// `shift` is the scroll offsets of the areas around `node`; a portal is placed against the viewport, not its parent's
/// content, so it starts again from zero (as hit-testing does).
fn visit<Msg>(
    node: &LaidNode<Msg>,
    shift: Vec2,
    memory: &UiMemory,
    wanted: &[WidgetId],
    out: &mut Vec<Target>,
) {
    let inner = match &node.kind {
        LaidKind::Scroll { id, .. } => shift + memory.scroll_offset(*id),
        LaidKind::Portal { .. } => Vec2::ZERO,
        LaidKind::TextField { id, .. } | LaidKind::Interactive { id, .. } => {
            if wanted.contains(id) {
                out.push(Target {
                    id: *id,
                    rect: node.rect - shift,
                });
            }
            shift
        }
        LaidKind::Stack { .. }
        | LaidKind::Panel { .. }
        | LaidKind::Text { .. }
        | LaidKind::Image { .. }
        | LaidKind::Shape { .. }
        | LaidKind::Layer { .. }
        | LaidKind::World { .. } => shift,
    };
    for child in node.children() {
        visit(child, inner, memory, wanted, out);
    }
}

/// A span on one axis, oriented so that "ahead" is increasing.
#[derive(Copy, Clone)]
struct Span {
    start: f64,
    end: f64,
}
impl Span {
    fn mid(self) -> f64 {
        (self.start + self.end) / 2.0
    }
}

/// `(along, across)` of `rect` for a move in `direction`.
fn oriented(rect: Rect, direction: NavDirection) -> (Span, Span) {
    let x = Span {
        start: rect.x0,
        end: rect.x1,
    };
    let y = Span {
        start: rect.y0,
        end: rect.y1,
    };
    let flipped = |s: Span| Span {
        start: -s.end,
        end: -s.start,
    };
    match direction {
        NavDirection::Right => (x, y),
        NavDirection::Left => (flipped(x), y),
        NavDirection::Down => (y, x),
        NavDirection::Up => (flipped(y), x),
    }
}

/// Rules 2 and 3: `None` when `to` is not ahead of `from`.
fn score(from: Rect, to: Rect, direction: NavDirection) -> Option<f64> {
    let ((from_along, from_across), (to_along, to_across)) =
        (oriented(from, direction), oriented(to, direction));
    if to_along.mid() <= from_along.mid() {
        return None;
    }
    let major = (to_along.start - from_along.end).max(0.0);
    let minor = (to_across.mid() - from_across.mid()).abs();
    Some(major + CROSS_WEIGHT * minor)
}

pub(crate) fn step(targets: &[Target], from: Focus, direction: NavDirection) -> Step {
    let source = match from {
        Focus::On(id) => targets.iter().find(|t| t.id == id),
        Focus::Unfocused => None,
    };
    match (source, targets.first(), targets.last()) {
        (_, None, _) | (_, _, None) => Step::Nowhere,
        (Some(source), Some(_), Some(_)) => targets
            .iter()
            .filter(|t| t.id != source.id)
            .filter_map(|t| score(source.rect, t.rect, direction).map(|s| (s, t.id)))
            // `min_by` keeps the first of equal scores: the earlier control in Tab order.
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map_or(Step::Stay, |(_, id)| Step::To(id)),
        (None, Some(first), Some(last)) => match direction {
            NavDirection::Down | NavDirection::Right => Step::To(first.id),
            NavDirection::Up | NavDirection::Left => Step::To(last.id),
        },
    }
}

impl Router {
    /// One navigation action. `Move` is spatial; the others are what Enter, Escape, Tab and Shift-Tab do.
    pub(crate) fn nav<Msg: Clone>(
        &self,
        laid: &LaidOut<Msg>,
        memory: &mut UiMemory,
        action: NavAction,
        now: Time,
        out: &mut Vec<Msg>,
    ) -> Claim {
        match action {
            NavAction::Move(direction) => {
                match step(&targets(laid, memory), memory.focus, direction) {
                    Step::Nowhere => Claim::Free,
                    Step::Stay => Claim::Taken,
                    Step::To(id) => {
                        focus::move_focus(laid, memory, id, now);
                        Claim::Taken
                    }
                }
            }
            NavAction::Activate => match focus::classify(laid, memory) {
                Focused::Control(id) => self.activate_focused(laid, id, memory.mods, out),
                Focused::Field(_) | Focused::Nothing => Claim::Free,
            },
            NavAction::Cancel => {
                if crate::track::escape(laid, memory, out) {
                    Claim::Taken
                } else {
                    self.escape(laid, memory, out)
                }
            }
            NavAction::Next => self.tab(laid, memory, false, now),
            NavAction::Previous => self.tab(laid, memory, true, now),
        }
    }
}
