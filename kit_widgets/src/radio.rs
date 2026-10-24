// Ported from egui 0.36.2 `crates/egui/src/widgets/radio_button.rs` (MIT OR Apache-2.0, (c) Emil Ernerfeldt, Rerun Technologies).
// Changes: kit state, intents and theme tokens replace egui's Ui/Response/Memory/epaint.
//! Click/Space selects; Up/Down moves focus. Panics unless exactly one option matches `selected`. Immediate arrow selection and radio roles need router support.
use crate::common::{Press, ink, pressable, solid, surface};
use kit_core::{Border, Shadow, ShapeSpec, Theme, WidgetId};
use kit_state::{Cx, Host, Ident, Intent};
use kit_view::taffy::{AlignItems, JustifyContent};
use kit_view::{A11y, Availability, Node, Paint, Role, Style, Toggle, Weight};
use kit_view::{column, nothing, panel, row, shape, text};
pub struct RadioOption<'a, V> {
    pub value: V,
    pub label: &'a str,
    pub availability: Availability,
}
pub struct Radio<'a, V, Msg, K> {
    pub theme: &'a Theme,
    pub key: K,
    pub options: &'a [RadioOption<'a, V>],
    pub selected: &'a V,
    pub choose: &'a dyn Fn(&V) -> Msg,
}
pub fn radio<A: Host, K: Ident<A>, V: PartialEq>(
    cx: &Cx<'_, A>,
    p: Radio<'_, V, Intent<A>, K>,
) -> Node<Intent<A>> {
    assert!(
        p.options.iter().filter(|o| &o.value == p.selected).count() == 1,
        "radio: selected value must match exactly one option"
    );
    let (c, th) = (&p.theme.colors, p.theme);
    column(p.options.iter().enumerate().map(|(i, o)| {
        let key = (&p.key, i);
        let (color, edge) = match (o.availability, cx.hovered(&key)) {
            (Availability::Disabled, _) => (c.text_disabled, c.text_disabled),
            (Availability::Enabled, true) => (c.text, c.text),
            (Availability::Enabled, false) => (c.text, c.field_border),
        };
        // egui spacing: icon_width=14, icon_width_inner=8; checked radius is inner width / 3.
        let (outer, dot) = (14.0, 2.0 * 8.0 / 3.0);
        let size = Style::row().sized(dot, dot);
        let paint = Paint::Fill(solid(color));
        let mark = || shape(size.clone(), ShapeSpec::Ellipse, paint.clone());
        let (mark, toggle) = match &o.value == p.selected {
            true => (mark(), Toggle::On),
            false => (nothing(), Toggle::Off),
        };
        let center = Style::row().sized(outer, outer);
        let center = center.align_items(AlignItems::CENTER);
        let border = Border::Solid {
            width: 1.0,
            color: edge,
        };
        let frame = surface(c.field_fill, outer / 2.0, border, Shadow::None);
        let circle = panel(center.justify(JustifyContent::CENTER), frame, vec![mark]);
        let caption = text(o.label, ink(th.type_scale.label, color, Weight::NORMAL));
        let face = row([circle, caption]).gap(th.spacing.sm).pad(th.spacing.xs);
        let face = face.align_items(AlignItems::CENTER);
        let a11y = A11y::Labeled {
            role: Role::Option,
            label: o.label.to_owned(),
            toggle,
        };
        let press = match o.availability {
            Availability::Enabled => Press::Sends((p.choose)(&o.value)),
            Availability::Disabled => Press::Disabled,
        };
        pressable(WidgetId::of(&key), press, a11y, face)
    }))
    .gap(th.spacing.xs)
}
