// Ported from egui 0.36.2 `crates/egui/src/containers/tooltip.rs` (MIT OR Apache-2.0, (c) Emil Ernerfeldt, Rerun Technologies).
// Changes: kit state, intents and theme tokens replace egui's Ui/Response/Memory/epaint.
//! Delayed hover help with rich content. Hosts must rebuild as `cx.now()` advances.
use crate::common::surface;
use core::time::Duration;
use kit_core::{Border, Shadow, Theme, Time, WidgetId};
use kit_state::{Act, Cx, Host, Ident, Intent};
use kit_view::{A11y, Bindings, Layer, Node, PortalAnchor, Side, Style};
use kit_view::{column, interactive, nothing, panel, portal};
pub struct Tooltip<'a, Msg, K> {
    pub theme: &'a Theme,
    pub key: K,
    pub delay: Duration,
    /// Trigger face (no nested controls); its bindings and accessible name follow.
    pub trigger: Node<Msg>,
    pub on: Bindings<Msg>,
    pub a11y: A11y,
    pub content: Node<Msg>,
}
#[derive(Clone)]
enum Hover {
    Away,
    Since(Time),
}
pub fn tooltip<A: Host, K: Ident<A>>(
    cx: &Cx<'_, A>,
    p: Tooltip<'_, Intent<A>, K>,
) -> Node<Intent<A>> {
    let hover = cx.state(
        &p.key,
        || Hover::Away,
        |s: &mut Hover, _: &K, enter: bool, app: &A| {
            *s = match enter {
                true => Hover::Since(app.now()),
                false => Hover::Away,
            };
            Act::none()
        },
    );
    let shown = match *hover {
        Hover::Away => false,
        Hover::Since(start) => {
            cx.hovered(&p.key) && cx.redraw_at(Time(start.0 + p.delay)).since(start) >= p.delay
        }
    };
    let (enter, leave) = (hover.send(true), hover.send(false));
    let both = |a, b| Intent::All(vec![a, b]);
    let mut on = p.on;
    on = match on.hover.take() {
        None => on.with_hover(enter, leave),
        Some(old) => on.with_hover(both(old.enter, enter), both(old.leave, leave)),
    };
    let id = WidgetId::of(&p.key);
    let trigger = interactive(id, on, p.a11y, p.trigger);
    if !shown {
        return column([trigger, nothing()]);
    }
    let th = p.theme;
    let border = Border::Solid {
        width: 1.0,
        color: th.colors.field_border,
    };
    let frame = surface(th.colors.surface_raised, th.radii.md, border, Shadow::None);
    let style = Style::column().padded(th.spacing.sm);
    let body = panel(style, frame, vec![p.content]);
    let anchor = PortalAnchor::Node {
        id,
        side: Side::Below,
        gap: th.spacing.xs,
    };
    column([trigger, portal(Layer::Tooltip, anchor, body)])
}
