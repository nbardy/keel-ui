// Ported from egui 0.36.2 `crates/egui/src/containers/collapsing_header.rs` (MIT OR Apache-2.0, (c) Emil Ernerfeldt, Rerun Technologies).
// Changes: kit state, intents and theme tokens replace egui's Ui/Response/Memory/epaint.
//! A keyed disclosure: closed bodies are lazy; height, opacity and the chevron animate together.
use crate::common::{ink, solid};
use kit_core::kurbo::{BezPath, Rect, Stroke};
use kit_core::{ShapeSpec, Theme, WidgetId};
use kit_state::{Act, Cx, Disclosure, DisclosureEv, Host, Ident, Intent};
use kit_view::taffy::{AlignItems, style_helpers::flex};
use kit_view::{A11y, Bindings, Node, Paint, Role, Style, Toggle, Weight};
use kit_view::{clipped, column, interactive, layer, nothing, px, row, shape, stack, text};
/// egui's `spacing.icon_width`; the chevron turns through the default icon's quarter-circle.
const ICON: f64 = 14.0;
pub struct Collapsing<'a, K, F> {
    pub theme: &'a Theme,
    pub key: K,
    pub label: &'a str,
    pub default_open: bool,
    pub body: F,
}
pub fn collapsing<A: Host, K: Ident<A>, F: FnOnce() -> Node<Intent<A>>>(
    cx: &Cx<'_, A>,
    p: Collapsing<'_, K, F>,
) -> Node<Intent<A>> {
    let (c, th) = (&p.theme.colors, p.theme);
    let initial = match p.default_open {
        true => Disclosure::OPEN,
        false => Disclosure::CLOSED,
    };
    let open = cx.state(
        &p.key,
        || initial,
        |d: &mut Disclosure, _: &K, ev: DisclosureEv, app: &A| {
            d.step(ev, app.now());
            Act::none()
        },
    );
    let (on, toggle) = match *open {
        Disclosure::Closed { .. } => (false, Toggle::Off),
        Disclosure::Open { .. } => (true, Toggle::On),
    };
    let reveal = cx.animate_bool(&p.key, on, th.motion.normal());
    let angle = f64::from(reveal) * core::f64::consts::FRAC_PI_2;
    let (sin, cos) = angle.sin_cos();
    // egui's icon uses a centred 0.75-size box, rotated from right to down.
    let mid = ICON / 2.0;
    let point = |x, y| (mid + x * cos - y * sin, mid + x * sin + y * cos);
    let mut path = BezPath::new();
    path.move_to(point(-ICON * 0.25, -ICON * 0.375));
    path.line_to(point(ICON * 0.125, 0.0));
    path.line_to(point(-ICON * 0.25, ICON * 0.375));
    // egui's hovered foreground stroke is 1.5 logical px.
    let paint = Paint::Stroke {
        brush: solid(c.text),
        stroke: Stroke::new(1.5),
    };
    let glyph = ShapeSpec::Path {
        path,
        view_box: Rect::new(0.0, 0.0, ICON, ICON),
    };
    let icon = shape(Style::row().sized(ICON, ICON), glyph, paint);
    let label = text(p.label, ink(th.type_scale.label, c.text, Weight::NORMAL));
    let header = row([icon, label])
        .pad(th.spacing.sm)
        .gap(th.spacing.md)
        .align_items(AlignItems::CENTER);
    let a11y = A11y::Labeled {
        role: Role::Button,
        label: p.label.to_owned(),
        toggle,
    };
    let id = WidgetId::of(&p.key);
    let bind = Bindings::activate(open.send(DisclosureEv::Toggle));
    let header = interactive(id, bind, a11y, header);
    let body = match reveal > 0.0 {
        false => nothing(),
        true => {
            let grid = Style::grid(vec![flex(1.0)], vec![flex(reveal)]);
            let mut viewport = Style::column();
            viewport.layout.min_size.height = px(0.0);
            let mut content = Style::column();
            content.layout.flex_shrink = 0.0;
            content.layout.padding.left = px(ICON + th.spacing.md + th.spacing.sm);
            content.layout.padding.top = px(th.spacing.sm);
            let body = stack(content, vec![(p.body)()]);
            let viewport = clipped(stack(viewport, vec![body]));
            layer(reveal, stack(grid, vec![viewport]))
        }
    };
    column([header, body])
}
