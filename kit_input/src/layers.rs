//! Portal behaviour in the router: dismissal and what a modal takes from the world.
use crate::{InputEvent, Phase};
use kit_core::kurbo::Point;
use kit_view::{Dismissal, LaidOut, ModalFloor, Stacking};

/// What a pointer press means for the layers it landed on.
pub(crate) enum Press {
    /// Carry on: hit-test the layers and start whatever gesture is under the point.
    Proceeds,
    /// A portal was dismissed by a press that landed on no opaque portal, so it was a click on whatever lies behind,
    /// and that is not allowed to reach it: the press ends here.
    Consumed,
}

/// Click-outside: each reachable dismissible portal that does not contain `point` sends its intent, once per press.
pub(crate) fn press_outside<Msg: Clone>(
    laid: &LaidOut<Msg>,
    point: Point,
    out: &mut Vec<Msg>,
) -> Press {
    let stacking = Stacking::of(laid);
    let mut dismissed = false;
    for index in stacking.reachable() {
        match &stacking.portals[index].dismissal {
            Dismissal::Never => {}
            Dismissal::Intent(msg) => {
                if !stacking.inside(index, point) {
                    out.push(msg.clone());
                    dismissed = true;
                }
            }
        }
    }
    if dismissed && !stacking.opaque_at(point) {
        Press::Consumed
    } else {
        Press::Proceeds
    }
}

/// Escape: the topmost reachable dismissible portal sends its intent. True when one did.
pub(crate) fn escape<Msg: Clone>(laid: &LaidOut<Msg>, out: &mut Vec<Msg>) -> bool {
    let stacking = Stacking::of(laid);
    let intent = stacking.reachable().into_iter().find_map(|index| {
        match &stacking.portals[index].dismissal {
            Dismissal::Never => None,
            Dismissal::Intent(msg) => Some(msg.clone()),
        }
    });
    match intent {
        Some(msg) => {
            out.push(msg);
            true
        }
        None => false,
    }
}

pub(crate) fn modal_open<Msg>(laid: &LaidOut<Msg>) -> bool {
    match Stacking::of(laid).modal() {
        ModalFloor::At(_) => true,
        ModalFloor::None => false,
    }
}

/// Whether the world behind the UI would otherwise see this event. A modal takes all of them; the rest (window and
/// viewport bookkeeping, a pointer leaving) are not input to the world at all.
pub(crate) fn reaches_world(event: &InputEvent) -> bool {
    match event {
        InputEvent::Pointer(p) => match p.phase {
            Phase::Down | Phase::Move | Phase::Up | Phase::Cancel => true,
            Phase::Leave => false,
        },
        InputEvent::Wheel { .. }
        | InputEvent::Key { .. }
        | InputEvent::Text(_)
        | InputEvent::Ime(_)
        | InputEvent::Paste { .. } => true,
        InputEvent::Focus(_)
        | InputEvent::Modifiers(_)
        | InputEvent::Resize(_)
        | InputEvent::ScaleFactor(_)
        | InputEvent::SafeArea(_) => false,
    }
}
