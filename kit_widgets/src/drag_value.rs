// Ported from egui 0.36.2 `crates/egui/src/widgets/drag_value.rs` (MIT OR Apache-2.0, (c) Emil Ernerfeldt, Rerun Technologies).
// Changes: kit state, intents and theme tokens replace egui's Ui/Response/Memory/epaint.
//! Controlled numeric drag. Dragging right or up raises the value by `speed` per logical px, measured from the press
//! (egui keeps the unclamped precise value, so dragging back past a clamp returns at once). A click (press and release
//! without leaving the slop circle) opens a focused text field (a focus claim, no host help): Enter or leaving the
//! field commits, Escape cancels, and invalid text reports a typed error.
use crate::common::{applied, clamp, ink, ordered, surface};
use kit_core::kurbo::Point;
use kit_core::{Border, Shadow, Theme, WidgetId};
use kit_state::{Act, Applied, Cx, Host, Ident, Intent};
use kit_view::{A11y, Bindings, Edit, EditorState, Node, PointerAt, PointerPhase as Ph, Role};
use kit_view::{Style, Travel, Weight, interactive, panel, text, text_field};

pub struct DragValue<'a, K> {
    pub theme: &'a Theme,
    pub key: K,
    pub label: &'a str,
    pub value: f64,
    /// Value change per logical pixel. Must be positive and finite.
    pub speed: f64,
    /// NaN bounds panic; reversed bounds are supported, as in egui.
    pub range: std::ops::RangeInclusive<f64>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TypedError {
    InvalidNumber,
    NotFinite,
    StaleValue,
}
#[derive(Clone, Debug, PartialEq)]
pub struct DragValueChanged<K> {
    pub key: K,
    pub value: Result<f64, TypedError>,
}
#[derive(Clone)]
enum Mode {
    Button,
    /// A press is held: `at` and `from` are where it began and the value then.
    Held {
        at: Point,
        from: f64,
    },
    Editing {
        editor: EditorState,
        original: f64,
    },
}
#[derive(Clone, Debug, PartialEq)]
enum Input {
    Pointer(PointerAt),
    Edit(Edit),
}
/// The input with the props the reducer needs (it cannot capture them): value, speed and the ordered bounds.
#[derive(Clone, Debug, PartialEq)]
struct Event(Input, f64, f64, (f64, f64));

/// egui's parser: whitespace ignored, Unicode minus accepted; user edits clamp. Text typed against a value that has
/// changed since is refused (egui ignores it).
fn parse(text: &str, original: f64, Event(_, value, _, range): &Event) -> Result<f64, TypedError> {
    let minus = |c| if c == '−' { '-' } else { c };
    let text: String = text
        .chars()
        .filter(|c| !c.is_whitespace())
        .map(minus)
        .collect();
    match (original == *value, text.parse::<f64>()) {
        (false, _) => Err(TypedError::StaleValue),
        (true, Ok(v)) if v.is_finite() => Ok(clamp(v, *range)),
        (true, Ok(_)) => Err(TypedError::NotFinite),
        (true, Err(_)) => Err(TypedError::InvalidNumber),
    }
}

/// Panics for a nonfinite value, a nonpositive/nonfinite speed, or NaN range bounds.
pub fn drag_value<A: Host, K: Ident<A>>(cx: &Cx<'_, A>, p: DragValue<'_, K>) -> Node<Intent<A>>
where
    A::Ask: From<DragValueChanged<K>>,
{
    let range = ordered(&p.range);
    let ok = p.value.is_finite() && p.speed.is_finite() && p.speed > 0.0;
    assert!(
        ok,
        "drag_value: value must be finite, speed positive and finite"
    );
    let mode = cx.state(
        &p.key,
        || Mode::Button,
        |mode: &mut Mode, key: &K, ev: Event, _: &A| {
            let report = |value| {
                Act::ask(
                    DragValueChanged {
                        key: key.clone(),
                        value,
                    }
                    .into(),
                )
            };
            let Event(input, value, speed, range) = ev.clone();
            match (input, &mut *mode) {
                (Input::Pointer(p), mode) => match (p.phase, p.travel, &*mode) {
                    (Ph::Press, _, _) => {
                        *mode = Mode::Held {
                            at: p.local,
                            from: value,
                        };
                        Act::none()
                    }
                    // egui: right and up increase; an unbounded range that overflows is a typed error.
                    (Ph::Drag, _, Mode::Held { at, from }) => {
                        match clamp(
                            from + (p.local.x - at.x - (p.local.y - at.y)) * speed,
                            range,
                        ) {
                            v if v.is_finite() => report(Ok(v)),
                            _ => report(Err(TypedError::NotFinite)),
                        }
                    }
                    (Ph::Drag, _, Mode::Button | Mode::Editing { .. }) => {
                        Act::fault("drag_value: drag without a press")
                    }
                    // egui `response.clicked()`: edit the value as text, all of it selected.
                    (Ph::Release, Travel::Still, _) => {
                        let mut editor = EditorState::single_line(value.to_string());
                        assert!(matches!(editor.apply(Edit::SelectAll), Applied::Ok(_)));
                        *mode = Mode::Editing {
                            editor,
                            original: value,
                        };
                        Act::none()
                    }
                    (Ph::Release, Travel::Moved, _) | (Ph::Cancel, _, _) => {
                        *mode = Mode::Button;
                        Act::none()
                    }
                },
                // Escape cancels; the Blur that follows it (and focus news outside text mode) changes nothing.
                (Input::Edit(Edit::Escape), _) => {
                    *mode = Mode::Button;
                    Act::none()
                }
                (Input::Edit(Edit::Focus | Edit::Blur), Mode::Button | Mode::Held { .. }) => {
                    Act::none()
                }
                (Input::Edit(_), Mode::Button | Mode::Held { .. }) => {
                    Act::fault("drag_value: edit outside text mode")
                }
                // Enter keeps invalid text for correction; leaving the field (egui `lost_focus`) ends editing.
                (
                    Input::Edit(e @ (Edit::Submit | Edit::Blur)),
                    Mode::Editing { editor, original },
                ) => {
                    let parsed = parse(editor.text(), *original, &ev);
                    if parsed.is_ok() || e == Edit::Blur {
                        *mode = Mode::Button;
                    }
                    report(parsed)
                }
                (Input::Edit(edit), Mode::Editing { editor, .. }) => applied(editor.apply(edit)),
            }
        },
    );
    let (c, th, id, sender) = (
        &p.theme.colors,
        p.theme,
        WidgetId::of(&p.key),
        mode.sender(),
    );
    let (value, speed) = (p.value, p.speed);
    let with = move |input| sender.send(Event(input, value, speed, range));
    let style = ink(th.type_scale.body, c.text, Weight::NORMAL);
    match &*mode {
        Mode::Button | Mode::Held { .. } => {
            let border = Border::Solid {
                width: th.spacing.xs / 4.0,
                color: c.field_border,
            };
            let frame = surface(c.field_fill, th.radii.control, border, Shadow::None);
            let padded = Style::row().padded_xy(th.spacing.md, th.spacing.sm);
            let face = panel(padded, frame, vec![text(p.value.to_string(), style)]);
            let on = Bindings::none().on_pointer(move |at| with(Input::Pointer(at)));
            interactive(id, on, A11y::named(Role::Button, p.label), face)
        }
        Mode::Editing { editor, .. } => text_field(id, p.label, editor, style, move |edit| {
            with(Input::Edit(edit))
        })
        .claim_focus(&())
        .pad_xy(th.spacing.md, th.spacing.sm),
    }
}
