// Component template. Copy to `src/<name>.rs`, rename, delete these two header lines.
// Ported from egui 0.36.2 `crates/egui/src/widgets/<file>.rs` (MIT OR Apache-2.0, (c) Emil Ernerfeldt, Rerun Technologies); changes: kit state, theme tokens.
//! `tally`: a button that counts its presses and wraps at `limit`. Props in, `Node<Intent<A>>` out.
use crate::common::{ink, surface};
use kit_core::{Border, Shadow, Theme};
use kit_state::{Act, Cx, Host, Ident, Intent};
use kit_view::{A11y, Bindings, Node, Role, Style, Toggle, Weight, interactive, panel, text};

/// Everything the caller decides. The key is DATA (the egui `Id`): it names the state and the node.
pub struct Tally<'a, K> {
    pub theme: &'a Theme,
    pub key: K,
    pub label: &'a str,
    pub limit: u32,
}

#[derive(Clone, Debug, PartialEq)]
enum Ev {
    Bump { limit: u32 },
}

pub fn tally<A: Host, K: Ident<A>>(cx: &Cx<'_, A>, p: Tally<'_, K>) -> Node<Intent<A>> {
    let (c, th) = (&p.theme.colors, p.theme);
    // egui `ui.memory`: keyed state with an inline, NON-capturing reducer (the limit rides in the event).
    let count = cx.state(
        &p.key,
        || 0_u32,
        |n: &mut u32, _: &K, ev: Ev, _: &A| match ev {
            Ev::Bump { limit } => {
                *n = (*n + 1) % limit;
                Act::none()
            }
        },
    );
    let fill = if *count == 0 {
        c.surface_raised
    } else {
        c.accent
    };
    let border = Border::Solid {
        width: 1.0,
        color: c.field_border,
    };
    let face = panel(
        Style::row().padded_xy(th.spacing.md, th.spacing.sm),
        surface(fill, th.radii.control, border, Shadow::None),
        vec![text(
            format!("{}: {}", p.label, *count),
            ink(th.type_scale.label, c.text, Weight::NORMAL),
        )],
    );
    // egui `response.clicked()`: the press IS the intent; the reducer runs when the host dispatches it.
    let on = Bindings::activate(count.send(Ev::Bump { limit: p.limit }));
    let lit = if *count == 0 { Toggle::Off } else { Toggle::On }; // announced to screen readers and tests
    let name = A11y::Labeled {
        role: Role::Button,
        label: p.label.to_string(),
        toggle: lit,
    };
    interactive(kit_core::WidgetId::of(&p.key), on, name, face)
}
