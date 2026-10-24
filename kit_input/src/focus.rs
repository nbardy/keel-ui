//! Keyboard focus: what has it, what Tab visits, and the scope Tab stays inside.
//!
//! The order is tree order (pre-order) among the focusable nodes of the topmost scope. The scope is the normal tree
//! (portals included, at the place they are written) until a modal is open; then it is that modal's subtree alone, so
//! Tab can neither leave a dialog nor reach the screen behind it. Focusable means a text field, or an enabled control
//! with something to do on a press; disabled subtrees and culled world boxes are skipped, as they are for hit-testing.
use crate::hit::bindings_of;
use crate::router::Claim;
use crate::{AnimKey, Focus, Key, Modifiers, Router, UiMemory, layers};
use kit_core::{Time, WidgetId};
use kit_view::{
    Availability, Edit, FocusClaim, KeyAnswer, KeyPress, LaidKind, LaidNode, LaidOut, ModalFloor,
    Stacking,
};
use std::hash::Hash;

/// What focus is on, told by the laid tree: the id alone does not say whether keys are text.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub(crate) enum Focused {
    Nothing,
    Field(WidgetId),
    Control(WidgetId),
}

pub(crate) fn classify<Msg>(laid: &LaidOut<Msg>, memory: &UiMemory) -> Focused {
    match memory.focus {
        Focus::Unfocused => Focused::Nothing,
        Focus::On(id) => match laid.find(id).map(|node| &node.kind) {
            Some(LaidKind::TextField { .. }) => Focused::Field(id),
            Some(LaidKind::Interactive { .. }) => Focused::Control(id),
            Some(
                LaidKind::Stack { .. }
                | LaidKind::Panel { .. }
                | LaidKind::Text { .. }
                | LaidKind::Shape { .. }
                | LaidKind::Scroll { .. }
                | LaidKind::Layer { .. }
                | LaidKind::World { .. }
                | LaidKind::Portal { .. },
            )
            | None => Focused::Nothing,
        },
    }
}

/// The subtree keys and Tab act on: the topmost modal's content, else the whole screen.
pub(crate) fn scope<Msg>(laid: &LaidOut<Msg>) -> &LaidNode<Msg> {
    let stacking = Stacking::of(laid);
    match stacking.modal() {
        ModalFloor::At(index) => stacking.portals[index].child,
        ModalFloor::None => &laid.root,
    }
}

/// Tab order of the current scope.
pub(crate) fn order<Msg>(laid: &LaidOut<Msg>) -> Vec<WidgetId> {
    focusable(laid).into_iter().map(|(id, _)| id).collect()
}

/// The scope's focusable nodes in Tab order, with the focus claim each carries.
fn focusable<Msg>(laid: &LaidOut<Msg>) -> Vec<(WidgetId, FocusClaim)> {
    let mut found = Vec::new();
    collect(scope(laid), &mut found);
    found
}

fn collect<Msg>(node: &LaidNode<Msg>, out: &mut Vec<(WidgetId, FocusClaim)>) {
    match &node.kind {
        LaidKind::TextField { id, claim, .. } => out.push((*id, *claim)),
        LaidKind::Interactive {
            id,
            on,
            availability,
            child,
            ..
        } => match availability {
            Availability::Disabled => {}
            Availability::Enabled => {
                if on.captures_press() {
                    out.push((*id, on.claim));
                }
                collect(child, out);
            }
        },
        LaidKind::World {
            shown: kit_view::Shown::Culled,
            ..
        } => {}
        LaidKind::Stack { .. }
        | LaidKind::Panel { .. }
        | LaidKind::Text { .. }
        | LaidKind::Shape { .. }
        | LaidKind::Scroll { .. }
        | LaidKind::Layer { .. }
        | LaidKind::World { .. }
        | LaidKind::Portal { .. } => {
            for child in node.children() {
                collect(child, out);
            }
        }
    }
}

/// The control after (or, `reverse`, before) the focused one, wrapping. From nowhere (nothing focused, or focus outside
/// the scope) it is the first (or last). `None` only when nothing in scope is focusable.
pub(crate) fn step(order: &[WidgetId], current: Focus, reverse: bool) -> Option<WidgetId> {
    let n = order.len();
    if n == 0 {
        return None;
    }
    let at = match current {
        Focus::Unfocused => None,
        Focus::On(id) => order.iter().position(|o| *o == id),
    };
    let index = match (at, reverse) {
        (Some(i), false) => (i + 1) % n,
        (Some(i), true) => (i + n - 1) % n,
        (None, false) => 0,
        (None, true) => n - 1,
    };
    order.get(index).copied()
}

pub(crate) fn focus_on(memory: &mut UiMemory, id: WidgetId, now: Time) {
    if memory.focus != Focus::On(id) {
        memory.anim.insert((id, AnimKey::Focus), now);
    }
    memory.focus = Focus::On(id);
}

/// keys: tell text fields how focus moved since they were last told: the field that lost it hears `Edit::Blur`, the
/// one that gained it `Edit::Focus`. The router calls this around every event, so a change made anywhere (a press,
/// Tab, Escape, a claim applied by `Router::frame`, a host request) reaches the field before the next input's edits.
/// A field that is no longer in `laid` has nobody to tell (its keyed state ended with it).
pub(crate) fn tell<Msg>(laid: &LaidOut<Msg>, memory: &mut UiMemory, out: &mut Vec<Msg>) {
    let (was, now) = (memory.told, memory.focus);
    if was == now {
        return;
    }
    memory.told = now;
    for (focus, edit) in [(was, Edit::Blur), (now, Edit::Focus)] {
        if let Focus::On(id) = focus
            && let Some(LaidKind::TextField { on_edit, .. }) = laid.find(id).map(|node| &node.kind)
        {
            out.push(on_edit(edit));
        }
    }
}

/// keys: apply the scope's focus claims, each ticket once (`FocusClaim` is edge-triggered so the player can move
/// focus away afterwards). When several nodes present a new ticket in one frame the last in Tab order wins. Tickets of
/// nodes that left the scope are forgotten, so a node that comes back claims again.
pub(crate) fn claim<Msg>(laid: &LaidOut<Msg>, memory: &mut UiMemory, now: Time) {
    let claims: Vec<(WidgetId, u64)> = focusable(laid)
        .into_iter()
        .filter_map(|(id, claim)| match claim {
            FocusClaim::Nothing => None,
            FocusClaim::Ticket(ticket) => Some((id, ticket)),
        })
        .collect();
    let fresh = claims
        .iter()
        .rev()
        .find(|(id, ticket)| memory.claims.get(id) != Some(ticket))
        .map(|(id, _)| *id);
    memory.claims = claims.into_iter().collect();
    if let Some(id) = fresh {
        focus_on(memory, id, now);
    }
}

/// Focus left on something the scope no longer holds (the node left the screen, went disabled, or a modal opened
/// over it) is dropped, so keys never reach a field the player cannot see or reach.
pub(crate) fn prune<Msg>(laid: &LaidOut<Msg>, memory: &mut UiMemory) {
    if let Focus::On(id) = memory.focus
        && !order(laid).contains(&id)
    {
        memory.focus = Focus::Unfocused;
    }
}

/// keys: offer a key to the focused control's own binding (`Bindings::on_key`). Free when it binds none or passes.
pub(crate) fn key<Msg>(
    laid: &LaidOut<Msg>,
    id: WidgetId,
    key: Key,
    mods: Modifiers,
    out: &mut Vec<Msg>,
) -> Claim {
    let answer = laid
        .find(id)
        .and_then(bindings_of)
        .and_then(|on| on.key.as_ref())
        .map(|spec| (spec.on)(KeyPress { key, mods }));
    match answer {
        Some(KeyAnswer::Handled(msg)) => {
            out.push(msg);
            Claim::Taken
        }
        Some(KeyAnswer::Pass) | None => Claim::Free,
    }
}

/// What a host's request to focus a node came to.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum FocusOutcome {
    Focused,
    /// The tree holds no node with that key. Counted in `UiMemory::focus_misses`.
    Missing,
    /// The node exists but cannot take focus here: not a field or an enabled control, disabled, or behind a modal.
    /// Counted in `UiMemory::focus_misses`.
    Unfocusable,
}

/// Programmatic focus: the host asks, after layout, for focus on the node keyed `id`.
pub(crate) fn request<Msg>(
    laid: &LaidOut<Msg>,
    memory: &mut UiMemory,
    id: WidgetId,
    now: Time,
) -> FocusOutcome {
    let outcome = match laid.find(id) {
        None => FocusOutcome::Missing,
        Some(_) if order(laid).contains(&id) => FocusOutcome::Focused,
        Some(_) => FocusOutcome::Unfocusable,
    };
    match outcome {
        FocusOutcome::Focused => focus_on(memory, id, now),
        FocusOutcome::Missing | FocusOutcome::Unfocusable => memory.focus_misses += 1,
    }
    outcome
}

/// `request` for a node keyed the way components key it (`.key(&key)` is `WidgetId::of(&key)`).
pub(crate) fn request_key<Msg>(
    laid: &LaidOut<Msg>,
    memory: &mut UiMemory,
    key: &impl Hash,
    now: Time,
) -> FocusOutcome {
    request(laid, memory, WidgetId::of(key), now)
}

impl Router {
    /// The host asks, after layout, for focus on the node keyed `id`. A node that is not there, or cannot take focus
    /// here, is counted in `UiMemory::focus_misses` and reported; focus does not move.
    pub fn request_focus<Msg>(
        &self,
        laid: &LaidOut<Msg>,
        memory: &mut UiMemory,
        id: WidgetId,
        now: Time,
    ) -> FocusOutcome {
        request(laid, memory, id, now)
    }

    /// [`Router::request_focus`] for a node keyed the way components key it (`.key(&key)`).
    pub fn request_focus_key<Msg>(
        &self,
        laid: &LaidOut<Msg>,
        memory: &mut UiMemory,
        key: &impl Hash,
        now: Time,
    ) -> FocusOutcome {
        request_key(laid, memory, key, now)
    }

    /// Tab (or Shift-Tab): focus moves along the scope's order. Taken whenever something in scope can take focus.
    pub(crate) fn tab<Msg>(
        &self,
        laid: &LaidOut<Msg>,
        memory: &mut UiMemory,
        reverse: bool,
        now: Time,
    ) -> Claim {
        match step(&order(laid), memory.focus, reverse) {
            Some(id) => {
                focus_on(memory, id, now);
                Claim::Taken
            }
            None => Claim::Free,
        }
    }

    /// Escape outside a text field: the topmost dismissible portal sends its intent; failing that a focused control
    /// lets go. (A focused field handles its own Escape first.)
    pub(crate) fn escape<Msg: Clone>(
        &self,
        laid: &LaidOut<Msg>,
        memory: &mut UiMemory,
        out: &mut Vec<Msg>,
    ) -> Claim {
        if layers::escape(laid, out) {
            return Claim::Taken;
        }
        match classify(laid, memory) {
            Focused::Control(_) => {
                memory.focus = Focus::Unfocused;
                Claim::Taken
            }
            Focused::Field(_) | Focused::Nothing => Claim::Free,
        }
    }

    /// Enter or Space on the focused control sends its activate message (built from `mods` when it asks).
    pub(crate) fn activate_focused<Msg: Clone>(
        &self,
        laid: &LaidOut<Msg>,
        id: WidgetId,
        mods: Modifiers,
        out: &mut Vec<Msg>,
    ) -> Claim {
        match laid
            .find(id)
            .and_then(bindings_of)
            .and_then(|on| on.activation(mods))
        {
            Some(msg) => {
                out.push(msg);
                Claim::Taken
            }
            None => Claim::Free,
        }
    }
}
