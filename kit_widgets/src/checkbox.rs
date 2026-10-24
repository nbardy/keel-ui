// Ported from egui 0.36.2 `crates/egui/src/widgets/checkbox.rs` (MIT OR Apache-2.0, (c) Emil Ernerfeldt, Rerun Technologies).
// Changes: kit state, intents and theme tokens replace egui's Ui/Response/Memory/epaint.
//! `checkbox`: a box with a check mark (or egui's indeterminate dash) and a label. Controlled: the caller owns the
//! value and hands over the intent that flips it. A press toggles it; so do Space and Enter, but only while it has
//! keyboard focus (the router's rule, so a page of checkboxes never all flip on one Space).
use crate::common::{Press, ink, pressable, solid, surface};
use kit_core::kurbo::{BezPath, Rect, Stroke};
use kit_core::{Border, Shadow, ShapeSpec, Theme, WidgetId};
use kit_state::{Cx, Host, Ident, Intent};
use kit_view::taffy::{AlignItems, JustifyContent};
use kit_view::{A11y, Node, Paint, Role, Style, Toggle, Weight, nothing, panel, row, shape, text};

/// egui `spacing.icon_width` (the box) and `icon_width_inner` (the mark inside it), logical px.
const BOX: f64 = 14.0;
const MARK: f64 = 8.0;

/// The value shown. `Mixed` is egui's `indeterminate`: a group whose members differ.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum Check {
    Off,
    On,
    Mixed,
}

pub struct Checkbox<'a, Msg, K> {
    pub theme: &'a Theme,
    pub key: K,
    pub label: &'a str,
    pub check: Check,
    /// The caller's intent that flips its value, or `Disabled` (greyed, skipped by presses, keys and Tab).
    pub press: Press<Msg>,
}

pub fn checkbox<A: Host, K: Ident<A>>(
    cx: &Cx<'_, A>,
    p: Checkbox<'_, Intent<A>, K>,
) -> Node<Intent<A>> {
    let (c, th) = (&p.theme.colors, p.theme);
    // egui strengthens the box outline on hover (`widgets.hovered.bg_stroke`).
    let (ink_color, edge) = match (&p.press, cx.hovered(&p.key)) {
        (Press::Disabled, _) => (c.text_disabled, c.text_disabled),
        (Press::Sends(_), true) => (c.text, c.text),
        (Press::Sends(_), false) => (c.text, c.field_border),
    };
    let mark = |points: &[(f64, f64)]| {
        let mut path = BezPath::new();
        path.move_to(points[0]);
        points[1..].iter().for_each(|&p| path.line_to(p));
        let view_box = Rect::new(0.0, 0.0, MARK, MARK);
        // 1.5: egui's `widgets.hovered.fg_stroke` width (the inactive 1.0 vanishes at this size).
        let paint = Paint::Stroke {
            brush: solid(ink_color),
            stroke: Stroke::new(1.5),
        };
        shape(
            Style::row().sized(MARK, MARK),
            ShapeSpec::Path { path, view_box },
            paint,
        )
    };
    // egui's marks: a check through left-middle, bottom-middle, top-right; a dash across the middle.
    let (inner, toggle) = match p.check {
        Check::Off => (nothing(), Toggle::Off),
        Check::On => (
            mark(&[(0.0, MARK / 2.0), (MARK / 2.0, MARK), (MARK, 0.0)]),
            Toggle::On,
        ),
        Check::Mixed => (
            mark(&[(0.0, MARK / 2.0), (MARK, MARK / 2.0)]),
            Toggle::Mixed,
        ),
    };
    let centred = Style::row().sized(BOX, BOX).align_items(AlignItems::CENTER);
    let border = Border::Solid {
        width: 1.0,
        color: edge,
    };
    let frame = surface(c.field_fill, th.radii.sm, border, Shadow::None);
    let boxed = panel(centred.justify(JustifyContent::CENTER), frame, vec![inner]);
    let caption = text(p.label, ink(th.type_scale.label, ink_color, Weight::NORMAL));
    let face = row([boxed, caption])
        .gap(th.spacing.sm)
        .pad(th.spacing.xs)
        .align_items(AlignItems::CENTER);
    let a11y = A11y::Labeled {
        role: Role::CheckBox,
        label: p.label.to_string(),
        toggle,
    };
    pressable(WidgetId::of(&p.key), p.press, a11y, face)
}
