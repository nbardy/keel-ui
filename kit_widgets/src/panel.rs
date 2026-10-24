//! Containers: `panel` and `inset`.
use crate::common::surface;
use kit_core::kurbo::Vec2;
use kit_core::{Border, Shadow, Theme};
use kit_view::panel as panel_node;
use kit_view::{Node, Style};

/// Raised surface with a 1px border and a soft drop shadow (the HUD's `panel`). `style` decides size and content flow.
pub fn panel<Msg>(theme: &Theme, style: Style, children: Vec<Node<Msg>>) -> Node<Msg> {
    let c = &theme.colors;
    let shadow = Shadow::Drop {
        offset: Vec2::new(0.0, 2.0),
        blur: 6.0,
        spread: 0.0,
        color: c.scrim,
    };
    let border = Border::Solid {
        width: 1.0,
        color: c.field_border,
    };
    panel_node(
        style,
        surface(c.surface, theme.radii.lg, border, shadow),
        children,
    )
}

/// Recessed well (bar tracks, slot backgrounds): darker than the panel, no border, no shadow.
pub fn inset<Msg>(theme: &Theme, style: Style, children: Vec<Node<Msg>>) -> Node<Msg> {
    panel_node(
        style,
        surface(
            theme.colors.surface_inset,
            theme.radii.md,
            Border::None,
            Shadow::None,
        ),
        children,
    )
}
