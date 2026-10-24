// Ported from egui 0.36.2 `crates/egui/src/widgets/text_edit/builder.rs` (MIT OR Apache-2.0, (c) Emil Ernerfeldt, Rerun Technologies).
// Changes: kit state, intents and theme tokens replace egui's Ui/Response/Memory/epaint.
//! Search field: a text field that asks [`SearchChanged`] once the text has been still for `quiet` (a kit wake-up,
//! re-armed by every change; caret moves do not postpone it). Escape or the clear button empties it.
use crate::common::{applied, ink, solid};
use crate::text_field::{Shape, build};
use kit_core::kurbo::{BezPath, Circle, Rect, Shape as _, Stroke};
use kit_core::{ShapeSpec, Theme, WidgetId};
use kit_state::{Act, Cx, Dir, Host, Ident, Intent};
use kit_view::taffy::AlignItems;
use kit_view::{A11y, Availability, Bindings, Edit, EditorState, Node, Paint, Role, Style, Unit};
use kit_view::{Weight, interactive, nothing, row, shape, text};
use std::time::Duration;

pub struct SearchField<'a, K> {
    pub theme: &'a Theme,
    pub key: K,
    pub label: &'a str,
    pub placeholder: &'a str,
    /// How long the text must stay unchanged before it is asked for.
    pub quiet: Duration,
}
#[derive(Clone, Debug, PartialEq)]
pub struct SearchChanged<K> {
    pub key: K,
    pub text: String,
}
#[derive(Clone, Debug, PartialEq)]
enum In {
    Edit(Edit),
    Clear,
    Settled,
}

pub fn search_field<A: Host, K: Ident<A>>(cx: &Cx<'_, A>, p: SearchField<'_, K>) -> Node<Intent<A>>
where
    A::Ask: From<SearchChanged<K>>,
{
    let s = cx.state(
        &p.key,
        || EditorState::single_line(""),
        |s: &mut EditorState, key: &K, (ev, quiet): (In, Duration), app: &A| {
            let before = s.text().to_owned();
            let act = match ev {
                In::Settled => Act::ask(
                    SearchChanged {
                        key: key.clone(),
                        text: before.clone(),
                    }
                    .into(),
                ),
                In::Edit(Edit::Escape) | In::Clear => applied(s.apply(Edit::SelectAll))
                    .and(applied(s.apply(Edit::Delete(Dir::Back, Unit::Grapheme)))),
                In::Edit(edit) => applied(s.apply(edit)),
            };
            match s.text() == before {
                true => act,
                false => act.and(Act::wake(
                    kit_core::Time(app.now().0 + quiet),
                    (In::Settled, quiet),
                )),
            }
        },
    );
    let (th, sender, quiet) = (p.theme, s.sender(), p.quiet);
    let clear = match s.text().is_empty() {
        true => nothing(),
        false => {
            let name = A11y::named(Role::Button, format!("Clear {}", p.label));
            let on = Bindings::activate(s.send((In::Clear, quiet)));
            let face = text("×", ink(th.type_scale.body, th.colors.text, Weight::NORMAL));
            interactive(
                WidgetId::of(&(&p.key, "clear")),
                on,
                name,
                face.pad(th.spacing.xs),
            )
        }
    };
    // Normalized magnifier geometry; the theme's body size sets its logical-pixel dimensions.
    let mut path: BezPath = Circle::new((0.4, 0.4), 0.3).to_path(0.01);
    path.move_to((0.62, 0.62));
    path.line_to((1.0, 1.0));
    let (view_box, stroke) = (Rect::new(0.0, 0.0, 1.0, 1.0), Stroke::new(0.08));
    let paint = Paint::Stroke {
        brush: solid(th.colors.text_muted),
        stroke,
    };
    let size = Style::row().sized(th.type_scale.body, th.type_scale.body);
    let icon = shape(size, ShapeSpec::Path { path, view_box }, paint);
    let shape = Shape {
        theme: th,
        key: &p.key,
        label: p.label,
        placeholder: p.placeholder,
    };
    let edit = move |e| sender.send((In::Edit(e), quiet));
    build(shape, &s, Availability::Enabled, edit, |field| {
        row([icon, field.grow(1.0), clear])
            .gap(th.spacing.sm)
            .align_items(AlignItems::CENTER)
    })
}
