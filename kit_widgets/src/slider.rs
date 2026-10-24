// Ported from egui 0.36.2 `crates/egui/src/widgets/slider.rs` (MIT OR Apache-2.0, (c) Emil Ernerfeldt, Rerun Technologies).
// Changes: kit state, intents and theme tokens replace egui's Ui/Response/Memory/epaint.
//! Controlled horizontal linear slider. A press anywhere on the track jumps the value there; the pointer stays
//! captured, so a drag beyond the track clamps to the range end. While focused, ArrowLeft/ArrowRight move it by one
//! step (or one logical px of travel when continuous), as egui's `kb_step`. Not ported: logarithmic scale, smart aim,
//! vertical orientation and the DragValue beside it.
use crate::common::{clamp, ink, ordered, solid, surface};
use core::hash::Hash;
use core::ops::RangeInclusive;
use kit_core::kurbo::Vec2;
use kit_core::{Border, Key, NamedKey, Shadow, ShapeSpec, Theme, WidgetId};
use kit_view::taffy::AlignItems;
use kit_view::{A11y, AnchorH, AnchorV, Bindings, KeyAnswer, KeyPress, Node, Paint, PointerAt};
use kit_view::{RangeValue, Style, Weight, interactive, panel, row, shape, stack, text};
use std::rc::Rc;

/// egui `Spacing::slider_width` and `slider_rail_height`.
const WIDTH: f64 = 100.0;
const RAIL: f64 = 8.0;

/// egui's `step_by`: `Continuous` is egui's `None` (step 0).
#[derive(Copy, Clone, PartialEq, Debug)]
pub enum Step {
    Continuous,
    By(f64),
}

pub struct Slider<'a, K, F> {
    pub theme: &'a Theme,
    pub key: K,
    pub label: &'a str,
    /// Shown clamped to `range` (egui `SliderClamping::Always`).
    pub value: f64,
    /// Finite bounds; a reversed range puts `start` at the left end, as in egui.
    pub range: RangeInclusive<f64>,
    pub step: Step,
    /// Every pointer phase reports the value under the pointer (Cancel: the last one; egui has no revert), and every
    /// arrow key the value it moves to.
    pub on_change: F,
}

/// egui `set_value`: clamp, then snap from `start` (so 0..=10 by 3 ends at 9); the snap can overshoot an end
/// (0..=10 by 4 gives 12), which egui's `get_value` clamps on the next frame, so this clamps again at once.
fn set_value(value: f64, range: &RangeInclusive<f64>, step: Step) -> f64 {
    let value = clamp(value, ordered(range));
    match step {
        Step::Continuous => value,
        Step::By(step) => {
            let start = *range.start();
            clamp(
                start + ((value - start) / step).round() * step,
                ordered(range),
            )
        }
    }
}

/// egui `value_from_position` + `set_value` for a linear slider `width` wide whose handle has radius `radius`:
/// the handle centre travels `radius..width - radius`.
pub fn value_at(x: f64, width: f64, radius: f64, range: &RangeInclusive<f64>, step: Step) -> f64 {
    let t = (x - radius) / (width - 2.0 * radius);
    let (start, end) = (*range.start(), *range.end());
    set_value(start + (end - start) * t, range, step)
}

/// Panics on a nonfinite bound or a step that is not positive and finite (egui would draw NaN).
pub fn slider<Msg: 'static, K: Hash, F>(p: Slider<'_, K, F>) -> Node<Msg>
where
    F: Fn(f64) -> Msg + 'static,
{
    let (range, step) = (p.range.clone(), p.step);
    assert!(
        range.start().is_finite() && range.end().is_finite(),
        "slider: range bounds must be finite"
    );
    let decimals = match step {
        Step::Continuous => 2,
        Step::By(s) => {
            assert!(
                s.is_finite() && s > 0.0,
                "slider: step must be positive and finite"
            );
            (-s.log10()).ceil().max(0.0) as usize
        }
    };
    let (c, th) = (&p.theme.colors, p.theme);
    let h = th.controls.mini.1;
    let radius = h / 2.5; // egui `handle_radius`: rect height / 2.5.
    let value = clamp(p.value, ordered(&range));
    let (start, end) = (*range.start(), *range.end());
    let t = match start == end {
        true => 0.5, // egui `normalized_from_value`: an empty range shows the centre.
        false => (value - start) / (end - start),
    };
    let at = |x: f64| Vec2::new(x, 0.0);
    let centred = |w, h, x| {
        Style::row()
            .sized(w, h)
            .anchored(AnchorH::Start, AnchorV::Center, at(x))
    };
    let rail = panel(
        centred(WIDTH, RAIL, 0.0),
        surface(c.field_fill, RAIL / 2.0, Border::None, Shadow::None),
        vec![],
    );
    // The handle centre travels radius..WIDTH - radius; the knob box starts one radius left of it.
    let knob = shape(
        centred(2.0 * radius, 2.0 * radius, t * (WIDTH - 2.0 * radius)),
        ShapeSpec::Ellipse,
        Paint::Fill(solid(c.accent)),
    );
    let track = stack(Style::anchors().sized(WIDTH, h), vec![rail, knob]);
    let (on_change, keys) = (Rc::new(p.on_change), range.clone());
    let changed = Rc::clone(&on_change);
    // egui `kb_step`: a step, or one logical px of handle travel from where the value is shown.
    let nudge = move |by: f64| match step {
        Step::By(s) => changed(set_value(value + by * s, &keys, step)),
        Step::Continuous => changed(value_at(
            radius + t * (WIDTH - 2.0 * radius) + by,
            WIDTH,
            radius,
            &keys,
            step,
        )),
    };
    let on = Bindings::none()
        .on_pointer(move |p: PointerAt| {
            let radius = p.size.height / 2.5;
            on_change(value_at(p.local.x, p.size.width, radius, &range, step))
        })
        .on_key(move |k: KeyPress| match k.key {
            Key::Named(NamedKey::ArrowLeft) => KeyAnswer::Handled(nudge(-1.0)),
            Key::Named(NamedKey::ArrowRight) => KeyAnswer::Handled(nudge(1.0)),
            Key::Named(_) | Key::Character(_) => KeyAnswer::Pass,
        });
    let (min, max) = ordered(&p.range);
    let a11y = A11y::Ranged {
        label: p.label.to_owned(),
        value: RangeValue {
            now: value,
            min,
            max,
        },
    };
    let caption = format!("{value:.decimals$} {}", p.label);
    row([
        interactive(WidgetId::of(&p.key), on, a11y, track),
        text(caption, ink(th.type_scale.label, c.text, Weight::NORMAL)),
    ])
    .gap(th.spacing.sm)
    .align_items(AlignItems::CENTER)
}
