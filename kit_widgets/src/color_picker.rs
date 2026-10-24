// Ported from egui 0.36.2 `crates/egui/src/widgets/color_picker.rs` (MIT OR Apache-2.0, (c) Emil Ernerfeldt, Rerun Technologies).
// Changes: kit state, intents and theme tokens replace egui's Ui/Response/Memory/epaint.
//! Opaque sRGB picker. HSV is controlled, so hue survives gray. Pressing or dragging the saturation/value square or
//! the hue bar picks continuously and clamps at their edges (the pointer stays captured beyond them), as in egui.
use crate::common::{ink, surface};
use kit_core::kurbo::Vec2;
use kit_core::peniko::Color;
use kit_core::{Border, Shadow, Theme, WidgetId};
use kit_state::{Act, Applied, Cx, Host, Ident, Intent};
use kit_view::{A11y, Bindings, Edit, EditorState, Layer, Node, PortalAnchor, Role, Side, Style};
use kit_view::{Toggle, Weight, column, dismissible_from, interactive, nothing, panel, row, text};

/// Hue, saturation, value, each finite and in 0..=1. Invalid inputs to conversion/build panic.
pub type Hsv = [f32; 3];
fn check(values: &[f32]) {
    let ok = values
        .iter()
        .all(|v| v.is_finite() && (0.0..=1.0).contains(v));
    assert!(ok, "color_picker: expected finite unit components");
}
pub fn hsv_to_rgb([h, s, v]: Hsv) -> [f32; 3] {
    check(&[h, s, v]);
    // Six HSV sectors; modulo wraps h=1 to red, without quantizing any channel.
    let channel = |offset: f32| {
        let k = (offset + h * 6.0) % 6.0;
        v * (1.0 - s * k.min(4.0 - k).clamp(0.0, 1.0))
    };
    [channel(5.0), channel(3.0), channel(1.0)]
}
pub fn rgb_to_hsv([r, g, b]: [f32; 3]) -> Hsv {
    check(&[r, g, b]);
    let (min, max) = (r.min(g).min(b), r.max(g).max(b));
    let delta = max - min;
    if delta == 0.0 {
        return [0.0, 0.0, max];
    }
    let sector = if max == r {
        (g - b) / delta
    } else if max == g {
        2.0 + (b - r) / delta
    } else {
        4.0 + (r - g) / delta
    };
    [sector.rem_euclid(6.0) / 6.0, delta / max, max]
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HexError {
    Length,
    Digit,
}
/// Exactly six hexadecimal digits, optionally prefixed with '#'; whitespace is an error.
pub fn parse_hex(value: &str) -> Result<Hsv, HexError> {
    let digits = value.strip_prefix('#').unwrap_or(value);
    match (digits.len(), digits.bytes().all(|b| b.is_ascii_hexdigit())) {
        (6, true) => {
            let n = u32::from_str_radix(digits, 16).map_err(|_| HexError::Digit)?;
            Ok(rgb_to_hsv(
                [16, 8, 0].map(|shift| ((n >> shift) & 255) as f32 / 255.0),
            ))
        }
        (6, false) => Err(HexError::Digit),
        (_, _) => Err(HexError::Length),
    }
}
fn color(hsv: Hsv) -> Color {
    let [r, g, b] = hsv_to_rgb(hsv);
    Color::new([r, g, b, 1.0])
}
fn hex(hsv: Hsv) -> String {
    let [r, g, b] = hsv_to_rgb(hsv).map(|c| (c * 255.0).round() as u8);
    format!("#{r:02X}{g:02X}{b:02X}")
}
pub struct ColorPicker<'a, K> {
    pub theme: &'a Theme,
    pub key: K,
    pub label: &'a str,
    pub value: Hsv,
}
#[derive(Clone, Debug, PartialEq)]
pub struct ColorChanged<K> {
    pub key: K,
    pub value: Result<Hsv, HexError>,
}
#[derive(Clone, Copy, PartialEq)]
enum Open {
    Closed,
    Open,
}
#[derive(Clone, Debug, PartialEq)]
enum Ev {
    Toggle(Hsv),
    Close,
    Set(Hsv),
    Edit(Edit),
}
#[derive(Clone)]
struct Picker {
    open: Open,
    editor: EditorState,
}
/// egui's color-slider resolution (multiple of six, preserving primary/secondary hues).
const N: usize = 36;
// egui color_edit_button_hsva's COLOR_SLIDER_WIDTH.
const WIDTH: f64 = 275.0;
/// A pointer fraction (unclamped) as a unit component: egui `remap_clamp` to the rect.
fn unit(f: f64) -> f32 {
    f.clamp(0.0, 1.0) as f32
}
pub fn color_picker<A: Host, K: Ident<A>>(cx: &Cx<'_, A>, p: ColorPicker<'_, K>) -> Node<Intent<A>>
where
    A::Ask: From<ColorChanged<K>>,
{
    let (th, c, id) = (p.theme, &p.theme.colors, WidgetId::of(&p.key));
    let init = || Picker {
        open: Open::Closed,
        editor: EditorState::single_line(hex(p.value)),
    };
    let state = cx.state(&p.key, init, |s: &mut Picker, key: &K, ev: Ev, _: &A| {
        let report = |value| {
            Act::ask(
                ColorChanged {
                    key: key.clone(),
                    value,
                }
                .into(),
            )
        };
        match ev {
            Ev::Toggle(value) => {
                let open = match s.open {
                    Open::Closed => Open::Open,
                    Open::Open => Open::Closed,
                };
                *s = Picker {
                    open,
                    editor: EditorState::single_line(hex(value)),
                };
                Act::none()
            }
            Ev::Close => {
                s.open = Open::Closed;
                Act::none()
            }
            Ev::Set(value) => {
                s.editor = EditorState::single_line(hex(value));
                report(Ok(value))
            }
            Ev::Edit(Edit::Submit) => report(parse_hex(s.editor.text())),
            Ev::Edit(edit) => match s.editor.apply(edit) {
                Applied::Ok(_) => Act::none(),
                Applied::Stale { .. } => Act::fault("color_picker: stale edit refused"),
                Applied::Misplaced => Act::fault("color_picker: misplaced edit refused"),
            },
        }
    });
    let border = Border::Solid {
        width: th.spacing.xs / 2.0,
        color: c.field_border,
    };
    let swatch = panel(
        Style::row().sized(th.controls.mini.0, th.controls.mini.1),
        surface(color(p.value), th.radii.sm, border, Shadow::None),
        vec![],
    );
    let caption = text(p.label, ink(th.type_scale.label, c.text, Weight::NORMAL));
    let toggle = match state.open {
        Open::Closed => Toggle::Off,
        Open::Open => Toggle::On,
    };
    let a11y = A11y::Labeled {
        role: Role::Button,
        label: p.label.into(),
        toggle,
    };
    let face = row([swatch, caption]).gap(th.spacing.md).pad(th.spacing.sm);
    let button = interactive(
        id,
        Bindings::activate(state.send(Ev::Toggle(p.value))),
        a11y,
        face,
    );
    let popup = match state.open {
        Open::Closed => nothing(),
        Open::Open => {
            // Solid samples: neither backend draws gradients. This square is the first use case for a future
            // `Material` draw kind (a shaded quad); until then, egui's 36 subdivisions as pixel-aligned cells
            // (fractional widths leave dark antialias seams between neighbours).
            let step = (WIDTH / N as f64).round();
            let at = |i: usize| i as f32 / (N - 1) as f32;
            let near = |a: f32, b: f32| (a - b).abs() <= 0.5 / (N - 1) as f32;
            let cell = |shown: Hsv, picked: bool, height| {
                let edge = match picked {
                    true => Border::Solid {
                        width: th.spacing.xs,
                        color: c.focus_ring,
                    },
                    false => Border::None,
                };
                panel(
                    Style::row().sized(step, height),
                    surface(color(shown), 0.0, edge, Shadow::None),
                    vec![],
                )
            };
            let [h, s, v] = p.value;
            let square = column((0..N).map(|y| {
                let cells = (0..N).map(|x| (at(x), 1.0 - at(y)));
                row(cells.map(|(cs, cv)| cell([h, cs, cv], near(cs, s) && near(cv, v), step)))
            }));
            let hue =
                row((0..N).map(|x| cell([at(x), 1.0, 1.0], near(at(x), h), th.controls.mini.1)));
            // Saturation increases rightward and value upward; the hue bar keeps saturation and value.
            let region = |name: &str, pick: fn(Hsv, Vec2) -> Hsv, child| {
                let (sender, value) = (state.sender(), p.value);
                let on = Bindings::none()
                    .on_pointer(move |at| sender.send(Ev::Set(pick(value, at.fraction()))));
                let a11y = A11y::Labeled {
                    role: Role::Bar,
                    label: name.into(),
                    toggle: Toggle::NotToggle,
                };
                interactive(id.child(name), on, a11y, child)
            };
            let sender = state.sender();
            let field = kit_view::text_field(
                WidgetId::of(&(&p.key, "hex")),
                "Hex",
                &state.editor,
                ink(th.type_scale.body, c.text, Weight::NORMAL),
                move |edit| sender.send(Ev::Edit(edit)),
            );
            let body = column([
                region(
                    "Saturation/value",
                    |[h, ..], f| [h, unit(f.x), 1.0 - unit(f.y)],
                    square,
                ),
                region("Hue", |[_, s, v], f| [unit(f.x), s, v], hue),
                field,
            ])
            .gap(th.spacing.md);
            let frame = surface(c.surface_raised, th.radii.md, border, Shadow::None);
            let body = panel(Style::column().padded(th.spacing.md), frame, vec![body]);
            let anchor = PortalAnchor::Node {
                id,
                side: Side::Below,
                gap: th.spacing.xs,
            };
            dismissible_from(id, Layer::Popup, anchor, state.send(Ev::Close), body)
        }
    };
    column([button, popup])
}
