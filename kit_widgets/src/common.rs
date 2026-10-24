//! Private helpers shared by the widgets: styles and brushes.
use kit_core::kurbo::{RoundedRectRadii, Vec2};
use kit_core::peniko::{Brush, Color};
use kit_core::{Border, Shadow, Surface};
use kit_view::taffy as tf;
use kit_view::{AnchorH, AnchorV, Numerals, Percent, Style, TextStyle, Weight};

pub(crate) fn solid(color: Color) -> Brush {
    Brush::Solid(color)
}

pub(crate) fn radii(r: f64) -> RoundedRectRadii {
    RoundedRectRadii::from_single_radius(r)
}

pub(crate) fn surface(fill: Color, r: f64, border: Border, shadow: Shadow) -> Surface {
    Surface {
        fill: solid(fill),
        radii: radii(r),
        border,
        shadow,
    }
}

/// A raised control face: the theme's raised fill at control radius, no border or shadow (a stepper button, a toast).
pub(crate) fn raised(theme: &kit_core::Theme) -> Surface {
    surface(
        theme.colors.surface_raised,
        theme.radii.control,
        Border::None,
        Shadow::None,
    )
}

/// A text edit's outcome as a reducer's act: a refused edit is data, counted in the store's faults.
pub(crate) fn applied<Ask, E>(applied: kit_view::Applied) -> kit_state::Act<Ask, E> {
    match applied {
        kit_view::Applied::Ok(_) => kit_state::Act::none(),
        kit_view::Applied::Stale { .. } => {
            kit_state::Act::fault("text edit for an older revision refused")
        }
        kit_view::Applied::Misplaced => {
            kit_state::Act::fault("text edit inside a grapheme refused")
        }
    }
}

/// A range's ends in increasing total order (egui accepts reversed ranges). Panics on a NaN end.
pub(crate) fn ordered(range: &core::ops::RangeInclusive<f64>) -> (f64, f64) {
    let (start, end) = (*range.start(), *range.end());
    assert!(
        !start.is_nan() && !end.is_nan(),
        "range bounds cannot be NaN"
    );
    match start.total_cmp(&end).is_gt() {
        true => (end, start),
        false => (start, end),
    }
}

/// egui `clamp_value_to_range`: total order, so signed zero is kept at the ends.
pub(crate) fn clamp(v: f64, (lo, hi): (f64, f64)) -> f64 {
    match (v.total_cmp(&lo).is_le(), v.total_cmp(&hi).is_ge()) {
        (true, _) => lo,
        (false, true) => hi,
        (false, false) => v,
    }
}

/// Box that fills its (explicitly sized) anchors parent.
pub(crate) fn fill_parent() -> Style {
    let mut s = Style::column().anchored(AnchorH::Start, AnchorV::Start, Vec2::ZERO);
    s.layout.size = tf::Size {
        width: Percent(100.0).into(),
        height: Percent(100.0).into(),
    };
    s
}

/// Container whose children are anchored inside a fixed box.
pub(crate) fn anchors_box(width: f64, height: f64) -> Style {
    Style::anchors().sized(width, height)
}

pub(crate) fn pad(s: Style, x: f64, y: f64) -> Style {
    s.padded_xy(x, y)
}

/// Theme sizes are `f64` logical px; cosmic-text metrics are `f32`.
pub(crate) fn ink(size: f64, color: Color, weight: Weight) -> TextStyle {
    TextStyle::new(size as f32, color).weight(weight)
}

/// Digits that do not jitter (timers, gold, quantities).
pub(crate) fn tabular(size: f64, color: Color, weight: Weight) -> TextStyle {
    ink(size, color, weight).numerals(Numerals::Tabular)
}

/// Shrink-wrapped `child` pinned to a corner/edge of an anchors container, nudged by `offset`.
pub(crate) fn pinned<Msg>(
    h: AnchorH,
    v: AnchorV,
    offset: Vec2,
    child: kit_view::Node<Msg>,
) -> kit_view::Node<Msg> {
    kit_view::stack(Style::column().anchored(h, v, offset), vec![child])
}

/// What pressing a control does: send `Msg`, or nothing because it is disabled. Disabled is a kind, not a flag beside
/// the message, so a component never holds a message it must remember not to send.
#[derive(Clone, Debug)]
pub enum Press<Msg> {
    Sends(Msg),
    Disabled,
}

/// The control node for a [`Press`]: enabled and bound, or visible, inert and skipped by hits and focus.
pub fn pressable<Msg>(
    id: impl Into<kit_core::Identity>,
    press: Press<Msg>,
    a11y: kit_view::A11y,
    face: kit_view::Node<Msg>,
) -> kit_view::Node<Msg> {
    match press {
        Press::Sends(msg) => {
            kit_view::interactive(id, kit_view::Bindings::activate(msg), a11y, face)
        }
        Press::Disabled => kit_view::disabled(id, a11y, face),
    }
}
