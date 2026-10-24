//! Ergonomic constructors. Widgets are just functions returning `Node<Msg>`.
use crate::scroll::ScrollSpec;
use crate::{
    A11y, AnchorH, AnchorV, Availability, Bindings, ClipSpec, Dismissal, Edit, EditorState,
    FieldStyle, FocusReturn, Layer, Masking, Mode, Node, Paint, PortalAnchor, RichText, Style,
    TextStyle, TextWrap, Tooltip, WorldAnchor,
};
use kit_core::kurbo::{RoundedRectRadii, Vec2};
use kit_core::peniko::Brush;
use kit_core::{Axis, Identity, ShapeSpec, Surface, WidgetId};
use std::rc::Rc;

/// Children in a column. Anything iterable of things that convert to a node: a `Vec<Node>`, or a mapped iterator of
/// widget builders (`items.map(|i| ui.button(..))`), with no `.into()` or `.collect()` at the call site.
pub fn column<Msg>(children: impl IntoIterator<Item = impl Into<Node<Msg>>>) -> Node<Msg> {
    Node::Stack {
        style: Style::column(),
        children: children.into_iter().map(Into::into).collect(),
    }
}
/// Children in a row. See [`column`].
pub fn row<Msg>(children: impl IntoIterator<Item = impl Into<Node<Msg>>>) -> Node<Msg> {
    Node::Stack {
        style: Style::row(),
        children: children.into_iter().map(Into::into).collect(),
    }
}
pub fn stack<Msg>(style: Style, children: Vec<Node<Msg>>) -> Node<Msg> {
    Node::Stack { style, children }
}
/// HUD container: children use `Style::anchored`.
pub fn anchors<Msg>(children: Vec<Node<Msg>>) -> Node<Msg> {
    Node::Stack {
        style: Style::anchors(),
        children,
    }
}
pub fn panel<Msg>(style: Style, surface: Surface, children: Vec<Node<Msg>>) -> Node<Msg> {
    Node::Panel {
        style,
        surface,
        children,
    }
}
pub fn text<Msg>(content: impl Into<String>, text_style: TextStyle) -> Node<Msg> {
    rich_text(RichText::plain(content), text_style)
}
pub fn rich_text<Msg>(text: RichText, text_style: TextStyle) -> Node<Msg> {
    Node::Text {
        style: Style::column(),
        text,
        text_style,
    }
}
pub fn shape<Msg>(style: Style, shape: ShapeSpec, paint: Paint) -> Node<Msg> {
    Node::Shape {
        style,
        shape,
        paint,
    }
}
pub fn rect<Msg>(style: Style, brush: Brush) -> Node<Msg> {
    shape(style, ShapeSpec::Rect, Paint::Fill(brush))
}
pub fn rounded<Msg>(style: Style, radii: RoundedRectRadii, brush: Brush) -> Node<Msg> {
    shape(style, ShapeSpec::RoundRect(radii), Paint::Fill(brush))
}
/// A box with no content and no size: the placeholder that keeps a tree's shape (and so positional identity) stable
/// when an optional part is absent.
pub fn nothing<Msg>() -> Node<Msg> {
    column(Vec::<Node<Msg>>::new())
}
/// Where an overlay sits on its base.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}
impl Corner {
    fn anchor(self) -> (AnchorH, AnchorV) {
        match self {
            Corner::TopLeft => (AnchorH::Start, AnchorV::Start),
            Corner::TopRight => (AnchorH::End, AnchorV::Start),
            Corner::BottomLeft => (AnchorH::Start, AnchorV::End),
            Corner::BottomRight => (AnchorH::End, AnchorV::End),
        }
    }
}
/// `over` painted (and hit) above the `corner` of `base`, without changing `base`'s size or its parent's flow.
pub fn overlay<Msg>(base: Node<Msg>, corner: Corner, over: Node<Msg>) -> Node<Msg> {
    let (h, v) = corner.anchor();
    Node::Stack {
        style: Style::column(),
        children: vec![
            base,
            Node::Stack {
                style: Style::column().anchored(h, v, kit_core::kurbo::Vec2::ZERO),
                children: vec![over],
            },
        ],
    }
}
/// Empty flexible space.
pub fn spacer<Msg>() -> Node<Msg> {
    Node::Stack {
        style: Style::column().grow(1.0),
        children: Vec::new(),
    }
}
/// `identity` is `Identity::Positional` (the tree position) or a `WidgetId` (an explicit key, via `Into`).
pub fn interactive<Msg>(
    identity: impl Into<Identity>,
    on: Bindings<Msg>,
    a11y: A11y,
    child: Node<Msg>,
) -> Node<Msg> {
    Node::Interactive {
        identity: identity.into(),
        on,
        a11y,
        tooltip: Tooltip::None,
        availability: Availability::Enabled,
        child: Box::new(child),
    }
}
/// Visible but inert and transparent to hits (a disabled button).
pub fn disabled<Msg>(identity: impl Into<Identity>, a11y: A11y, child: Node<Msg>) -> Node<Msg> {
    Node::Interactive {
        identity: identity.into(),
        on: Bindings::none(),
        a11y,
        tooltip: Tooltip::None,
        availability: Availability::Disabled,
        child: Box::new(child),
    }
}
/// Enabled region with no bindings: swallows presses (modal scrim, panel background).
pub fn blocker<Msg>(identity: impl Into<Identity>, child: Node<Msg>) -> Node<Msg> {
    interactive(identity, Bindings::none(), A11y::Hidden, child)
}
/// A scroll area with bars, no stick and no scroll-to request: `scroll_area` with `ScrollSpec::new(axis)`.
pub fn scroll<Msg>(
    identity: impl Into<Identity>,
    axis: Axis,
    style: Style,
    child: Node<Msg>,
) -> Node<Msg> {
    scroll_area(identity, ScrollSpec::new(axis), style, child)
}
/// `style` sizes the viewport; `child` is the content. Offsets live in the router's memory under the area's id, so
/// key the area by data (`WidgetId::of(&key)`) to keep its offset when the tree around it changes.
pub fn scroll_area<Msg>(
    identity: impl Into<Identity>,
    spec: ScrollSpec,
    style: Style,
    child: Node<Msg>,
) -> Node<Msg> {
    Node::Scroll {
        style,
        identity: identity.into(),
        spec,
        child: Box::new(child),
    }
}
/// A text field showing `state` (the app keeps it in `cx.state`; `on_edit` is its reducer's sender). A multi-line
/// state wraps at the field's width.
pub fn text_field<Msg>(
    identity: impl Into<Identity>,
    label: impl Into<String>,
    state: &EditorState,
    text: TextStyle,
    on_edit: impl Fn(Edit) -> Msg + 'static,
) -> Node<Msg> {
    let wrap = match state.mode() {
        Mode::SingleLine => TextWrap::None,
        Mode::MultiLine => TextWrap::WordOrGlyph,
    };
    Node::TextField {
        style: Style::row(),
        identity: identity.into(),
        label: label.into(),
        view: state.view(),
        on_edit: Rc::new(on_edit),
        field: FieldStyle {
            text: text.wrap(wrap),
            masking: Masking::Plain,
            inset: FIELD_INSET,
        },
        claim: crate::FocusClaim::Nothing,
    }
}
/// Default space between a field's box and its text (the theme's `xs` and `sm` spacing).
pub const FIELD_INSET: Vec2 = Vec2::new(4.0, 2.0);
pub fn layer<Msg>(opacity: f32, child: Node<Msg>) -> Node<Msg> {
    Node::Layer {
        opacity,
        clip: ClipSpec::Unclipped,
        child: Box::new(child),
    }
}
pub fn clipped<Msg>(child: Node<Msg>) -> Node<Msg> {
    Node::Layer {
        opacity: 1.0,
        clip: ClipSpec::Bounds,
        child: Box::new(child),
    }
}
/// `child` drawn above the normal tree on `layer`, placed against `anchor`, taking no space where it is written. It
/// stays until the component stops building it; see [`dismissible`] for one that asks to be closed.
pub fn portal<Msg>(layer: Layer, anchor: PortalAnchor, child: Node<Msg>) -> Node<Msg> {
    Node::Portal {
        layer,
        anchor,
        dismissal: Dismissal::Never,
        returns: FocusReturn::Nowhere,
        child: Box::new(child),
    }
}
/// A [`portal`] that sends `dismiss` when a press lands outside it, or Escape is pressed while it is the topmost
/// dismissible portal. The component decides what closing means by how it handles `dismiss`.
pub fn dismissible<Msg>(
    layer: Layer,
    anchor: PortalAnchor,
    dismiss: Msg,
    child: Node<Msg>,
) -> Node<Msg> {
    Node::Portal {
        layer,
        anchor,
        dismissal: Dismissal::Intent(dismiss),
        returns: FocusReturn::Nowhere,
        child: Box::new(child),
    }
}
/// A [`dismissible`] portal opened from the control keyed `opener`: when it closes, focus goes back to that control
/// (or the documented fallback if the control is gone; see `kit_input::Router::frame`).
pub fn dismissible_from<Msg>(
    opener: WidgetId,
    layer: Layer,
    anchor: PortalAnchor,
    dismiss: Msg,
    child: Node<Msg>,
) -> Node<Msg> {
    Node::Portal {
        layer,
        anchor,
        dismissal: Dismissal::Intent(dismiss),
        returns: FocusReturn::To(opener),
        child: Box::new(child),
    }
}
pub fn world<Msg>(anchor: WorldAnchor, child: Node<Msg>) -> Node<Msg> {
    Node::World {
        anchor,
        child: Box::new(child),
    }
}

/// Children in a column, without the `vec![]`: `column![a, b, c]`. Each child is anything that is `Into<Node<Msg>>`
/// (a `Node`, or a widget builder such as `ui.button(..)`).
#[macro_export]
macro_rules! column {
    ($($child:expr),* $(,)?) => {
        $crate::column([$($crate::Node::from($child)),*])
    };
}
/// Children in a row: `row![a, b, c]`. See [`column!`].
#[macro_export]
macro_rules! row {
    ($($child:expr),* $(,)?) => {
        $crate::row([$($crate::Node::from($child)),*])
    };
}
