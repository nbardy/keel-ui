// Ported from egui 0.36.2 `crates/egui/src/containers/combo_box.rs` (MIT OR Apache-2.0, (c) Emil Ernerfeldt, Rerun Technologies).
// Changes: kit state, intents and theme tokens replace egui's Ui/Response/Memory/epaint.
//! `combo_box`: a button showing the chosen option. Pressing it opens the options in a popup portal below it (flipped
//! above when there is no room, clamped to the screen); past egui's `combo_height` the list scrolls. Choosing an
//! option closes it (egui `CloseOnClick`); a press outside or Escape closes it without choosing, and that press never
//! reaches what lies behind. Keyboard: Tab to it, Enter/Space opens, Down/Up walk the options (router focus, the
//! focused option is scrolled into view), Enter/Space chooses. Open/closed is the combo's state, keyed by `key`; the
//! chosen value is the caller's.
use crate::common::{ink, solid, surface};
use kit_core::kurbo::{BezPath, Rect};
use kit_core::{Axis, Border, Shadow, ShapeSpec, Theme, WidgetId};
use kit_state::{Act, Cx, Host, Ident, Intent};
use kit_view::taffy::AlignItems;
use kit_view::{A11y, Bindings, Layer, Node, Paint, PortalAnchor, RevealAlign, Role, ScrollSpec};
use kit_view::{
    Side, Style, Toggle, Weight, column, dismissible_from, interactive, nothing, panel,
};
use kit_view::{px, scroll_area, shape, stack, text};

/// egui `spacing.combo_height` (the list scrolls beyond it) and `combo_width` (least width), logical px.
const LIST_MAX: f64 = 200.0;
const MIN_WIDTH: f64 = 100.0;
/// egui `spacing.icon_width`; the open-arrow is 0.7 x 0.45 of it (`paint_default_icon`).
const ICON: f64 = 14.0;

pub struct ComboBox<'a, Msg, K> {
    pub theme: &'a Theme,
    pub key: K,
    /// The accessible name ("Ship class"); the button shows the chosen option.
    pub label: &'a str,
    pub options: &'a [&'a str],
    /// Index into `options`. Panics when out of range: the caller's value names no option.
    pub selected: usize,
    /// The caller's intent choosing option `i`.
    pub choose: &'a dyn Fn(usize) -> Msg,
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
enum Open {
    Closed,
    Open,
}
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
enum Ev {
    Toggle,
    Close,
}
/// Option `i` of the combo keyed `K`: its node id, for focus and scroll-to.
#[derive(Hash)]
struct Item<'k, K>(&'k K, usize);

pub fn combo_box<A: Host, K: Ident<A>>(
    cx: &Cx<'_, A>,
    p: ComboBox<'_, Intent<A>, K>,
) -> Node<Intent<A>> {
    let (c, th) = (&p.theme.colors, p.theme);
    let open = cx.state(
        &p.key,
        || Open::Closed,
        |o: &mut Open, _: &K, ev: Ev, _: &A| {
            *o = match (ev, *o) {
                (Ev::Toggle, Open::Closed) => Open::Open,
                (Ev::Toggle, Open::Open) | (Ev::Close, _) => Open::Closed,
            };
            Act::none()
        },
    );
    let mut arrow = BezPath::new();
    arrow.move_to((0.0, 0.0));
    arrow.line_to((1.0, 0.0));
    arrow.line_to((0.5, 1.0));
    arrow.close_path();
    let (view_box, fill) = (Rect::new(0.0, 0.0, 1.0, 1.0), Paint::Fill(solid(c.text)));
    let icon = shape(
        Style::row().sized(ICON * 0.7, ICON * 0.45),
        ShapeSpec::Path {
            path: arrow,
            view_box,
        },
        fill,
    );
    let chosen = text(
        p.options[p.selected],
        ink(th.type_scale.label, c.text, Weight::NORMAL),
    )
    .grow(1.0);
    let border = Border::Solid {
        width: 1.0,
        color: c.field_border,
    };
    let mut face = Style::row()
        .padded_xy(th.spacing.md, th.spacing.sm)
        .gap(th.spacing.sm);
    face.layout.min_size.width = px(MIN_WIDTH);
    let frame = surface(c.surface_raised, th.radii.control, border, Shadow::None);
    let button = panel(
        face.align_items(AlignItems::CENTER),
        frame,
        vec![chosen, icon],
    );
    let (toggle, list) = match *open {
        Open::Closed => (Toggle::Off, nothing()),
        Open::Open => (Toggle::On, options(cx, &p, open.send(Ev::Close))),
    };
    let a11y = A11y::Labeled {
        role: Role::ComboBox,
        label: p.label.to_string(),
        toggle,
    };
    // The list follows the button in tree order, so Down from the focused button lands on the first option.
    column([
        interactive(
            WidgetId::of(&p.key),
            Bindings::activate(open.send(Ev::Toggle)),
            a11y,
            button,
        ),
        list,
    ])
}

fn options<A: Host, K: Ident<A>>(
    cx: &Cx<'_, A>,
    p: &ComboBox<'_, Intent<A>, K>,
    close: Intent<A>,
) -> Node<Intent<A>> {
    let (c, th) = (&p.theme.colors, p.theme);
    let rows = p.options.iter().enumerate().map(|(i, name)| {
        let pad = Style::row().padded_xy(th.spacing.md, th.spacing.sm);
        let label = text(*name, ink(th.type_scale.label, c.text, Weight::NORMAL));
        // egui `selectable_value`: the chosen row is filled.
        let (face, toggle) = match i == p.selected {
            true => (
                panel(
                    pad,
                    surface(c.accent, th.radii.sm, Border::None, Shadow::None),
                    vec![label],
                ),
                Toggle::On,
            ),
            false => (stack(pad, vec![label]), Toggle::Off),
        };
        let pick = Intent::All(vec![(p.choose)(i), close.clone()]);
        let a11y = A11y::Labeled {
            role: Role::Option,
            label: name.to_string(),
            toggle,
        };
        interactive(
            WidgetId::of(&Item(&p.key, i)),
            Bindings::activate(pick),
            a11y,
            face,
        )
    });
    // Keep the focused option in view while Down/Up walk the list; with none focused, the chosen one.
    let focused = (0..p.options.len()).find(|&i| cx.focused(&Item(&p.key, i)));
    let shown = Item(&p.key, focused.unwrap_or(p.selected));
    let spec = ScrollSpec::new(Axis::Vertical).reveal_child(&shown, RevealAlign::Nearest);
    let mut viewport = Style::column();
    viewport.layout.max_size.height = px(LIST_MAX);
    let mut body = Style::column().padded(th.spacing.xs);
    body.layout.min_size.width = px(MIN_WIDTH);
    let frame = surface(
        c.surface_raised,
        th.radii.md,
        Border::Solid {
            width: 1.0,
            color: c.field_border,
        },
        Shadow::None,
    );
    let list = scroll_area(
        WidgetId::of(&(&p.key, "list")),
        spec,
        viewport,
        column(rows),
    );
    let anchor = PortalAnchor::Node {
        id: WidgetId::of(&p.key),
        side: Side::Below,
        gap: th.spacing.xs,
    };
    dismissible_from(
        WidgetId::of(&p.key),
        Layer::Popup,
        anchor,
        close,
        panel(body, frame, vec![list]),
    )
}
