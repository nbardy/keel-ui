// Ported from egui 0.36.2 `crates/egui/src/widgets/button.rs` (MIT OR Apache-2.0, (c) Emil Ernerfeldt, Rerun Technologies).
// Changes: kit state, intents and theme tokens replace egui's Ui/Response/Memory/epaint.
//! Equal-width selectable buttons with a sliding background. Focused Left/Right select a neighbor;
//! navigation deliberately stops at the ends instead of wrapping.
use crate::common::{ink, surface};
use kit_core::{Border, Key, NamedKey, Shadow, Theme, WidgetId, kurbo::Vec2};
use kit_state::{Cx, Host, Ident, Intent};
use kit_view::taffy::{AlignItems, JustifyContent};
use kit_view::{A11y, AnchorH, AnchorV, Bindings, Node, Role, Style, Toggle, Weight};
use kit_view::{interactive, panel, stack, text};

/// Controlled toggle buttons; `segment_width` should be theme-derived and fit the labels.
pub struct Segmented<'a, Msg, K> {
    pub theme: &'a Theme,
    pub key: K,
    pub options: &'a [&'a str],
    pub selected: usize,
    pub segment_width: f64,
    pub choose: &'a dyn Fn(usize) -> Msg,
}
/// Panics unless there are 2–6 options, `selected` is in range and `segment_width` is finite and positive.
pub fn segmented<A: Host, K: Ident<A>>(
    cx: &Cx<'_, A>,
    p: Segmented<'_, Intent<A>, K>,
) -> Node<Intent<A>> {
    let valid = (2..=6).contains(&p.options.len())
        && p.selected < p.options.len()
        && p.segment_width.is_finite()
        && p.segment_width > 0.0;
    assert!(valid, "segmented: invalid options, selection or width");
    let (th, c) = (p.theme, &p.theme.colors);
    let (w, h) = (p.segment_width, th.controls.compact.1);
    let frame = |fill| surface(fill, th.radii.control, Border::None, Shadow::None);
    let at = cx.animate(&p.key, p.selected as f32, th.motion.fast());
    let anchored = |x| Style::row().anchored(AnchorH::Start, AnchorV::Start, Vec2::new(x, 0.0));
    let position = anchored(f64::from(at) * w).sized(w, h);
    let indicator = panel(position, frame(c.accent), vec![]);
    let focused = (0..p.options.len()).any(|i| cx.focused(&(&p.key, i)));
    let buttons = p.options.iter().enumerate().map(|(i, label)| {
        let mut on = Bindings::activate((p.choose)(i));
        // No scoped key binding exists yet: publish neighbor shortcuts only while this group owns focus.
        if focused && i + 1 == p.selected {
            on = on.with_shortcut(Key::Named(NamedKey::ArrowLeft));
        }
        if focused && i == p.selected + 1 {
            on = on.with_shortcut(Key::Named(NamedKey::ArrowRight));
        }
        let toggle = if i == p.selected {
            Toggle::On
        } else {
            Toggle::Off
        };
        let name = A11y::Labeled {
            role: Role::Button,
            label: label.to_string(),
            toggle,
        };
        let style = Style::row()
            .sized(w, h)
            .align_items(AlignItems::CENTER)
            .justify(JustifyContent::CENTER);
        let caption = text(*label, ink(th.type_scale.label, c.text, Weight::NORMAL));
        let id = WidgetId::of(&(&p.key, i));
        interactive(id, on, name, stack(style, vec![caption]))
    });
    let width = w * p.options.len() as f64;
    let labels = stack(anchored(0.0).sized(width, h), buttons.collect());
    let style = Style::anchors().sized(width, h);
    panel(style, frame(c.field_fill), vec![indicator, labels])
}
