// Ported from egui 0.36.2 `crates/egui/src/widgets/text_edit/builder.rs` (MIT OR Apache-2.0, (c) Emil Ernerfeldt, Rerun Technologies).
// Changes: kit state, intents and theme tokens replace egui's Ui/Response/Memory/epaint.
//! `text_field` (one line, Enter submits) and `text_area` (wraps, grows to `max_rows` then scrolls), each with a
//! visible label and egui's `hint_text` placeholder. The text lives in the field's own keyed `EditorState`; keys,
//! IME, clipboard and pointer arrive as `Edit`s from the router. An edit computed against an older revision (a paste
//! that lands after more typing) is refused and counted as a fault, never applied to text it was not meant for.
//! Enter asks the host with [`Submitted`], built in the reducer, so it carries the text as of the Enter even when
//! several edits arrive in one frame.
use crate::common::{applied, ink, surface};
use kit_core::{Axis, Border, Shadow, Theme, WidgetId};
use kit_state::{Act, Cx, Host, Ident, Intent};
use kit_view::{A11y, Availability, Corner, EditorState, FIELD_INSET, Node, Role, ScrollSpec};
use kit_view::{Style, Toggle, Weight, column, disabled, overlay, panel, px};
use kit_view::{scroll_area, text, text_field as field_node};

/// egui `spacing.text_edit_width`, logical px. Restyle the returned node to size it otherwise.
const WIDTH: f64 = 280.0;
/// Line height over font size: `kit_text::TextStyle::new`'s `Metrics::relative(size, 1.25)`.
const LINE: f64 = 1.25;

/// What Enter in a [`text_field`] asks the host: the app implements `From<Submitted<K>> for Ask`.
#[derive(Clone, PartialEq, Debug)]
pub struct Submitted<K> {
    pub key: K,
    pub text: String,
}

pub struct TextField<'a, K> {
    pub theme: &'a Theme,
    pub key: K,
    pub label: &'a str,
    pub placeholder: &'a str,
    pub availability: Availability,
}

pub struct TextArea<'a, K> {
    pub theme: &'a Theme,
    pub key: K,
    pub label: &'a str,
    pub placeholder: &'a str,
    pub availability: Availability,
    /// Rows shown before the area scrolls instead of growing.
    pub max_rows: u16,
}

pub fn text_field<A: Host, K: Ident<A>>(cx: &Cx<'_, A>, p: TextField<'_, K>) -> Node<Intent<A>>
where
    A::Ask: From<Submitted<K>>,
{
    let state = cx.state(
        &p.key,
        || EditorState::single_line(""),
        |s: &mut EditorState, key: &K, edit: kit_view::Edit, _: &A| match edit {
            kit_view::Edit::Submit => Act::ask(
                Submitted {
                    key: key.clone(),
                    text: s.text().to_owned(),
                }
                .into(),
            ),
            // Every other edit is the editor's: `EditorState::apply` matches them exhaustively.
            edit => applied(s.apply(edit)),
        },
    );
    let sender = state.sender();
    let shape = Shape {
        theme: p.theme,
        key: &p.key,
        label: p.label,
        placeholder: p.placeholder,
    };
    build(
        shape,
        &state,
        p.availability,
        move |e| sender.send(e),
        |field| field,
    )
}

pub fn text_area<A: Host, K: Ident<A>>(cx: &Cx<'_, A>, p: TextArea<'_, K>) -> Node<Intent<A>> {
    let state = cx.state(
        &p.key,
        || EditorState::multi_line(""),
        |s: &mut EditorState, _: &K, edit: kit_view::Edit, _: &A| applied(s.apply(edit)),
    );
    let sender = state.sender();
    // Grows with its text up to `max_rows` lines; past that the area scrolls and follows the end while typing there.
    let line = p.theme.type_scale.body * LINE;
    let mut viewport = Style::column();
    viewport.layout.max_size.height = px(line * f64::from(p.max_rows) + 2.0 * FIELD_INSET.y);
    let id = WidgetId::of(&(&p.key, "rows"));
    let shape = Shape {
        theme: p.theme,
        key: &p.key,
        label: p.label,
        placeholder: p.placeholder,
    };
    let spec = ScrollSpec::new(Axis::Vertical).stick_to_end();
    build(
        shape,
        &state,
        p.availability,
        move |e| sender.send(e),
        |field| scroll_area(id, spec, viewport, field),
    )
}

/// What both kinds of field look like (and the search field, which wraps the field in its icon row).
pub(crate) struct Shape<'a, K> {
    pub theme: &'a Theme,
    pub key: &'a K,
    pub label: &'a str,
    pub placeholder: &'a str,
}

/// The caption above the field, egui's `hint_text` over it while empty, and `frame` around it.
pub(crate) fn build<A: Host, K: Ident<A>>(
    p: Shape<'_, K>,
    state: &EditorState,
    availability: Availability,
    edit: impl Fn(kit_view::Edit) -> Intent<A> + 'static,
    frame: impl FnOnce(Node<Intent<A>>) -> Node<Intent<A>>,
) -> Node<Intent<A>> {
    let (c, th) = (&p.theme.colors, p.theme);
    let style = |color| ink(th.type_scale.body, color, Weight::NORMAL);
    let id = WidgetId::of(p.key);
    let mut least = Style::row();
    least.layout.min_size.height = px(th.type_scale.body * LINE + 2.0 * FIELD_INSET.y);
    let body: Node<Intent<A>> = match availability {
        Availability::Enabled => {
            let node = field_node(id, p.label, state, style(c.text), edit).styled(least);
            // egui `hint_text`: shown over an empty field, never part of its value.
            let hint =
                text(p.placeholder, style(c.text_muted)).pad_xy(FIELD_INSET.x, FIELD_INSET.y);
            match state.text().is_empty() {
                true => frame(overlay(node, Corner::TopLeft, hint)),
                false => frame(node),
            }
        }
        // Not editable at all: the value as plain text in the field's box, announced as a disabled text box.
        Availability::Disabled => {
            let border = Border::Solid {
                width: 1.0,
                color: c.field_border,
            };
            let boxed = panel(
                least.padded_xy(FIELD_INSET.x, FIELD_INSET.y),
                surface(c.field_fill, th.radii.sm, border, Shadow::None),
                vec![text(state.text(), style(c.text_disabled))],
            );
            let a11y = A11y::Labeled {
                role: Role::TextBox,
                label: p.label.to_string(),
                toggle: Toggle::NotToggle,
            };
            disabled(id, a11y, boxed)
        }
    };
    let caption = text(
        p.label,
        ink(th.type_scale.label, c.text_muted, Weight::NORMAL),
    );
    column([caption, body.fill_width()])
        .gap(th.spacing.xs)
        .width(px::<kit_view::taffy::Dimension>(WIDTH))
}
