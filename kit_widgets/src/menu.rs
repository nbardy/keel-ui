// Ported from egui 0.36.2 `crates/egui/src/containers/menu.rs` (MIT OR Apache-2.0, (c) Emil Ernerfeldt, Rerun Technologies).
// Changes: kit state, intents and theme tokens replace egui's Ui/Response/Memory/epaint.
//! Context menus open from a control's `Bindings::on_context` (a secondary-button click; the router has no long-press)
//! and need a fresh key per gesture. Every popup returns focus to the control that opened it when it closes
//! (`dismissible_from`); a context menu names its opener in `MenuKind::Context`.
//! Controls use Button semantics; the kit has no menu roles.
//! Up/Down follow router focus; Right opens its submenu, Left/Escape closes the child, selection closes all.
use crate::common::{Press, ink, surface};
use kit_core::kurbo::Point;
use kit_core::{Border, Key, NamedKey, Shadow, Theme, WidgetId};
use kit_state::{Act, Cx, Host, Ident, Intent};
use kit_view::{
    A11y, Anchor, AnchorH, AnchorV, Bindings, Layer, Node, PortalAnchor, Role, SafeArea,
};
use kit_view::{
    Side, Style, Toggle, Weight, column, disabled, dismissible_from, interactive, nothing, panel,
    px, text,
};

/// Leaves cannot contain submenus: the data model enforces one level.
pub enum MenuItem<'a, Msg> {
    Item(&'a str, Press<Msg>),
    Separator,
}
pub enum MenuEntry<'a, Msg> {
    Item(MenuItem<'a, Msg>),
    Submenu(&'a str, &'a [MenuItem<'a, Msg>]),
}
#[derive(Copy, Clone)]
pub enum MenuKind {
    Button,
    /// Starts open at this viewport position. Use a fresh key for each context gesture. `opener` is the control the
    /// player right-clicked: focus returns to it when the menu closes.
    Context {
        at: Point,
        opener: WidgetId,
    },
}
pub struct Menu<'a, Msg, K> {
    pub theme: &'a Theme,
    pub key: K,
    pub label: &'a str,
    pub kind: MenuKind,
    pub entries: &'a [MenuEntry<'a, Msg>],
}
#[derive(Copy, Clone, Debug, PartialEq)]
enum Open {
    Closed,
    Root,
    Submenu(usize),
}
#[derive(Hash)]
struct Item<'a, K>(&'a K, usize, usize);

pub fn menu<A: Host, K: Ident<A>>(cx: &Cx<'_, A>, p: Menu<'_, Intent<A>, K>) -> Node<Intent<A>> {
    let th = p.theme;
    let initial = match p.kind {
        MenuKind::Button => Open::Closed,
        MenuKind::Context { .. } => Open::Root,
    };
    let state = cx.state(
        &p.key,
        || initial,
        |s: &mut Open, _: &K, e: Open, _: &A| {
            *s = e;
            Act::none()
        },
    );
    let close = state.send(Open::Closed);
    let (toggle, next) = match *state {
        Open::Closed => (Toggle::Off, Open::Root),
        Open::Root | Open::Submenu(_) => (Toggle::On, Open::Closed),
    };
    let button = match p.kind {
        MenuKind::Button => interactive(
            WidgetId::of(&p.key),
            Bindings::activate(state.send(next)),
            name(p.label, toggle),
            face(th, p.label, true),
        ),
        MenuKind::Context { .. } => nothing(),
    };
    if *state == Open::Closed {
        return column([button, nothing()]);
    }
    let rows = p.entries.iter().enumerate().map(|(i, entry)| {
        let id = Item(&p.key, 0, i);
        match entry {
            MenuEntry::Item(item) => leaf(th, WidgetId::of(&id), item, &close),
            MenuEntry::Submenu(label, items) => {
                let expanded = *state == Open::Submenu(i);
                let (event, key, toggle) = match expanded {
                    true => (Open::Root, NamedKey::ArrowLeft, Toggle::On),
                    false => (Open::Submenu(i), NamedKey::ArrowRight, Toggle::Off),
                };
                let mut on = Bindings::activate(state.send(event));
                if expanded || cx.focused(&id) {
                    on = on.with_shortcut(Key::Named(key));
                }
                let button = interactive(
                    WidgetId::of(&id),
                    on,
                    name(label, toggle),
                    face(th, &format!("{label}  >"), true),
                );
                let child = match expanded {
                    false => nothing(),
                    true => {
                        let rows = items.iter().enumerate().map(|(j, item)| {
                            leaf(th, WidgetId::of(&Item(&p.key, i + 1, j)), item, &close)
                        });
                        popup(
                            th,
                            WidgetId::of(&id),
                            beside(WidgetId::of(&id), Side::Right),
                            state.send(Open::Root),
                            column(rows),
                        )
                    }
                };
                column([button, child])
            }
        }
    });
    let (opener, anchor) = match p.kind {
        MenuKind::Button => (
            WidgetId::of(&p.key),
            beside(WidgetId::of(&p.key), Side::Below),
        ),
        MenuKind::Context { at, opener } => (
            opener,
            PortalAnchor::Viewport(Anchor {
                h: AnchorH::Start,
                v: AnchorV::Start,
                offset: at.to_vec2(),
                safe_area: SafeArea::Ignore,
            }),
        ),
    };
    column([
        button,
        popup(th, opener, anchor, close.clone(), column(rows)),
    ])
}
fn name(label: &str, toggle: Toggle) -> A11y {
    A11y::Labeled {
        role: Role::Button,
        label: label.to_owned(),
        toggle,
    }
}
fn face<M>(th: &Theme, label: &str, enabled: bool) -> Node<M> {
    let color = if enabled {
        th.colors.text
    } else {
        th.colors.text_disabled
    };
    text(label, ink(th.type_scale.label, color, Weight::NORMAL))
        .pad_xy(th.spacing.md, th.spacing.sm)
}
fn leaf<A: Host>(
    th: &Theme,
    id: WidgetId,
    item: &MenuItem<'_, Intent<A>>,
    close: &Intent<A>,
) -> Node<Intent<A>> {
    match item {
        MenuItem::Separator => {
            let mut style = Style::row();
            style.layout.size.height = px(th.spacing.xs);
            panel(
                style,
                surface(th.colors.field_border, 0.0, Border::None, Shadow::None),
                vec![],
            )
        }
        MenuItem::Item(label, Press::Disabled) => {
            disabled(id, name(label, Toggle::NotToggle), face(th, label, false))
        }
        MenuItem::Item(label, Press::Sends(intent)) => interactive(
            id,
            Bindings::activate(Intent::All(vec![intent.clone(), close.clone()])),
            name(label, Toggle::NotToggle),
            face(th, label, true),
        ),
    }
}
fn beside(id: WidgetId, side: Side) -> PortalAnchor {
    PortalAnchor::Node { id, side, gap: 0.0 }
}
fn popup<M>(
    th: &Theme,
    opener: WidgetId,
    anchor: PortalAnchor,
    close: M,
    body: Node<M>,
) -> Node<M> {
    let border = Border::Solid {
        width: 1.0,
        color: th.colors.field_border,
    };
    let frame = surface(th.colors.surface_raised, th.radii.md, border, Shadow::None);
    dismissible_from(
        opener,
        Layer::Popup,
        anchor,
        close,
        panel(Style::column().padded(th.spacing.xs), frame, vec![body]),
    )
}
