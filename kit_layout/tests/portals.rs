mod common;

use common::*;
use kit_core::kurbo::{Rect, Size};
use kit_core::{Viewport, WidgetId};
use kit_layout::TaffyLayout;
use kit_view::taffy::{AlignItems, JustifyContent};
use kit_view::*;

fn lay(root: Node<()>) -> Result<LaidOut<()>, LayoutError> {
    let vp = Viewport::new(Size::new(200.0, 200.0), 1.0);
    TaffyLayout::new().layout(root, vp, &mut text_system(), &Plane)
}

fn portal_rect(laid: &LaidOut<()>) -> Rect {
    laid.root
        .descendants()
        .into_iter()
        .find(|n| matches!(n.kind, LaidKind::Portal { .. }))
        .expect("the tree holds a portal")
        .rect
}

/// The opener sits in the bottom-right corner of a 200 px screen.
fn corner_scene(popup: Node<()>, side: Side) -> Node<()> {
    let opener = interactive(
        WidgetId::key("opener"),
        Bindings::none(),
        A11y::Hidden,
        block(80.0, 30.0),
    );
    let anchor = PortalAnchor::Node {
        id: WidgetId::key("opener"),
        side,
        gap: 0.0,
    };
    stack(
        Style::column()
            .justify(JustifyContent::FLEX_END)
            .align_items(AlignItems::FLEX_END),
        vec![opener, portal(Layer::Popup, anchor, popup)],
    )
}

/// A menu opened "below" a control at the bottom of the screen must not hang off it: it flips above the control, and
/// its width, which would cross the right edge, is clamped back in. A placement that only added the gap put it at
/// y 200..280, entirely invisible.
#[test]
fn a_popup_that_does_not_fit_below_flips_above_and_stays_on_screen() {
    let laid = lay(corner_scene(block(100.0, 80.0), Side::Below)).expect("laid");
    let r = portal_rect(&laid);
    assert_eq!(
        (r.y0, r.y1),
        (90.0, 170.0),
        "flipped above the opener (y 170..200)"
    );
    assert_eq!((r.x0, r.x1), (100.0, 200.0), "clamped off the right edge");
}

/// When neither side has room (taller than either half of the screen), it is clamped into the viewport instead of
/// being left partly off it.
#[test]
fn a_popup_with_no_room_on_either_side_is_clamped_into_the_viewport() {
    let laid = lay(corner_scene(block(100.0, 190.0), Side::Below)).expect("laid");
    let r = portal_rect(&laid);
    assert!(r.y0 >= 0.0 && r.y1 <= 200.0, "inside the screen: {r:?}");
}

/// A portal naming a node the tree does not hold has nothing to be placed against: a typed error, not a guess.
#[test]
fn a_portal_anchored_to_a_missing_node_is_a_layout_error() {
    let anchor = PortalAnchor::Node {
        id: WidgetId::key("gone"),
        side: Side::Below,
        gap: 0.0,
    };
    let tree = column(vec![portal(Layer::Popup, anchor, block(10.0, 10.0))]);
    assert!(matches!(
        lay(tree),
        Err(LayoutError::PortalAnchorMissing { id }) if id == WidgetId::key("gone")
    ));
}
