//! What the layout engine needs to know about a `Node`: the five structural shapes a node
//! can take, how a leaf measures itself through `CosmicText`, and the "wrapper chain over a world anchor" case.
use kit_core::Axis;
use kit_core::kurbo::Size;
use kit_view::taffy as tf;
use kit_view::taffy::style_helpers::TaffyAuto;
use kit_view::{
    Content, CosmicText, FieldStyle, FieldView, LayoutError, MaxWidth, Node, RichText, TextStyle,
    TextWrap, WorldAnchor,
};

pub(crate) enum Kind<'a, Msg> {
    /// `Stack` / `Panel`: children placed by the node's own `Content`.
    Container(&'a [Node<Msg>]),
    Leaf(Leaf<'a>),
    /// `Interactive` / `Layer`: no box of their own; they take their child's box.
    Wrap(&'a Node<Msg>),
    Scroll {
        axis: Axis,
        child: &'a Node<Msg>,
    },
    World {
        anchor: WorldAnchor,
        child: &'a Node<Msg>,
    },
    /// Detached like `World`; placed against its anchor after layout (see `portals.rs`).
    Portal {
        child: &'a Node<Msg>,
    },
}

/// A childless node and what gives it an intrinsic size.
pub(crate) enum Leaf<'a> {
    Text {
        text: &'a RichText,
        style: &'a TextStyle,
    },
    Field {
        view: &'a FieldView,
        field: &'a FieldStyle,
    },
    /// `Shape`: sized by style only.
    Image(Size),
    Empty,
}

pub(crate) fn kind<Msg>(node: &Node<Msg>) -> Kind<'_, Msg> {
    match node {
        Node::Stack { children, .. } | Node::Panel { children, .. } => Kind::Container(children),
        Node::Text {
            text, text_style, ..
        } => Kind::Leaf(Leaf::Text {
            text,
            style: text_style,
        }),
        Node::TextField { view, field, .. } => Kind::Leaf(Leaf::Field { view, field }),
        Node::Image { image, .. } => Kind::Leaf(Leaf::Image(image.size().logical())),
        Node::Shape { .. } => Kind::Leaf(Leaf::Empty),
        Node::Interactive { child, .. } | Node::Layer { child, .. } => Kind::Wrap(child),
        Node::Scroll { spec, child, .. } => Kind::Scroll {
            axis: spec.axis,
            child,
        },
        Node::World { anchor, child } => Kind::World {
            anchor: *anchor,
            child,
        },
        Node::Portal { child, .. } => Kind::Portal { child },
    }
}

/// What a detached run ends in.
#[derive(Copy, Clone)]
pub(crate) enum Tail {
    /// Floats at a projected point.
    World(WorldAnchor),
    /// Placed against an anchor once the main tree is solved.
    Portal,
}

/// A run of `Wrap` nodes ending in a `World` or `Portal` node (possibly just that node itself). The whole run is
/// detached: it takes no space in its parent.
pub(crate) struct Detached<'a, Msg> {
    /// Number of nodes in the run (wrappers plus the tail), which all share the detached child's rect.
    pub links: usize,
    pub tail: Tail,
    pub child: &'a Node<Msg>,
}

pub(crate) fn detached<Msg>(node: &Node<Msg>) -> Option<Detached<'_, Msg>> {
    let mut links = 1;
    let mut here = node;
    loop {
        match kind(here) {
            Kind::Wrap(child) => {
                links += 1;
                here = child;
            }
            Kind::World { anchor, child } => {
                return Some(Detached {
                    links,
                    tail: Tail::World(anchor),
                    child,
                });
            }
            Kind::Portal { child } => {
                return Some(Detached {
                    links,
                    tail: Tail::Portal,
                    child,
                });
            }
            Kind::Container(_) | Kind::Leaf(_) | Kind::Scroll { .. } => return None,
        }
    }
}

/// What a node's parent says about a grid row/column on the node.
#[derive(Copy, Clone)]
enum Parent {
    /// Root, wrapper child, or world child: the node's own placement is never consulted.
    Unplaced,
    Grid,
    /// Flex, anchors or scroll host: a grid cell here has no meaning.
    NotGrid,
}

/// Rejects a grid row/column on any node whose parent is not a grid, before taffy sees the tree. `TaffyLayout` calls
/// this first, so a misplaced cell is a typed error instead of a placement taffy silently ignores.
pub(crate) fn check_cells<Msg>(root: &Node<Msg>) -> Result<(), LayoutError> {
    fn walk<Msg>(node: &Node<Msg>, parent: Parent, next: &mut usize) -> Result<(), LayoutError> {
        let index = *next;
        if let Some(chain) = detached(node) {
            *next += chain.links;
            return walk(chain.child, Parent::Unplaced, next);
        }
        if let (Parent::NotGrid, true) = (parent, names_cell(&node.style().layout)) {
            return Err(LayoutError::CellOutsideGrid { node: index });
        }
        *next += 1;
        match kind(node) {
            Kind::Leaf(_) => Ok(()),
            Kind::Wrap(child) => walk(child, Parent::Unplaced, next),
            Kind::Scroll { child, .. } => walk(child, Parent::NotGrid, next),
            Kind::Container(children) => {
                let style = node.style();
                let host = match (&style.content, style.layout.display) {
                    (Content::Taffy, tf::Display::Grid) => Parent::Grid,
                    (Content::Taffy, _) | (Content::Anchors, _) => Parent::NotGrid,
                };
                children.iter().try_for_each(|c| walk(c, host, next))
            }
            // `detached` caught every `World` and `Portal` above.
            Kind::World { child, .. } | Kind::Portal { child } => {
                walk(child, Parent::Unplaced, next)
            }
        }
    }
    walk(root, Parent::Unplaced, &mut 0)
}

fn names_cell(s: &tf::Style) -> bool {
    let auto = tf::Line::<tf::GridPlacement>::AUTO;
    s.grid_row != auto || s.grid_column != auto
}

/// How much horizontal room a leaf is offered while measuring.
#[derive(Copy, Clone, PartialEq, Debug)]
pub(crate) enum Room {
    /// The box width is already decided.
    Exactly(f64),
    /// At most this wide.
    Within(f64),
    /// As narrow as the content can get (longest unbreakable run).
    MinContent,
    /// As wide as the content wants.
    MaxContent,
}

impl Room {
    /// The room left after `by` of horizontal padding.
    fn narrowed(self, by: f64) -> Room {
        match self {
            Room::Exactly(w) => Room::Exactly((w - by).max(0.0)),
            Room::Within(w) => Room::Within((w - by).max(0.0)),
            Room::MinContent | Room::MaxContent => self,
        }
    }
    fn max_width(self, wrap: TextWrap) -> MaxWidth {
        match (wrap, self) {
            (TextWrap::None, _) => MaxWidth::Unbounded,
            (TextWrap::Glyph | TextWrap::Word | TextWrap::WordOrGlyph, room) => match room {
                Room::Exactly(w) | Room::Within(w) => MaxWidth::Wrap(w),
                Room::MaxContent => MaxWidth::Unbounded,
                Room::MinContent => MaxWidth::Wrap(0.0),
            },
        }
    }
}

impl Leaf<'_> {
    /// The room with every distinction this leaf's measure ignores erased, so equal answers share one key: an
    /// `Exactly` width measures like a `Within` one, and unwrapped text or a fixed-size leaf ignores the room.
    pub(crate) fn canonical(&self, room: Room) -> Room {
        let wrap = match self {
            Leaf::Text { style, .. } => style.wrap,
            Leaf::Field { field, .. } => field.text.wrap,
            Leaf::Image(_) | Leaf::Empty => TextWrap::None,
        };
        match (wrap, room) {
            (TextWrap::None, _) => Room::MaxContent,
            (TextWrap::Glyph | TextWrap::Word | TextWrap::WordOrGlyph, Room::Exactly(w)) => {
                Room::Within(w)
            }
            (
                TextWrap::Glyph | TextWrap::Word | TextWrap::WordOrGlyph,
                Room::Within(_) | Room::MinContent | Room::MaxContent,
            ) => room,
        }
    }

    /// Content size of the leaf (no padding) when offered `room`.
    pub(crate) fn measure(&self, text: &mut CosmicText, room: Room) -> Size {
        match self {
            Leaf::Text { text: rich, style } => {
                text.layout(rich, style, room.max_width(style.wrap)).size
            }
            // The field's box is its text plus the inset on every side (a multi-line field wraps inside it).
            Leaf::Field { view, field } => {
                let room = room.narrowed(2.0 * field.inset.x);
                let (_, layout) = field.shape(text, view, room.max_width(field.text.wrap));
                Size::new(
                    layout.size.width + 2.0 * field.inset.x,
                    layout.size.height + 2.0 * field.inset.y,
                )
            }
            Leaf::Image(size) => *size,
            Leaf::Empty => Size::ZERO,
        }
    }
}
