//! track: pointer bindings (`Bindings::on_pointer`). A press on a node that binds positions captures that pointer;
//! the press, every move, the release and any cancel are reported to the node relative to its laid rect, wherever
//! the pointer is.
use crate::hit::{HitPath, Region, bindings_of};
use crate::router::end_gesture;
use crate::{Gesture, PointerId, UiMemory};
use kit_core::kurbo::Point;
use kit_core::{Epoch, WidgetId};
use kit_view::{LaidOut, PointerAt, PointerPhase, Travel};

/// The track a press begins: `Some` when the innermost press-capturing region binds positions.
pub(crate) fn begin<Msg>(
    path: &HitPath<'_, Msg>,
    at: Point,
    epoch: Epoch,
) -> Option<(WidgetId, Gesture)> {
    let (source, on, local) = path.regions.iter().find_map(|r| match r {
        Region::Bound { id, on, at } if on.captures_press() => Some((*id, *on, *at)),
        Region::Bound { .. } | Region::Field { .. } => None,
    })?;
    on.pointer.as_ref().map(|_| {
        let track = Gesture::Track {
            source,
            shift: at - local,
            origin: at,
            travel: Travel::Still,
            epoch,
        };
        (source, track)
    })
}

/// Tell the tracking node where `pointer` is now. Nothing to tell when the pointer is not tracking, or when the node
/// it tracks is no longer in `laid` (the component is gone, and its keyed state with it).
pub(crate) fn report<Msg>(
    laid: &LaidOut<Msg>,
    memory: &UiMemory,
    pointer: PointerId,
    phase: PointerPhase,
    out: &mut Vec<Msg>,
) {
    let Some(state) = memory.pointers.get(&pointer) else {
        return;
    };
    let Gesture::Track {
        source,
        shift,
        travel,
        ..
    } = state.gesture
    else {
        return;
    };
    let Some((node, spec)) = laid
        .find(source)
        .and_then(|node| Some((node, bindings_of(node)?.pointer.as_ref()?)))
    else {
        return;
    };
    out.push((spec.on)(PointerAt {
        phase,
        local: (state.at - shift) - node.rect.origin().to_vec2(),
        size: node.rect.size(),
        travel,
    }));
}

/// A tracked pointer moved: it has travelled once it leaves the slop circle (and stays travelled).
pub(crate) fn moved(memory: &mut UiMemory, pointer: PointerId, slop: f64) {
    let Some(state) = memory.pointers.get_mut(&pointer) else {
        return;
    };
    if let Gesture::Track { origin, travel, .. } = &mut state.gesture
        && (state.at - *origin).hypot2() >= slop * slop
    {
        *travel = Travel::Moved;
    }
}

/// End `pointer`'s gesture without a release; a tracking node hears `Cancel` first.
pub(crate) fn abandon<Msg>(
    laid: &LaidOut<Msg>,
    memory: &mut UiMemory,
    pointer: PointerId,
    out: &mut Vec<Msg>,
) {
    report(laid, memory, pointer, PointerPhase::Cancel, out);
    end_gesture(memory, pointer);
}

/// Escape lets go of every tracked pointer. True when there was one (the key is then taken).
pub(crate) fn escape<Msg>(laid: &LaidOut<Msg>, memory: &mut UiMemory, out: &mut Vec<Msg>) -> bool {
    let tracking: Vec<PointerId> = memory
        .pointers
        .iter()
        .filter(|(_, s)| matches!(s.gesture, Gesture::Track { .. }))
        .map(|(id, _)| *id)
        .collect();
    for &pointer in &tracking {
        abandon(laid, memory, pointer, out);
    }
    !tracking.is_empty()
}
