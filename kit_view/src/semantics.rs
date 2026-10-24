//! The accessibility projection of a laid-out screen: one [`Semantic`] per labelled interactive node, in paint order.
//!
//! The kit paints pixels, so a screen reader, browser find and an audit driver see nothing unless the host mirrors
//! this list into its platform tree (host_web keeps an invisible DOM layer; native hosts would feed AccessKit). The
//! projection is pure data derived from a `LaidOut`; it has no opinion about the platform.
//!
//! [`semantics`] gives layout rects (children of a `Scroll` at scroll offset zero). A host mirroring what is ON SCREEN
//! uses [`semantics_scrolled`] with the router's offsets: rects are moved by every enclosing scroll offset and clipped
//! to every enclosing scroll viewport, and a control scrolled wholly out of view is omitted. Regression (kit flip,
//! 2026-10-23, `inventory` stage): the browser mirror kept a scrolled shop's controls at their unscrolled places, so a
//! mirror-driven click or a screen reader aimed at stale positions.

use crate::{
    A11y, Availability, LaidKind, LaidNode, LaidOut, RangeValue, Role, Shortcut, Toggle, Tooltip,
};
use kit_core::WidgetId;
use kit_core::kurbo::{Rect, Vec2};

/// What activating the control does right now.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum SemanticState {
    /// A press activates it.
    Enabled,
    /// Visible but transparent to presses.
    Disabled,
    /// Takes presses (so they never reach the world behind it) but binds no message: an information tile.
    Inert,
}

/// The value a control announces. Absence is named: most controls have none.
#[derive(Copy, Clone, PartialEq, Debug)]
pub enum SemanticValue {
    None,
    Range(RangeValue),
}

/// What a control says beyond its name (its tooltip): `aria-description` on the web. Absence is named.
#[derive(Clone, PartialEq, Debug)]
pub enum Description {
    None,
    Text(String),
}

#[derive(Clone, PartialEq, Debug)]
pub struct Semantic {
    pub id: WidgetId,
    pub role: Role,
    pub label: String,
    pub description: Description,
    pub state: SemanticState,
    /// `aria-pressed`: whether this is a toggle and which way it is set.
    pub toggle: Toggle,
    /// `aria-valuenow/min/max` of a ranged control.
    pub value: SemanticValue,
    /// `aria-keyshortcuts`: the key that activates it.
    pub shortcut: Shortcut,
    /// Absolute logical px.
    pub rect: Rect,
}

/// Every labelled interactive node of `laid`, pre-order (later entries paint above earlier ones). `A11y::Hidden`
/// nodes are decorative and omitted.
pub fn semantics<Msg>(laid: &LaidOut<Msg>) -> Vec<Semantic> {
    laid.root
        .descendants()
        .into_iter()
        .filter_map(semantic_of)
        .collect()
}

/// Every labelled interactive node of `laid` as it is on screen: `offset(id)` is the scroll offset of the `Scroll` node
/// `id` (the router's `UiMemory::scroll_offset`). Pre-order like [`semantics`]; controls scrolled out of view are
/// omitted, partly visible ones carry their visible part.
pub fn semantics_scrolled<Msg>(
    laid: &LaidOut<Msg>,
    offset: &dyn Fn(WidgetId) -> Vec2,
) -> Vec<Semantic> {
    fn walk<Msg>(
        node: &LaidNode<Msg>,
        shift: Vec2,
        clip: Rect,
        offset: &dyn Fn(WidgetId) -> Vec2,
        out: &mut Vec<Semantic>,
    ) {
        let shown = (node.rect - shift).intersect(clip);
        if let Some(mut semantic) = semantic_of(node)
            && shown.width() > 0.0
            && shown.height() > 0.0
        {
            semantic.rect = shown;
            out.push(semantic);
        }
        let (shift, clip) = match &node.kind {
            LaidKind::Scroll { id, .. } => (shift + offset(*id), shown),
            // A portal is laid out detached at its anchor (`portals::settle`): no enclosing scroll moves or clips it.
            LaidKind::Portal { .. } => (Vec2::ZERO, everywhere()),
            LaidKind::Stack { .. }
            | LaidKind::Panel { .. }
            | LaidKind::Interactive { .. }
            | LaidKind::Text { .. }
            | LaidKind::Shape { .. }
            | LaidKind::TextField { .. }
            | LaidKind::Layer { .. }
            | LaidKind::World { .. } => (shift, clip),
        };
        for child in node.children() {
            walk(child, shift, clip, offset, out);
        }
    }
    fn everywhere() -> Rect {
        Rect::new(
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
            f64::INFINITY,
            f64::INFINITY,
        )
    }
    let mut out = Vec::new();
    walk(&laid.root, Vec2::ZERO, everywhere(), offset, &mut out);
    out
}

/// What a labelled node says: role, name, toggle and value. `None` is the meaning for a decorative (`Hidden`) node.
fn spoken(a11y: &A11y) -> Option<(Role, &String, Toggle, SemanticValue)> {
    match a11y {
        A11y::Hidden => None,
        A11y::Labeled {
            role,
            label,
            toggle,
        } => Some((*role, label, *toggle, SemanticValue::None)),
        A11y::Ranged { label, value } => Some((
            Role::Slider,
            label,
            Toggle::NotToggle,
            SemanticValue::Range(*value),
        )),
    }
}

/// The one handler: a node either is a labelled control or contributes nothing.
fn semantic_of<Msg>(node: &LaidNode<Msg>) -> Option<Semantic> {
    match &node.kind {
        LaidKind::Interactive {
            id,
            on,
            a11y,
            tooltip,
            availability,
            ..
        } => spoken(a11y).map(|(role, label, toggle, value)| {
            // A key- or pointer-driven control (a slider) is operable without an activate message; a drag-only
            // source still reads as information, as it always has.
            let operable = on.activate.is_some() || on.pointer.is_some() || on.key.is_some();
            Semantic {
                id: *id,
                role,
                label: label.clone(),
                toggle,
                value,
                shortcut: on.shortcut,
                description: match tooltip {
                    Tooltip::None => Description::None,
                    Tooltip::Text(text) | Tooltip::About { text, .. } => {
                        Description::Text(text.clone())
                    }
                },
                state: match (availability, operable) {
                    (Availability::Disabled, _) => SemanticState::Disabled,
                    (Availability::Enabled, true) => SemanticState::Enabled,
                    (Availability::Enabled, false) => SemanticState::Inert,
                },
                rect: node.rect,
            }
        }),
        LaidKind::TextField { id, label, .. } => Some(Semantic {
            id: *id,
            role: Role::TextBox,
            label: label.clone(),
            description: Description::None,
            // A field takes presses (focus) and has no activate message: it is enabled, not an information tile.
            state: SemanticState::Enabled,
            toggle: Toggle::NotToggle,
            value: SemanticValue::None,
            shortcut: Shortcut::Unbound,
            rect: node.rect,
        }),
        LaidKind::Stack { .. }
        | LaidKind::Panel { .. }
        | LaidKind::Text { .. }
        | LaidKind::Shape { .. }
        | LaidKind::Scroll { .. }
        | LaidKind::Layer { .. }
        | LaidKind::World { .. }
        | LaidKind::Portal { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::laid_out;
    use crate::{Bindings, Node, column, disabled, interactive, spacer, text_field};
    use kit_core::Identity;

    fn button(label: &str) -> A11y {
        A11y::Labeled {
            role: Role::Button,
            label: label.into(),
            toggle: Toggle::NotToggle,
        }
    }

    /// The mirror is what an audit clicks and a screen reader announces: pre-order, absolute rects, decorative nodes
    /// omitted, and the three activation states told apart (a regression here silently hides controls from both).
    #[test]
    fn projects_labelled_controls_in_paint_order_with_their_activation_state() {
        let press = Bindings {
            activate: Some(7u8),
            ..Bindings::none()
        };
        let tree = column(vec![
            Node::Interactive {
                identity: Identity::Positional,
                on: press,
                a11y: button("Fire"),
                tooltip: Tooltip::Text("Fires every mounted gun".into()),
                availability: Availability::Enabled,
                child: Box::new(spacer()),
            },
            interactive(
                Identity::Positional,
                Bindings::none(),
                button("Gold"),
                spacer(),
            ),
            disabled(Identity::Positional, button("Sell"), spacer()),
            interactive(
                Identity::Positional,
                Bindings::none(),
                A11y::Hidden,
                spacer(),
            ),
        ]);
        let rect = |y: f64| Rect::new(0.0, y, 40.0, y + 10.0);
        // Pre-order: root column, then per control the interactive node and its spacer child.
        let rects = [
            Rect::new(0.0, 0.0, 40.0, 100.0),
            rect(0.0),
            rect(0.0),
            rect(20.0),
            rect(20.0),
            rect(40.0),
            rect(40.0),
            rect(60.0),
            rect(60.0),
        ];
        let found = semantics(&laid_out(tree, &rects));
        let seen: Vec<_> = found
            .iter()
            .map(|s| (s.label.as_str(), s.state, s.rect.y0))
            .collect();
        assert_eq!(
            found[0].description,
            Description::Text("Fires every mounted gun".into())
        );
        assert_eq!(found[1].description, Description::None);
        assert_eq!(
            seen,
            [
                ("Fire", SemanticState::Enabled, 0.0),
                ("Gold", SemanticState::Inert, 20.0),
                ("Sell", SemanticState::Disabled, 40.0),
            ]
        );
    }

    /// J-web-10: a text field carried no accessible name, so the mirror could not expose it and a screen reader met an
    /// unlabelled edit box. It projects as a labelled `TextBox` the player can focus (enabled, not an information tile).
    #[test]
    fn a_text_field_projects_as_a_named_textbox() {
        let tree: Node<()> = column(vec![text_field(
            Identity::Positional,
            "Crew name",
            &crate::EditorState::single_line("keel"),
            crate::TextStyle::new(12.0, kit_core::peniko::Color::BLACK),
            |_| (),
        )]);
        let found = semantics(&laid_out(
            tree,
            &[
                Rect::new(0.0, 0.0, 100.0, 40.0),
                Rect::new(0.0, 0.0, 100.0, 20.0),
            ],
        ));
        assert_eq!(found.len(), 1);
        assert_eq!(
            (found[0].role, found[0].label.as_str(), found[0].state),
            (Role::TextBox, "Crew name", SemanticState::Enabled)
        );
    }

    /// Regression (kit flip, 2026-10-23, `inventory` stage): the browser mirror placed a scrolled area's controls at
    /// their unscrolled layout rects. On-screen semantics move by the scroll offset, clip to the area, and omit what
    /// is scrolled out of view.
    #[test]
    fn scrolled_controls_are_where_they_are_drawn_and_hidden_ones_are_omitted() {
        let id = WidgetId::key("area");
        let tree: Node<()> = column(vec![crate::scroll(
            id,
            kit_core::Axis::Vertical,
            crate::Style::column(),
            column(vec![
                disabled(Identity::Positional, button("Top"), spacer()),
                disabled(Identity::Positional, button("Middle"), spacer()),
                disabled(Identity::Positional, button("Bottom"), spacer()),
            ]),
        )]);
        let row = |y: f64| Rect::new(0.0, y, 40.0, y + 40.0);
        // Pre-order: root, scroll (a 50 px viewport), content column, then each control and its spacer.
        let rects = [
            Rect::new(0.0, 0.0, 40.0, 100.0),
            Rect::new(0.0, 0.0, 40.0, 50.0),
            Rect::new(0.0, 0.0, 40.0, 120.0),
            row(0.0),
            row(0.0),
            row(40.0),
            row(40.0),
            row(80.0),
            row(80.0),
        ];
        let laid = laid_out(tree, &rects);
        let on_screen = semantics_scrolled(&laid, &|_| Vec2::new(0.0, 45.0));
        let seen: Vec<_> = on_screen
            .iter()
            .map(|s| (s.label.as_str(), s.rect.y0, s.rect.y1))
            .collect();
        // Scrolled by 45: "Top" (0..40) is gone, "Middle" shows 0..35, "Bottom" shows 35..50 (clipped at 50).
        assert_eq!(seen, [("Middle", 0.0, 35.0), ("Bottom", 35.0, 50.0)]);
    }
}
