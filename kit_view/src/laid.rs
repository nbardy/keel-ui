use crate::scroll::ScrollSpec;
use crate::{
    A11y, Availability, Bindings, ClipSpec, CosmicText, Dismissal, Edit, FieldGeometry, FieldStyle,
    FieldView, FocusClaim, Layer, MaxWidth, Node, Paint, PortalAnchor, Shortcut, TextLayout,
    TextWrap, Tooltip, WorldAnchor, WorldPoint,
};
use crate::{Image, ImageStyle};
use kit_core::kurbo::{Point, Rect, Vec2};
use kit_core::peniko::Color;
use kit_core::{Epoch, Key, ShapeSpec, Surface, Viewport, WidgetId};
use std::rc::Rc;

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum Shown {
    Visible,
    /// Behind the camera / off-screen: neither painted nor hit-testable (subtree included).
    Culled,
}

/// The layout's answer for one node. `Culled` is only meaningful for `Node::World`.
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct Placed {
    /// Absolute logical px. Children of a `Scroll` are placed as if its scroll offset were zero.
    pub rect: Rect,
    pub shown: Shown,
}

#[derive(Copy, Clone, PartialEq, Debug)]
pub enum Projection {
    OnScreen(Point),
    Culled,
}
/// The game's camera, seen from the UI. Keeps projection out of the kit.
pub trait WorldProjector {
    fn project(&self, point: WorldPoint) -> Projection;
}
/// For screens without a world: every world anchor is culled (a visible, typed outcome).
#[derive(Copy, Clone, Debug)]
pub struct NoWorld;
impl WorldProjector for NoWorld {
    fn project(&self, _point: WorldPoint) -> Projection {
        Projection::Culled
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum LayoutError {
    /// The engine returned a different number of `Placed` than the tree has nodes (pre-order, wrappers included).
    PlacedCount { expected: usize, got: usize },
    /// The node at pre-order index `node` names a grid row or column but its parent is not a grid (taffy would
    /// silently ignore the placement).
    CellOutsideGrid { node: usize },
    /// Two interactive nodes resolved to the same `WidgetId` (a repeated `.key(..)`): hover, press and drag memory
    /// would be shared between them, so the tree is rejected instead of laid out.
    DuplicateIdentity { id: WidgetId },
    /// Two enabled controls bind the same key: one press would have to pick, so the tree is rejected instead.
    DuplicateShortcut { key: Key },
    /// A portal anchors to a node key that the tree does not hold, so there is nothing to place it against.
    PortalAnchorMissing { id: WidgetId },
}

pub enum LaidKind<Msg> {
    Stack {
        children: Vec<LaidNode<Msg>>,
    },
    Panel {
        surface: Surface,
        children: Vec<LaidNode<Msg>>,
    },
    Text {
        layout: TextLayout,
        color: Color,
    },
    Image {
        image: Image,
        image_style: ImageStyle,
    },
    Shape {
        shape: ShapeSpec,
        paint: Paint,
    },
    Interactive {
        id: WidgetId,
        on: Bindings<Msg>,
        a11y: A11y,
        tooltip: Tooltip,
        availability: Availability,
        child: Box<LaidNode<Msg>>,
    },
    /// `child.rect` is the content rect at scroll offset zero; its size is the content size.
    Scroll {
        id: WidgetId,
        spec: ScrollSpec,
        child: Box<LaidNode<Msg>>,
    },
    TextField {
        id: WidgetId,
        label: String,
        layout: TextLayout,
        view: FieldView,
        geometry: FieldGeometry,
        on_edit: Rc<dyn Fn(Edit) -> Msg>,
        field: FieldStyle,
        claim: FocusClaim,
    },
    Layer {
        opacity: f32,
        clip: ClipSpec,
        child: Box<LaidNode<Msg>>,
    },
    World {
        anchor: WorldAnchor,
        shown: Shown,
        child: Box<LaidNode<Msg>>,
    },
    /// Not part of the normal tree's paint or hit order: see [`crate::Stacking`]. `rect` is the child's.
    Portal {
        layer: Layer,
        anchor: PortalAnchor,
        dismissal: Dismissal<Msg>,
        child: Box<LaidNode<Msg>>,
    },
}

pub struct LaidNode<Msg> {
    pub rect: Rect,
    pub kind: LaidKind<Msg>,
}

impl<Msg> LaidNode<Msg> {
    pub fn children(&self) -> &[LaidNode<Msg>] {
        match &self.kind {
            LaidKind::Stack { children } | LaidKind::Panel { children, .. } => children,
            LaidKind::Interactive { child, .. }
            | LaidKind::Scroll { child, .. }
            | LaidKind::Layer { child, .. }
            | LaidKind::World { child, .. }
            | LaidKind::Portal { child, .. } => core::slice::from_ref(child),
            LaidKind::Text { .. }
            | LaidKind::Image { .. }
            | LaidKind::Shape { .. }
            | LaidKind::TextField { .. } => &[],
        }
    }
    /// The id of a node that carries one. `None` is the meaning: containers, text and shapes have no identity.
    pub fn id(&self) -> Option<WidgetId> {
        match &self.kind {
            LaidKind::Interactive { id, .. }
            | LaidKind::Scroll { id, .. }
            | LaidKind::TextField { id, .. } => Some(*id),
            LaidKind::Stack { .. }
            | LaidKind::Panel { .. }
            | LaidKind::Text { .. }
            | LaidKind::Image { .. }
            | LaidKind::Shape { .. }
            | LaidKind::Layer { .. }
            | LaidKind::World { .. }
            | LaidKind::Portal { .. } => None,
        }
    }
    pub fn children_mut(&mut self) -> &mut [LaidNode<Msg>] {
        match &mut self.kind {
            LaidKind::Stack { children } | LaidKind::Panel { children, .. } => children,
            LaidKind::Interactive { child, .. }
            | LaidKind::Scroll { child, .. }
            | LaidKind::Layer { child, .. }
            | LaidKind::World { child, .. }
            | LaidKind::Portal { child, .. } => core::slice::from_mut(child),
            LaidKind::Text { .. }
            | LaidKind::Image { .. }
            | LaidKind::Shape { .. }
            | LaidKind::TextField { .. } => &mut [],
        }
    }
    /// Move this node and everything under it by `by` (a portal is laid out at the origin, then moved to its anchor).
    pub fn translate(&mut self, by: Vec2) {
        self.rect = self.rect + by;
        for child in self.children_mut() {
            child.translate(by);
        }
    }
    pub fn find(&self, target: WidgetId) -> Option<&LaidNode<Msg>> {
        if self.id() == Some(target) {
            Some(self)
        } else {
            self.children().iter().find_map(|c| c.find(target))
        }
    }
    /// This node and every descendant, pre-order.
    pub fn descendants(&self) -> Vec<&LaidNode<Msg>> {
        fn walk<'a, Msg>(node: &'a LaidNode<Msg>, out: &mut Vec<&'a LaidNode<Msg>>) {
            out.push(node);
            for child in node.children() {
                walk(child, out);
            }
        }
        let mut out = Vec::new();
        walk(self, &mut out);
        out
    }
    fn first_repeated_id(&self) -> Option<WidgetId> {
        let mut ids: Vec<WidgetId> = self.descendants().iter().filter_map(|n| n.id()).collect();
        ids.sort_unstable();
        ids.windows(2)
            .find(|pair| pair[0] == pair[1])
            .map(|pair| pair[0])
    }
    /// The first key that two enabled controls both bind.
    fn first_repeated_shortcut(&self) -> Option<Key> {
        let mut keys: Vec<Key> = self
            .descendants()
            .iter()
            .filter_map(|n| n.shortcut())
            .collect();
        keys.sort_unstable();
        keys.windows(2).find(|p| p[0] == p[1]).map(|p| p[0])
    }
    /// The key that activates this node, if it is an enabled control with a message to send. `None` is the meaning:
    /// most nodes have no keyboard binding.
    pub fn shortcut(&self) -> Option<Key> {
        match &self.kind {
            LaidKind::Interactive {
                on:
                    Bindings {
                        activate: Some(_),
                        shortcut: Shortcut::Key(key),
                        ..
                    },
                availability: Availability::Enabled,
                ..
            } => Some(*key),
            LaidKind::Interactive { .. }
            | LaidKind::Stack { .. }
            | LaidKind::Panel { .. }
            | LaidKind::Text { .. }
            | LaidKind::Image { .. }
            | LaidKind::Shape { .. }
            | LaidKind::Scroll { .. }
            | LaidKind::TextField { .. }
            | LaidKind::Layer { .. }
            | LaidKind::World { .. }
            | LaidKind::Portal { .. } => None,
        }
    }
    /// Topmost control under `point` in paint order (later siblings are above earlier ones).
    pub fn control_at(&self, point: Point, reach: Reach) -> Option<Control<'_, Msg>> {
        self.children()
            .iter()
            .rev()
            .find_map(|child| child.control_at(point, reach))
            .or_else(|| match &self.kind {
                LaidKind::Interactive {
                    id,
                    on,
                    a11y,
                    tooltip,
                    availability,
                    ..
                } if self.rect.contains(point) && reach.admits(*availability) => Some(Control {
                    rect: self.rect,
                    id: *id,
                    on,
                    a11y,
                    tooltip,
                    availability: *availability,
                }),
                LaidKind::Interactive { .. }
                | LaidKind::Stack { .. }
                | LaidKind::Panel { .. }
                | LaidKind::Text { .. }
                | LaidKind::Image { .. }
                | LaidKind::Shape { .. }
                | LaidKind::Scroll { .. }
                | LaidKind::TextField { .. }
                | LaidKind::Layer { .. }
                | LaidKind::World { .. }
                | LaidKind::Portal { .. } => None,
            })
    }
}

/// Which controls a point query counts: a press ignores disabled controls (they are transparent to hits); a tooltip
/// or accessible-name lookup does not.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum Reach {
    Pressable,
    Named,
}
impl Reach {
    fn admits(self, availability: Availability) -> bool {
        match self {
            Reach::Named => true,
            Reach::Pressable => availability == Availability::Enabled,
        }
    }
}

/// An interactive node seen from outside the tree: what a pointer at a point is over.
pub struct Control<'a, Msg> {
    pub rect: Rect,
    pub id: WidgetId,
    pub on: &'a Bindings<Msg>,
    pub a11y: &'a A11y,
    pub tooltip: &'a Tooltip,
    pub availability: Availability,
}

/// Node rects + interactive regions (the `Interactive`/`TextField`/`Scroll` nodes) for one frame.
/// Hit regions are not a separate list: scroll offsets live in `UiMemory`, so the router walks this tree.
pub struct LaidOut<Msg> {
    pub viewport: Viewport,
    /// Gestures begun under another epoch never fire; set by the app when the screen/turn is replaced.
    pub epoch: Epoch,
    pub root: LaidNode<Msg>,
}

impl<Msg> LaidOut<Msg> {
    pub fn with_epoch(mut self, epoch: Epoch) -> Self {
        self.epoch = epoch;
        self
    }
    pub fn find(&self, id: WidgetId) -> Option<&LaidNode<Msg>> {
        self.root.find(id)
    }

    /// Tail of `TaffyLayout::layout`: given its pre-order `Placed` list, build the tree. Text is laid
    /// out here at the final box width, so measuring and drawing agree on the line breaks.
    pub fn assemble(
        root: Node<Msg>,
        viewport: Viewport,
        placed: &[Placed],
        text: &mut CosmicText,
    ) -> Result<Self, LayoutError> {
        let expected = root.count();
        if placed.len() != expected {
            return Err(LayoutError::PlacedCount {
                expected,
                got: placed.len(),
            });
        }
        let mut next = 0;
        let root = build(root, WidgetId::key("kit"), placed, &mut next, text);
        if let Some(id) = root.first_repeated_id() {
            return Err(LayoutError::DuplicateIdentity { id });
        }
        if let Some(key) = root.first_repeated_shortcut() {
            return Err(LayoutError::DuplicateShortcut { key });
        }
        Ok(Self {
            viewport,
            epoch: Epoch::default(),
            root,
        })
    }
}

/// `path` is the node's tree path (root id, then one `index` per level): the `Identity::Positional` id.
fn build<Msg>(
    node: Node<Msg>,
    path: WidgetId,
    placed: &[Placed],
    next: &mut usize,
    text: &mut CosmicText,
) -> LaidNode<Msg> {
    let here = placed[*next];
    *next += 1;
    let rect = here.rect;
    let kind = match node {
        Node::Stack { children, .. } => LaidKind::Stack {
            children: build_all(children, path, placed, next, text),
        },
        Node::Panel {
            surface, children, ..
        } => LaidKind::Panel {
            surface,
            children: build_all(children, path, placed, next, text),
        },
        Node::Text {
            text: rich,
            text_style,
            ..
        } => {
            let width = match text_style.wrap {
                TextWrap::None => MaxWidth::Unbounded,
                TextWrap::Glyph | TextWrap::Word | TextWrap::WordOrGlyph => {
                    MaxWidth::Wrap(rect.width())
                }
            };
            LaidKind::Text {
                layout: text.layout(&rich, &text_style, width),
                color: text_style.color,
            }
        }
        Node::Image {
            image, image_style, ..
        } => LaidKind::Image { image, image_style },
        Node::Shape { shape, paint, .. } => LaidKind::Shape { shape, paint },
        Node::Interactive {
            identity,
            on,
            a11y,
            tooltip,
            availability,
            child,
        } => LaidKind::Interactive {
            id: identity.resolve(path),
            on,
            a11y,
            tooltip,
            availability,
            child: Box::new(build(*child, path.index(0), placed, next, text)),
        },
        Node::Scroll {
            identity,
            spec,
            child,
            ..
        } => LaidKind::Scroll {
            id: identity.resolve(path),
            spec,
            child: Box::new(build(*child, path.index(0), placed, next, text)),
        },
        Node::TextField {
            identity,
            label,
            view,
            on_edit,
            field,
            claim,
            ..
        } => {
            let room = rect.width() - 2.0 * field.inset.x;
            let width = match field.text.wrap {
                TextWrap::None => MaxWidth::Unbounded,
                TextWrap::Glyph | TextWrap::Word | TextWrap::WordOrGlyph => MaxWidth::Wrap(room),
            };
            let (shown, layout) = field.shape(text, &view, width);
            let geometry = text
                .field_geometry(layout.id, shown, &view, room as f32)
                .expect("a layout made a line ago is live until end_frame");
            LaidKind::TextField {
                id: identity.resolve(path),
                label,
                layout,
                view,
                geometry,
                on_edit,
                field,
                claim,
            }
        }
        Node::Layer {
            opacity,
            clip,
            child,
        } => LaidKind::Layer {
            opacity,
            clip,
            child: Box::new(build(*child, path.index(0), placed, next, text)),
        },
        Node::World { anchor, child } => LaidKind::World {
            anchor,
            shown: here.shown,
            child: Box::new(build(*child, path.index(0), placed, next, text)),
        },
        Node::Portal {
            layer,
            anchor,
            dismissal,
            child,
        } => LaidKind::Portal {
            layer,
            anchor,
            dismissal,
            child: Box::new(build(*child, path.index(0), placed, next, text)),
        },
    };
    LaidNode { rect, kind }
}

fn build_all<Msg>(
    children: Vec<Node<Msg>>,
    path: WidgetId,
    placed: &[Placed],
    next: &mut usize,
    text: &mut CosmicText,
) -> Vec<LaidNode<Msg>> {
    children
        .into_iter()
        .enumerate()
        .map(|(index, c)| build(c, path.index(index as u64), placed, next, text))
        .collect()
}
