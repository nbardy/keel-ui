//! Portal paint order and the focus ring.
use kit_core::kurbo::{Rect, Size, Vec2};
use kit_core::peniko::{Brush, Color};
use kit_core::{ColorVision, MotionPref, Theme, Time, Viewport, WidgetId};
use kit_input::{Router, UiMemory};
use kit_paint::*;
use kit_view::testing::laid_out;
use kit_view::*;

fn theme() -> Theme {
    Theme::new(ColorVision::Normal, MotionPref::Full)
}
fn memory() -> UiMemory {
    UiMemory::new(Viewport::new(Size::new(1000.0, 1000.0), 1.0))
}
fn mark(red: u8) -> Node<()> {
    kit_view::rect(
        Style::column(),
        Brush::Solid(Color::from_rgba8(red, 0, 0, 255)),
    )
}
fn anchor() -> PortalAnchor {
    PortalAnchor::Viewport(Anchor {
        h: AnchorH::Start,
        v: AnchorV::Start,
        offset: Vec2::ZERO,
        safe_area: SafeArea::Respect,
    })
}

/// Reds of the solid fills, in draw order (the scrim is black with alpha, so it reads 0).
fn fill_reds(list: &DrawList) -> Vec<u8> {
    list.items
        .iter()
        .filter_map(|d| match d {
            Draw::Fill {
                brush: Brush::Solid(c),
                ..
            } => Some((c.to_rgba8().r, c.to_rgba8().a)),
            _ => None,
        })
        .map(|(r, a)| if a == 255 { r } else { 0 })
        .collect()
}

/// Layers paint Popup, Modal (scrim first), Toast, Tooltip over the base whatever order they are written in. Written
/// tooltip-first below, so a painter that kept tree order draws the tooltip under the base's later sibling.
#[test]
fn portals_paint_above_the_tree_in_layer_order() {
    let tree: Node<()> = column(vec![
        portal(Layer::Tooltip, anchor(), mark(50)),
        portal(Layer::Popup, anchor(), mark(20)),
        portal(Layer::Modal, anchor(), mark(30)),
        portal(Layer::Toast, anchor(), mark(40)),
        mark(10),
    ]);
    let r = Rect::new(0.0, 0.0, 100.0, 100.0);
    let n = tree.count();
    let laid = laid_out(tree, &vec![r; n]);
    let list = paint(&laid, &memory(), &theme(), Time::default());
    // base 10, popup 20, scrim (0), modal 30, toast 40, tooltip 50.
    assert_eq!(fill_reds(&list), [10, 20, 0, 30, 40, 50]);
    assert!(list.layers_balanced());
}

/// Keyboard focus on a control is visible: a ring is stroked in the theme's focus colour, and only while focused.
#[test]
fn a_focused_control_gets_the_focus_ring() {
    let tree: Node<()> = column(vec![interactive(
        WidgetId::key("go"),
        Bindings::activate(()),
        A11y::Hidden,
        mark(10),
    )]);
    let r = Rect::new(10.0, 10.0, 90.0, 50.0);
    let n = tree.count();
    let laid = laid_out(tree, &vec![r; n]);
    let rings = |mem: &UiMemory| {
        paint(&laid, mem, &theme(), Time::default())
            .items
            .iter()
            .filter(|d| {
                matches!(d, Draw::Stroke { brush: Brush::Solid(c), .. } if *c == theme().colors.focus_ring)
            })
            .count()
    };
    let mut mem = memory();
    assert_eq!(rings(&mem), 0);
    Router::default().request_focus(&laid, &mut mem, WidgetId::key("go"), Time::default());
    assert_eq!(rings(&mem), 1);
}
