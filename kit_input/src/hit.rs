//! Hit-testing over a `LaidOut` tree, honouring scroll offsets (from `UiMemory`), clips, disabled subtrees and culling.
use crate::UiMemory;
use crate::scroll::Area;
use kit_core::WidgetId;
use kit_core::kurbo::Point;
use kit_view::{
    Availability, Bindings, ClipSpec, LaidKind, LaidNode, LaidOut, ModalFloor, Shown, Stacking,
};

pub(crate) enum Region<'a, Msg> {
    /// `at`: the point in this node's layout space (scroll offsets above it applied), for pointer bindings.
    Bound {
        id: WidgetId,
        on: &'a Bindings<Msg>,
        at: Point,
    },
    Field {
        id: WidgetId,
    },
}
/// Everything under a point, innermost first.
pub(crate) struct HitPath<'a, Msg> {
    pub regions: Vec<Region<'a, Msg>>,
    pub scrolls: Vec<Area>,
}

pub(crate) fn hit_test<'a, Msg>(
    laid: &'a LaidOut<Msg>,
    memory: &UiMemory,
    point: Point,
) -> HitPath<'a, Msg> {
    let mut path = HitPath {
        regions: Vec::new(),
        scrolls: Vec::new(),
    };
    // Layers first, topmost down. An opaque portal stops the point inside its rect even over empty content; a modal
    // stops everything below it (the normal tree and lower portals are not reachable while one is open).
    let stacking = Stacking::of(laid);
    for index in stacking.reachable() {
        let portal = &stacking.portals[index];
        if visit(portal.child, memory, point, &mut path) {
            return path;
        }
        if portal.layer.is_opaque() && portal.child.rect.contains(point) {
            return path;
        }
    }
    match stacking.modal() {
        ModalFloor::At(_) => {}
        ModalFloor::None => {
            visit(&laid.root, memory, point, &mut path);
        }
    }
    path
}

pub(crate) fn bindings_of<Msg>(node: &LaidNode<Msg>) -> Option<&Bindings<Msg>> {
    match &node.kind {
        LaidKind::Interactive { on, .. } => Some(on),
        LaidKind::Stack { .. }
        | LaidKind::Panel { .. }
        | LaidKind::Text { .. }
        | LaidKind::Shape { .. }
        | LaidKind::Scroll { .. }
        | LaidKind::TextField { .. }
        | LaidKind::Layer { .. }
        | LaidKind::World { .. }
        | LaidKind::Portal { .. } => None,
    }
}

/// Returns true when this subtree consumes the point (topmost wins; later siblings paint above earlier ones).
fn visit<'a, Msg>(
    node: &'a LaidNode<Msg>,
    memory: &UiMemory,
    point: Point,
    path: &mut HitPath<'a, Msg>,
) -> bool {
    match &node.kind {
        LaidKind::Stack { children } | LaidKind::Panel { children, .. } => {
            children.iter().rev().any(|c| visit(c, memory, point, path))
        }
        // Portals are reached through `Stacking` (layer order), never through the tree that holds them.
        LaidKind::Text { .. } | LaidKind::Shape { .. } | LaidKind::Portal { .. } => false,
        LaidKind::Interactive {
            id,
            on,
            availability,
            child,
            ..
        } => match availability {
            Availability::Disabled => false,
            Availability::Enabled => {
                let below = visit(child, memory, point, path);
                if node.rect.contains(point) {
                    path.regions.push(Region::Bound {
                        id: *id,
                        on,
                        at: point,
                    });
                    true
                } else {
                    below
                }
            }
        },
        LaidKind::Scroll { id, child, .. } => {
            if !node.rect.contains(point) {
                return false;
            }
            let inner = point + memory.scroll_offset(*id);
            visit(child, memory, inner, path);
            if let Some(scroll) = Area::of(node) {
                path.scrolls.push(scroll);
            }
            true
        }
        LaidKind::TextField { id, .. } => {
            if node.rect.contains(point) {
                path.regions.push(Region::Field { id: *id });
                true
            } else {
                false
            }
        }
        LaidKind::Layer { clip, child, .. } => match clip {
            ClipSpec::Unclipped => visit(child, memory, point, path),
            ClipSpec::Bounds => node.rect.contains(point) && visit(child, memory, point, path),
        },
        LaidKind::World { shown, child, .. } => match shown {
            Shown::Culled => false,
            Shown::Visible => visit(child, memory, point, path),
        },
    }
}
