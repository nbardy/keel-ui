//! Portal placement. The engine lays every portal out as its own root at the origin (it needs the portal's size, not its
//! place); this runs on the assembled tree, where the anchor node's rect exists, and moves each portal to its anchor.
use crate::place_anchored;
use kit_core::WidgetId;
use kit_core::kurbo::{Insets, Rect};
use kit_view::{LaidKind, LaidNode, LaidOut, LayoutError, PortalAnchor, place_beside};
use std::collections::BTreeMap;

/// Rects of every identified node, kept current as portals move: a portal may anchor to a node inside another portal.
type Rects = BTreeMap<WidgetId, Rect>;

pub(crate) fn settle<Msg>(laid: &mut LaidOut<Msg>) -> Result<(), LayoutError> {
    let screen = Rect::from_origin_size((0.0, 0.0), laid.viewport.size);
    let safe = laid.viewport.safe_area;
    let mut rects = Rects::new();
    remember(&laid.root, &mut rects);
    place(&mut laid.root, screen, safe, &mut rects)
}

fn remember<Msg>(node: &LaidNode<Msg>, rects: &mut Rects) {
    for n in node.descendants() {
        if let Some(id) = n.id() {
            rects.insert(id, n.rect);
        }
    }
}

/// Outer portals move first, so an inner portal anchored inside one finds the anchor where it ended up.
fn place<Msg>(
    node: &mut LaidNode<Msg>,
    screen: Rect,
    safe: Insets,
    rects: &mut Rects,
) -> Result<(), LayoutError> {
    if let LaidKind::Portal { anchor, .. } = &node.kind {
        let target = target_rect(anchor, node.rect, screen, safe, rects)?;
        node.translate(target.origin() - node.rect.origin());
        remember(node, rects);
    }
    node.children_mut()
        .iter_mut()
        .try_for_each(|child| place(child, screen, safe, rects))
}

fn target_rect(
    anchor: &PortalAnchor,
    laid: Rect,
    screen: Rect,
    safe: Insets,
    rects: &Rects,
) -> Result<Rect, LayoutError> {
    match anchor {
        PortalAnchor::Viewport(anchor) => Ok(place_anchored(
            screen,
            safe,
            laid.size(),
            Insets::ZERO,
            anchor,
        )),
        PortalAnchor::Node { id, side, gap } => {
            let base = rects
                .get(id)
                .ok_or(LayoutError::PortalAnchorMissing { id: *id })?;
            Ok(place_beside(*base, laid.size(), screen - safe, *side, *gap))
        }
    }
}
