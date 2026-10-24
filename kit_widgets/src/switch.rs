// Ported from egui 0.36.2 `crates/egui_demo_lib/src/demo/toggle_switch.rs` (MIT OR Apache-2.0, (c) Emil Ernerfeldt, Rerun Technologies).
// Changes: kit state, intents and theme tokens replace egui's Ui/Response/Memory/epaint.
//! Controlled on/off switch: click or focused Space/Enter toggles; reduced motion snaps the knob.
use crate::common::{Press, ink, pressable, solid, surface};
use kit_core::kurbo::Vec2;
use kit_core::{Border, Shadow, ShapeSpec, Theme, WidgetId};
use kit_state::{Cx, Host, Ident, Intent};
use kit_view::taffy::AlignItems;
use kit_view::{
    A11y, AnchorH, AnchorV, Node, Paint, Role, Style, Toggle, Weight, panel, row, shape, text,
};

pub struct Switch<'a, Msg, K> {
    pub theme: &'a Theme,
    pub key: K,
    pub label: &'a str,
    pub on: bool,
    pub press: Press<Msg>,
}

pub fn switch<A: Host, K: Ident<A>>(
    cx: &Cx<'_, A>,
    p: Switch<'_, Intent<A>, K>,
) -> Node<Intent<A>> {
    let (c, th) = (&p.theme.colors, p.theme);
    let t = f64::from(cx.animate_bool(&p.key, p.on, th.motion.fast()));
    let color = match &p.press {
        Press::Disabled => c.text_disabled,
        Press::Sends(_) => c.text,
    };
    let (fill, toggle) = match p.on {
        false => (c.field_fill, Toggle::Off),
        true => (c.accent, Toggle::On),
    };
    let h = th.controls.mini.1;
    // egui demo: track aspect 2:1, radius h/2, knob radius 0.75*radius, centre lerps h/2..1.5*h.
    let knob = Style::row().sized(h * 0.75, h * 0.75);
    let knob = knob.anchored(
        AnchorH::Start,
        AnchorV::Center,
        Vec2::new(h / 8.0 + h * t, 0.0),
    );
    let knob = shape(knob, ShapeSpec::Ellipse, Paint::Fill(solid(color)));
    let frame = surface(fill, h / 2.0, Border::None, Shadow::None);
    let track = panel(Style::anchors().sized(2.0 * h, h), frame, vec![knob]);
    let label = text(p.label, ink(th.type_scale.label, color, Weight::NORMAL));
    let face = row([track, label])
        .gap(th.spacing.sm)
        .pad(th.spacing.xs)
        .align_items(AlignItems::CENTER);
    let a11y = A11y::Labeled {
        role: Role::CheckBox,
        label: p.label.to_owned(),
        toggle,
    };
    pressable(WidgetId::of(&p.key), p.press, a11y, face)
}
