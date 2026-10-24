// Ported from egui 0.36.2 `crates/egui/src/containers/modal.rs` (MIT OR Apache-2.0, (c) Emil Ernerfeldt, Rerun Technologies).
// Changes: kit state, intents and theme tokens replace egui's Ui/Response/Memory/epaint.
//! Centered dialog; the caller controls its lifetime. The kit owns the scrim, the focus trap and the return of focus to
//! the opener when the dialog closes.
use crate::common::{ink, surface};
use kit_core::{Border, Shadow, Theme, WidgetId};
use kit_view::{A11y, Anchor, AnchorH, AnchorV, Bindings, Layer, Node, PortalAnchor, Role};
use kit_view::{SafeArea, Style, Toggle, Weight, dismissible_from, interactive, panel, row, text};
use std::hash::Hash;

/// One enabled action; its intent decides whether the caller also closes the dialog.
pub struct Action<'a, Msg> {
    pub label: &'a str,
    pub intent: Msg,
}

pub struct Modal<'a, Msg, K> {
    pub theme: &'a Theme,
    pub key: K,
    pub title: &'a str,
    pub body: Node<Msg>,
    pub primary: Action<'a, Msg>,
    pub actions: Vec<Action<'a, Msg>>,
    /// Sent by Close, Escape, or a press on the backdrop. Does not close locally.
    pub dismiss: Msg,
    /// The control that opened this dialog. Focus returns to it when the dialog closes, as for menus: if it is gone,
    /// disabled or behind the dialog, the first focusable control of the screen takes focus and the miss is counted.
    pub opener: WidgetId,
}

/// On opening, the host must pass this id to `Router::request_focus` after layout.
/// The kit currently has no view-level initial-focus request; do this once, not every frame.
pub fn primary_action_id(key: &impl Hash) -> WidgetId {
    WidgetId::of(&(key, "modal-primary"))
}

pub fn modal<Msg: Clone, K: Hash>(p: Modal<'_, Msg, K>) -> Node<Msg> {
    let (th, c) = (p.theme, &p.theme.colors);
    let button = |id, action: Action<'_, Msg>, fill| {
        let face = panel(
            Style::row().padded_xy(th.spacing.md, th.spacing.sm),
            surface(fill, th.radii.control, Border::None, Shadow::None),
            vec![text(
                action.label,
                ink(th.type_scale.label, c.text, Weight::NORMAL),
            )],
        );
        let name = A11y::Labeled {
            role: Role::Button,
            label: action.label.to_owned(),
            toggle: Toggle::NotToggle,
        };
        interactive(id, Bindings::activate(action.intent), name, face)
    };
    let mut buttons = vec![button(primary_action_id(&p.key), p.primary, c.accent)];
    buttons.extend(p.actions.into_iter().enumerate().map(|(i, action)| {
        button(
            WidgetId::of(&(&p.key, "modal-action", i)),
            action,
            c.surface_inset,
        )
    }));
    buttons.push(button(
        WidgetId::of(&(&p.key, "modal-close")),
        Action {
            label: "Close",
            intent: p.dismiss.clone(),
        },
        c.surface_inset,
    ));
    let title = text(p.title, ink(th.type_scale.title, c.text, Weight::BOLD));
    let body = panel(
        Style::column().padded(th.spacing.lg).gap(th.spacing.md),
        surface(c.surface_raised, th.radii.lg, Border::None, Shadow::None),
        vec![title, p.body, row(buttons).gap(th.spacing.sm)],
    );
    let anchor = Anchor {
        h: AnchorH::Center,
        v: AnchorV::Center,
        offset: kit_core::kurbo::Vec2::ZERO,
        safe_area: SafeArea::Respect,
    };
    // Same restoration as the popups (`dismissible_from`): a dialog that dropped its opener from `returns` would close
    // with focus on nothing, and the next key would start from the top of the screen.
    dismissible_from(
        p.opener,
        Layer::Modal,
        PortalAnchor::Viewport(anchor),
        p.dismiss,
        body,
    )
}
