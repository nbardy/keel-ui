//! The layout: taffy 0.14 over `Node`s whose `Style::layout` is already a `taffy::Style`.
//!
//! One taffy node per kit `Node`, created in pre-order so the slot list is the `Placed` list. Three kinds of node
//! never sit in their parent's flow and are resolved after taffy runs:
//! * anchored children are laid out by taffy only for their size; `place_anchored` positions them against the host
//!   box (taffy has no safe-area anchor: margins, safe area and offsets are resolved by `place_anchored`);
//! * `World` boxes are separate taffy roots, positioned at their projected point;
//! * culled world subtrees are never given to taffy.
//!
//! What this file adds to a node's own taffy style is only kit policy: the role a node plays (root fills the viewport
//! and pads by the safe area, a wrapper's child fills it, a world box floats), anchors, the anchors container that
//! stretches, and the scroll host (its child never shrinks, its overflow scrolls).
use crate::kind::{Detached, Kind, Leaf, Room, Tail, check_cells, detached, kind};
use kit_core::kurbo::{Insets, Point, Rect, Size, Vec2};
use kit_core::{Axis, Viewport};
use kit_view::taffy as tf;
use kit_view::taffy::style_helpers::TaffyAuto;
use kit_view::{
    Anchor, AnchorH, AnchorV, Content, CosmicText, LaidOut, LayoutError, Node, Percent, Placed,
    Placement, Projection, SafeArea, Shown, Style, WorldProjector, px,
};

#[derive(Copy, Clone)]
enum Edge {
    Start,
    Center,
    End,
}
impl From<AnchorH> for Edge {
    fn from(h: AnchorH) -> Self {
        match h {
            AnchorH::Start => Edge::Start,
            AnchorH::Center => Edge::Center,
            AnchorH::End => Edge::End,
        }
    }
}
impl From<AnchorV> for Edge {
    fn from(v: AnchorV) -> Self {
        match v {
            AnchorV::Start => Edge::Start,
            AnchorV::Center => Edge::Center,
            AnchorV::End => Edge::End,
        }
    }
}

/// Origin of a `len`-long box inside `lo..hi`, margins eating into the free space, then shifted by `offset`.
fn span_origin(
    lo: f64,
    hi: f64,
    len: f64,
    margin_lo: f64,
    margin_hi: f64,
    edge: Edge,
    offset: f64,
) -> f64 {
    let here = match edge {
        Edge::Start => lo + margin_lo,
        Edge::Center => lo + (hi - lo - len + margin_lo - margin_hi) / 2.0,
        Edge::End => hi - margin_hi - len,
    };
    here + offset
}

/// Where a box of `size` goes when anchored in `container`. `safe` is the safe-area inset of the container; it only
/// applies when the anchor says `SafeArea::Respect`. `offset` is a plain screen-space shift applied last (positive
/// is right/down regardless of the edge), so a bottom-right badge nudged inward uses a negative offset.
pub fn place_anchored(
    container: Rect,
    safe: Insets,
    size: Size,
    margin: Insets,
    anchor: &Anchor,
) -> Rect {
    let safe = match anchor.safe_area {
        SafeArea::Respect => safe,
        SafeArea::Ignore => Insets::ZERO,
    };
    let area = container - safe;
    let Vec2 { x: dx, y: dy } = anchor.offset;
    let x = span_origin(
        area.x0,
        area.x1,
        size.width,
        margin.x0,
        margin.x1,
        anchor.h.into(),
        dx,
    );
    let y = span_origin(
        area.y0,
        area.y1,
        size.height,
        margin.y0,
        margin.y1,
        anchor.v.into(),
        dy,
    );
    Rect::from_origin_size(Point::new(x, y), size)
}

/// Placement of a child that carries no anchor of its own (`Flow`/`Cell`) inside an anchors container: top-left of the
/// safe area.
pub(crate) const OVERLAY_DEFAULT: Anchor = Anchor {
    h: AnchorH::Start,
    v: AnchorV::Start,
    offset: Vec2::ZERO,
    safe_area: SafeArea::Respect,
};

/// The kit's layout (there is no other: the `LayoutEngine` seam was removed with the mirror style types). Keeps its
/// taffy tree allocation between frames (the tree is cleared and rebuilt each frame; per-node results are not reused
/// because the `Node` tree is new every frame anyway).
pub struct TaffyLayout {
    tree: tf::TaffyTree<usize>,
}

impl Default for TaffyLayout {
    fn default() -> Self {
        Self::new()
    }
}

impl TaffyLayout {
    pub fn new() -> Self {
        let mut tree = tf::TaffyTree::new();
        // Rounding would snap a measured text width down and make `assemble`'s re-layout wrap differently.
        tree.disable_rounding();
        Self { tree }
    }
}

impl TaffyLayout {
    /// Consumes the tree (rebuilt every frame anyway) so message values move into `LaidOut` without cloning.
    pub fn layout<Msg>(
        &mut self,
        root: Node<Msg>,
        viewport: Viewport,
        text: &mut CosmicText,
        world: &dyn WorldProjector,
    ) -> Result<LaidOut<Msg>, LayoutError> {
        check_cells(&root)?;
        self.tree.clear();
        let placed = {
            let mut frame = Frame {
                tree: &mut self.tree,
                viewport,
                world,
                leaves: Vec::new(),
                slots: Vec::with_capacity(root.count()),
                floating: Vec::new(),
            };
            let main = frame.build(&root, Role::Root, true);
            frame.solve(main, text)
        };
        let mut laid = LaidOut::assemble(root, viewport, &placed, text)?;
        crate::portals::settle(&mut laid)?;
        Ok(laid)
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Frame state

/// How a slot's rect is derived from taffy's output once layout has run.
#[derive(Copy, Clone)]
enum Link {
    /// Root, or a portal root (moved to its anchor by `portals::settle`): taffy location is already absolute.
    Origin,
    /// Floating world box: centred on this point.
    FloatCenter(Point),
    /// In flow: taffy location is relative to the parent slot's rect.
    Flow { parent: usize },
    /// Placed by `place_anchored` against the host's rect, with the margins taffy resolved.
    Anchored {
        host: usize,
        anchor: Anchor,
        safe: Insets,
    },
}

enum Slot {
    /// Inside a culled world subtree (or the culled `World` itself): zero rect, `Shown::Culled`.
    Culled,
    /// A wrapper over / the `World` node itself: shares the rect of the floating child that follows it.
    Follow,
    Laid {
        id: tf::NodeId,
        link: Link,
    },
}

#[derive(Copy, Clone)]
enum HostKind {
    /// taffy places the children (flex, grid or block, per the host's own style).
    Taffy,
    Anchors,
    Scroll,
}

/// The container a node is placed in.
#[derive(Copy, Clone)]
struct Host {
    kind: HostKind,
    /// Slot index of the container.
    entry: usize,
    /// Safe-area inset that anchors in this container respect: non-zero only for the root container.
    safe: Insets,
}

#[derive(Copy, Clone)]
enum Role {
    Root,
    /// Detached world box centred on this point.
    Float(Point),
    /// Detached portal box: laid out at the origin, no bigger than the viewport.
    Portal,
    /// Sole child of a wrapper: fills it.
    Fill {
        parent: usize,
    },
    In(Host),
}

/// What a placement means inside a particular host. Exhaustive over (placement, host).
enum Resolved {
    Anchored(Anchor),
    Flow,
}

fn resolve(placement: &Placement, host: HostKind) -> Resolved {
    match (placement, host) {
        (Placement::Anchored(a), _) => Resolved::Anchored(*a),
        (Placement::Flow, HostKind::Anchors) => Resolved::Anchored(OVERLAY_DEFAULT),
        (Placement::Flow, HostKind::Taffy | HostKind::Scroll) => Resolved::Flow,
    }
}

struct Frame<'a, 'n> {
    tree: &'a mut tf::TaffyTree<usize>,
    viewport: Viewport,
    world: &'a dyn WorldProjector,
    /// Measure contexts: taffy nodes carry an index into this.
    leaves: Vec<Leaf<'n>>,
    slots: Vec<Slot>,
    /// Roots of detached world trees, laid out after the main tree.
    floating: Vec<tf::NodeId>,
}

/// Built node: either part of its parent's taffy tree or a detached world box.
enum Built {
    InTree(tf::NodeId),
    Detached,
}

impl<'n> Frame<'_, 'n> {
    fn leaf_node(&mut self, style: tf::Style, leaf: Leaf<'n>) -> tf::NodeId {
        let index = self.leaves.len();
        self.leaves.push(leaf);
        self.tree
            .new_leaf_with_context(style, index)
            .expect("taffy new_leaf is infallible")
    }

    fn plain_node(&mut self, style: tf::Style) -> tf::NodeId {
        self.tree
            .new_leaf(style)
            .expect("taffy new_leaf is infallible")
    }

    fn adopt(&mut self, parent: tf::NodeId, child: Built) {
        match child {
            Built::InTree(id) => self
                .tree
                .add_child(parent, id)
                .expect("taffy add_child on live nodes"),
            Built::Detached => {}
        }
    }

    fn push_laid(&mut self, id: tf::NodeId, link: Link) -> usize {
        self.slots.push(Slot::Laid { id, link });
        self.slots.len() - 1
    }

    fn cull<Msg>(&mut self, node: &Node<Msg>) {
        for _ in 0..node.count() {
            self.slots.push(Slot::Culled);
        }
    }

    /// Pre-order build of `node`. `root_content` is true along the root's wrapper chain down to its real container.
    fn build<Msg>(&mut self, node: &'n Node<Msg>, role: Role, root_content: bool) -> Built {
        if let Some(chain) = detached(node) {
            return self.build_detached(node, chain);
        }
        let style = node.style();
        let safe = if root_content {
            self.viewport.safe_area
        } else {
            Insets::ZERO
        };
        match kind(node) {
            Kind::Wrap(child) => {
                let column = tf::Style {
                    display: tf::Display::Flex,
                    flex_direction: tf::FlexDirection::Column,
                    ..tf::Style::DEFAULT
                };
                let (t, link) = self.placed(column, style, role);
                let id = self.plain_node(t);
                let at = self.push_laid(id, link);
                let built = self.build(child, Role::Fill { parent: at }, root_content);
                self.adopt(id, built);
                Built::InTree(id)
            }
            Kind::Leaf(leaf) => {
                let (t, link) = self.placed(container_half(&style.layout), style, role);
                let id = self.leaf_node(t, leaf);
                self.push_laid(id, link);
                Built::InTree(id)
            }
            Kind::Container(children) => {
                let inner = self.padded(container_half(&style.layout), safe);
                let (t, link) = self.placed(inner, style, role);
                let (t, host_kind) = arrange(t, &style.content, role);
                let id = self.plain_node(t);
                let at = self.push_laid(id, link);
                let host = Host {
                    kind: host_kind,
                    entry: at,
                    safe,
                };
                for child in children {
                    let built = self.build(child, Role::In(host), false);
                    self.adopt(id, built);
                }
                Built::InTree(id)
            }
            Kind::Scroll { axis, child } => {
                let inner = self.padded(scrolling(container_half(&style.layout), axis), safe);
                let (t, link) = self.placed(inner, style, role);
                let id = self.plain_node(t);
                let at = self.push_laid(id, link);
                let host = Host {
                    kind: HostKind::Scroll,
                    entry: at,
                    safe: Insets::ZERO,
                };
                let built = self.build(child, Role::In(host), false);
                self.adopt(id, built);
                Built::InTree(id)
            }
            // Reached only when the early chain check above did not fire, which a bare `World`/`Portal` always does.
            Kind::World { anchor, child } => self.build_detached(
                node,
                Detached {
                    links: 1,
                    tail: Tail::World(anchor),
                    child,
                },
            ),
            Kind::Portal { child } => self.build_detached(
                node,
                Detached {
                    links: 1,
                    tail: Tail::Portal,
                    child,
                },
            ),
        }
    }

    fn build_detached<Msg>(&mut self, node: &'n Node<Msg>, chain: Detached<'n, Msg>) -> Built {
        let role = match chain.tail {
            Tail::Portal => Some(Role::Portal),
            Tail::World(anchor) => match self.world.project(anchor.point) {
                Projection::Culled => None,
                Projection::OnScreen(p) => Some(Role::Float(p + anchor.offset)),
            },
        };
        match role {
            None => self.cull(node),
            Some(role) => {
                for _ in 0..chain.links {
                    self.slots.push(Slot::Follow);
                }
                if let Built::InTree(id) = self.build(chain.child, role, false) {
                    self.floating.push(id);
                }
            }
        }
        Built::Detached
    }

    // -----------------------------------------------------------------------------------------------------------
    // Kit policy on top of the node's own taffy style

    /// `inner` (how the box arranges its children) plus everything the role decides about the box itself: size limits,
    /// margin, flex/grid item fields, position.
    fn placed(&self, inner: tf::Style, style: &Style, role: Role) -> (tf::Style, Link) {
        match role {
            Role::Root => {
                let Size { width, height } = self.viewport.size;
                let t = tf::Style {
                    size: tf::Size {
                        width: px(width),
                        height: px(height),
                    },
                    ..inner
                };
                (t, Link::Origin)
            }
            Role::Float(center) => {
                let t = tf::Style {
                    margin: tf::Rect::zero(),
                    ..item_half(inner, &style.layout)
                };
                (t, Link::FloatCenter(center))
            }
            Role::Portal => {
                // A portal never outgrows the screen; its margin is not part of where an anchor puts it.
                let Size { width, height } = self.viewport.size;
                let t = tf::Style {
                    margin: tf::Rect::zero(),
                    max_size: tf::Size {
                        width: px(width),
                        height: px(height),
                    },
                    ..item_half(inner, &style.layout)
                };
                (t, Link::Origin)
            }
            Role::Fill { parent } => {
                let full = Percent(100.0);
                let t = tf::Style {
                    size: tf::Size {
                        width: full.into(),
                        height: full.into(),
                    },
                    ..inner
                };
                (t, Link::Flow { parent })
            }
            Role::In(host) => match resolve(&style.placement, host.kind) {
                Resolved::Anchored(anchor) => {
                    // Only size and margin: `place_anchored` positions the box, so no flex/grid item field may move it.
                    let t = tf::Style {
                        position: tf::Position::Absolute,
                        align_self: None,
                        justify_self: None,
                        grid_row: tf::Line::<tf::GridPlacement>::AUTO,
                        grid_column: tf::Line::<tf::GridPlacement>::AUTO,
                        ..item_half(inner, &style.layout)
                    };
                    let link = Link::Anchored {
                        host: host.entry,
                        anchor,
                        safe: host.safe,
                    };
                    (t, link)
                }
                Resolved::Flow => {
                    let t = item_half(inner, &style.layout);
                    // A scroll child is laid out unconstrained on the scroll axis: it must not shrink to the viewport.
                    let flex_shrink = match host.kind {
                        HostKind::Scroll => 0.0,
                        HostKind::Taffy | HostKind::Anchors => t.flex_shrink,
                    };
                    let t = tf::Style { flex_shrink, ..t };
                    (t, Link::Flow { parent: host.entry })
                }
            },
        }
    }

    /// Adds the safe area (non-zero only on the root's own box) to the padding. The root box is the viewport, so a
    /// percent padding is resolved against the viewport width first (CSS percent padding is width-relative).
    fn padded(&self, inner: tf::Style, safe: Insets) -> tf::Style {
        let basis = self.viewport.size.width as f32;
        let add = |side: tf::LengthPercentage, extra: f64| -> tf::LengthPercentage {
            let own = match side.expand() {
                tf::ExpandedLengthPercentage::Length(v) => v,
                tf::ExpandedLengthPercentage::Percent(f) => f * basis,
                // What `TaffyTree` itself resolves a calc() to: kit styles never build one.
                tf::ExpandedLengthPercentage::Calc(_) => 0.0,
            };
            px(f64::from(own) + extra)
        };
        let p = inner.padding;
        let padding = tf::Rect {
            left: add(p.left, safe.x0),
            right: add(p.right, safe.x1),
            top: add(p.top, safe.y0),
            bottom: add(p.bottom, safe.y1),
        };
        tf::Style { padding, ..inner }
    }

    // -----------------------------------------------------------------------------------------------------------
    // Solve

    fn solve(&mut self, main: Built, text: &mut CosmicText) -> Vec<Placed> {
        let leaves = &self.leaves;
        let mut measure = |input: tf::LayoutInput,
                           _id: tf::NodeId,
                           ctx: Option<&mut usize>,
                           style: &tf::Style|
         -> tf::LayoutOutput {
            tf::compute_leaf_layout(
                input,
                style,
                |_, _| 0.0,
                |known, avail| match ctx {
                    None => known.unwrap_or(tf::Size::ZERO),
                    Some(index) => measure_leaf(&leaves[*index], text, known, avail),
                },
            )
        };
        let viewport_space = tf::Size {
            width: tf::AvailableSpace::Definite(self.viewport.size.width as f32),
            height: tf::AvailableSpace::Definite(self.viewport.size.height as f32),
        };
        match main {
            Built::InTree(root) => self
                .tree
                .compute_layout_with_measure(root, viewport_space, &mut measure)
                .expect("taffy layout on a live tree"),
            // The root itself is a world box: there is no main tree, only floating ones.
            Built::Detached => {}
        }
        let floating_space = tf::Size {
            width: tf::AvailableSpace::MaxContent,
            height: tf::AvailableSpace::MaxContent,
        };
        for id in &self.floating {
            self.tree
                .compute_layout_with_measure(*id, floating_space, &mut measure)
                .expect("taffy layout on a live tree");
        }
        self.collect()
    }

    fn collect(&self) -> Vec<Placed> {
        let mut placed: Vec<Placed> = Vec::with_capacity(self.slots.len());
        for slot in &self.slots {
            let here = match slot {
                Slot::Culled => Placed {
                    rect: Rect::ZERO,
                    shown: Shown::Culled,
                },
                // Patched below once the floating child's rect exists.
                Slot::Follow => Placed {
                    rect: Rect::ZERO,
                    shown: Shown::Visible,
                },
                Slot::Laid { id, link } => {
                    let layout = self.tree.layout(*id).expect("laid slot is a live node");
                    let size =
                        Size::new(f64::from(layout.size.width), f64::from(layout.size.height));
                    let at = Point::new(f64::from(layout.location.x), f64::from(layout.location.y));
                    let rect = match link {
                        Link::Origin => Rect::from_origin_size(at, size),
                        Link::FloatCenter(center) => Rect::from_center_size(*center, size),
                        Link::Flow { parent } => {
                            let origin = placed[*parent].rect.origin();
                            Rect::from_origin_size(origin + at.to_vec2(), size)
                        }
                        Link::Anchored { host, anchor, safe } => {
                            let m = layout.margin;
                            let margin = Insets::new(
                                f64::from(m.left),
                                f64::from(m.top),
                                f64::from(m.right),
                                f64::from(m.bottom),
                            );
                            place_anchored(placed[*host].rect, *safe, size, margin, anchor)
                        }
                    };
                    Placed {
                        rect,
                        shown: Shown::Visible,
                    }
                }
            };
            placed.push(here);
        }
        // Wrappers over a floating box share its rect; resolve back to front so chains settle.
        for i in (0..placed.len()).rev() {
            if let Slot::Follow = self.slots[i] {
                placed[i].rect = placed[i + 1].rect;
            }
        }
        placed
    }
}

/// Seconds of taffy measure: exactly one place where `f64` text metrics become `f32` layout input.
fn measure_leaf(
    leaf: &Leaf<'_>,
    text: &mut CosmicText,
    known: tf::Size<Option<f32>>,
    avail: tf::Size<tf::AvailableSpace>,
) -> tf::Size<f32> {
    if let (Some(width), Some(height)) = (known.width, known.height) {
        return tf::Size { width, height };
    }
    let room = match (known.width, avail.width) {
        (Some(w), _) => Room::Exactly(f64::from(w)),
        (None, tf::AvailableSpace::Definite(w)) => Room::Within(f64::from(w)),
        (None, tf::AvailableSpace::MinContent) => Room::MinContent,
        (None, tf::AvailableSpace::MaxContent) => Room::MaxContent,
    };
    let size = leaf.measure(text, room);
    tf::Size {
        width: known.width.unwrap_or_else(|| round_up(size.width)),
        height: known.height.unwrap_or_else(|| round_up(size.height)),
    }
}

/// `f64 -> f32` that never rounds down: a measured text width that lost precision would make the final re-layout in
/// `LaidOut::assemble` (at the f32 box width) wrap one word earlier than the measure did.
fn round_up(value: f64) -> f32 {
    let narrowed = value as f32;
    if f64::from(narrowed) < value {
        narrowed.next_up()
    } else {
        narrowed
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Style halves

/// `onto` with the item half of `from`: the fields by which a parent sizes and places a box (size limits, margin,
/// position, flex and grid item fields). Everything else (how the box arranges its children) stays `onto`'s.
fn item_half(onto: tf::Style, from: &tf::Style) -> tf::Style {
    tf::Style {
        size: from.size,
        min_size: from.min_size,
        max_size: from.max_size,
        aspect_ratio: from.aspect_ratio,
        margin: from.margin,
        position: from.position,
        inset: from.inset,
        flex_grow: from.flex_grow,
        flex_shrink: from.flex_shrink,
        flex_basis: from.flex_basis,
        align_self: from.align_self,
        justify_self: from.justify_self,
        grid_row: from.grid_row.clone(),
        grid_column: from.grid_column.clone(),
        ..onto
    }
}

/// `s` with its item half at taffy's defaults: only how the box arranges its children (and its padding) remains.
fn container_half(s: &tf::Style) -> tf::Style {
    item_half(s.clone(), &tf::Style::DEFAULT)
}

/// A container's display and what kind of host it is for its children.
fn arrange(t: tf::Style, content: &Content, role: Role) -> (tf::Style, HostKind) {
    match content {
        Content::Taffy => (t, HostKind::Taffy),
        Content::Anchors => {
            // An overlay has no content size: an `Auto` axis fills the host (the root is already the viewport).
            let fill = |d: tf::Dimension| match (role, d.is_auto()) {
                (Role::In(_) | Role::Float(_) | Role::Portal, true) => tf::Dimension::stretch(),
                (Role::In(_) | Role::Float(_) | Role::Portal, false)
                | (Role::Root | Role::Fill { .. }, _) => d,
            };
            let size = tf::Size {
                width: fill(t.size.width),
                height: fill(t.size.height),
            };
            let t = tf::Style {
                display: tf::Display::Block,
                size,
                ..t
            };
            (t, HostKind::Anchors)
        }
    }
}

/// A scroll viewport: a flex line along the scroll axis whose overflow scrolls.
fn scrolling(t: tf::Style, axis: Axis) -> tf::Style {
    let (flex_direction, items, x, y) = match axis {
        Axis::Vertical => (
            tf::FlexDirection::Column,
            tf::AlignItems::STRETCH,
            tf::Overflow::Visible,
            tf::Overflow::Scroll,
        ),
        Axis::Horizontal => (
            tf::FlexDirection::Row,
            tf::AlignItems::STRETCH,
            tf::Overflow::Scroll,
            tf::Overflow::Visible,
        ),
        Axis::Both => (
            tf::FlexDirection::Row,
            tf::AlignItems::FLEX_START,
            tf::Overflow::Scroll,
            tf::Overflow::Scroll,
        ),
    };
    tf::Style {
        display: tf::Display::Flex,
        flex_direction,
        align_items: Some(items),
        overflow: tf::Point { x, y },
        ..t
    }
}
