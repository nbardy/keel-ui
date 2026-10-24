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
use crate::kind::{Detached, Held, Kind, Leaf, Room, Tail, check_cells, detached, kind};
use kit_core::kurbo::{Insets, Point, Rect, Size, Vec2};
use kit_core::{Axis, Viewport};
use kit_view::taffy as tf;
use kit_view::taffy::style_helpers::TaffyAuto;
use kit_view::{
    Anchor, AnchorH, AnchorV, Content, CosmicText, LaidOut, LayoutError, LeafShaper, MaxWidth,
    Node, Percent, Placed, Placement, Projection, RichText, SafeArea, Shown, Style, TextLayout,
    TextStyle, WorldProjector, px,
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
/// taffy tree between frames. The `Node` tree is new every frame, so each call walks it in pre-order and, node by node,
/// makes the kept taffy tree match: a node whose place in the tree, style and measure input are what the kept node
/// already has costs a comparison and nothing else, and taffy's own caches then answer the solve; the first node
/// that differs in place cuts the kept tree off there and the rest is created. See [`Sink`].
pub struct TaffyLayout {
    tree: tf::TaffyTree<usize>,
    kept: Kept,
    reuse: Reuse,
}

/// What the last `layout` call did to the kept taffy tree. For tests and benches: nothing in layout depends on it.
/// `Reuse::default()` is a frame that changed nothing taffy can see.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub struct Reuse {
    /// Taffy nodes created: everything from the first node whose place in the tree differs from the kept tree's.
    pub created: usize,
    /// Kept taffy nodes dropped: the same tail of the kept tree (or all of it, for another text system).
    pub removed: usize,
    /// Kept nodes whose style was replaced.
    pub restyled: usize,
    /// Kept nodes whose measure input (text, text style, field, image size) changed, so taffy must measure again.
    pub remeasured: usize,
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
        Self {
            tree,
            kept: Kept {
                text: 0,
                nodes: Vec::new(),
            },
            reuse: Reuse::default(),
        }
    }

    /// What the last `layout` call did to the kept tree.
    pub fn last_reuse(&self) -> Reuse {
        self.reuse
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
        let count = root.count();
        let (placed, slots) = {
            let mut frame = Frame {
                viewport,
                world,
                sink: Sink::start(&mut self.tree, &mut self.kept, text.instance(), count),
                slots: Vec::with_capacity(count),
                floating: Vec::new(),
            };
            let main = frame.build(&root, Role::Root, true, Hang::Top);
            let Frame {
                viewport,
                sink,
                slots,
                floating,
                ..
            } = frame;
            let (reuse, mut leaves) = sink.finish();
            self.reuse = reuse;
            let solving = Solve {
                viewport,
                slots,
                floating,
            };
            let placed = solving.solve(&mut self.tree, &self.kept, main, &mut leaves, text);
            (placed, solving.slots)
        };
        let homes = slots.iter().map(Home::of).collect();
        let mut laid = LaidOut::assemble_with(
            root,
            viewport,
            &placed,
            text,
            &mut Recall {
                kept: &mut self.kept,
                homes,
            },
        )?;
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
        /// Index into `Kept::nodes`: the node's place in pre-order among the laid nodes.
        entry: usize,
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
    viewport: Viewport,
    world: &'a dyn WorldProjector,
    /// Makes the kept taffy tree hold each node as it is built.
    sink: Sink<'a, 'n>,
    slots: Vec<Slot>,
    /// `Kept::nodes` indices of the roots of detached world trees, laid out after the main tree.
    floating: Vec<usize>,
}

/// Where a taffy node hangs in the taffy tree.
#[derive(Copy, Clone, PartialEq, Eq)]
enum Hang {
    /// A tree root: the main tree's, or a detached world or portal box's.
    Top,
    /// The next child of the node at this index in `Kept::nodes` (children arrive in order).
    Under(usize),
}

/// The taffy tree the previous `layout` call left: one node per laid node, in pre-order.
struct Kept {
    /// `CosmicText::instance` that measured the leaves. Zero when nothing is kept.
    text: u64,
    nodes: Vec<KeptNode>,
}

struct KeptNode {
    id: tf::NodeId,
    hang: Hang,
    /// The layout `LaidOut::assemble` last got for this node, when it is a text leaf; see [`Recall`].
    shaped: Shaped,
    /// What the node was last measured with; the taffy node's context is its index in `nodes`.
    held: Held,
}

/// A text leaf's last layout at its final width. Valid exactly as long as `held` is: it answers a request made of the
/// leaf's text and style (both in `Held`) and the width.
#[derive(Copy, Clone)]
enum Shaped {
    /// Not shaped yet since the leaf was created or its text or style changed (and always, for other kinds of node).
    Not,
    At {
        width: MaxWidth,
        layout: TextLayout,
    },
}

/// Where a pre-order node of the `Placed` list lives in the kept tree.
#[derive(Copy, Clone)]
enum Home {
    Kept(usize),
    /// Culled, or a wrapper that shares its child's box: no kept node.
    Elsewhere,
}

impl Home {
    fn of(slot: &Slot) -> Home {
        match slot {
            Slot::Laid { entry, .. } => Home::Kept(*entry),
            Slot::Culled | Slot::Follow => Home::Elsewhere,
        }
    }
}

/// `assemble`'s text leaves, answered from the kept tree. `assemble` re-asks the text system for every text leaf at its
/// final width on every frame, and an unchanged frame had nothing else left to do with the text: each ask builds and
/// hashes a full cache key (text copy, span list, attrs) to get back the layout the leaf had last frame. A leaf whose
/// text, style and width are what it was shaped with (`Held` is dropped on any change of the first two) keeps that
/// layout, and `CosmicText::reuse` marks it used so the text cache does not age it out. If the cache lost it anyway
/// (a frame that never called `layout` let it age out), the leaf is asked again.
struct Recall<'a> {
    kept: &'a mut Kept,
    homes: Vec<Home>,
}

impl LeafShaper for Recall<'_> {
    fn shape(
        &mut self,
        node: usize,
        rich: &RichText,
        style: &TextStyle,
        width: MaxWidth,
        text: &mut CosmicText,
    ) -> TextLayout {
        match self.homes[node] {
            Home::Elsewhere => text.layout(rich, style, width),
            Home::Kept(entry) => {
                let slot = &mut self.kept.nodes[entry].shaped;
                let remembered = match *slot {
                    Shaped::At { width: w, layout } if w == width => text.reuse(layout.id),
                    Shaped::At { .. } | Shaped::Not => None,
                };
                let layout = remembered.unwrap_or_else(|| text.layout(rich, style, width));
                *slot = Shaped::At { width, layout };
                layout
            }
        }
    }
}

/// Which side of the first difference a node is on.
enum Mode {
    /// Every node so far matched the kept tree's place for it, so the kept node at this index is the one to compare.
    Patching,
    /// The kept tree was cut off at the first node that did not fit; everything from there is new.
    Appending,
}

/// Where the frame's nodes go. `node` takes the nodes in pre-order, as the frame builds them, and leaves the kept
/// taffy tree holding exactly those. Nothing is planned first: an unchanged node costs a style comparison and no
/// copy (a taffy `Style` is hundreds of bytes, and a plan of them was the largest cost of an unchanged frame).
struct Sink<'a, 'n> {
    tree: &'a mut tf::TaffyTree<usize>,
    kept: &'a mut Kept,
    mode: Mode,
    /// Measure state of each node so far (index = the node's context).
    leaves: Vec<Measured<'n>>,
    reuse: Reuse,
}

impl<'a, 'n> Sink<'a, 'n> {
    fn start(
        tree: &'a mut tf::TaffyTree<usize>,
        kept: &'a mut Kept,
        text: u64,
        capacity: usize,
    ) -> Self {
        let mut sink = Sink {
            tree,
            kept,
            mode: Mode::Patching,
            leaves: Vec::with_capacity(capacity),
            reuse: Reuse::default(),
        };
        // Sizes another text system measured are not this one's: keep nothing.
        if sink.kept.text != text {
            sink.cut(0);
            sink.kept.text = text;
        }
        sink
    }

    fn node(&mut self, style: tf::Style, hang: Hang, leaf: Leaf<'n>) -> usize {
        let index = self.leaves.len();
        match self.mode {
            Mode::Patching => self.patch(index, style, hang, &leaf),
            Mode::Appending => self.append(index, style, hang, &leaf),
        }
        self.leaves.push(Measured::new(leaf));
        index
    }

    /// The kept node at `index` if it hangs where this one does, else the kept tree ends here and the node is new.
    fn patch(&mut self, index: usize, style: tf::Style, hang: Hang, leaf: &Leaf<'n>) {
        match self.kept.nodes.get_mut(index).filter(|k| k.hang == hang) {
            Some(kept) => {
                let differs = self.tree.style(kept.id).expect("kept node is live") != &style;
                if differs {
                    self.tree
                        .set_style(kept.id, style)
                        .expect("kept node is live");
                    self.reuse.restyled += 1;
                }
                if !kept.held.is(leaf) {
                    kept.held = leaf.hold();
                    kept.shaped = Shaped::Not;
                    self.tree.mark_dirty(kept.id).expect("kept node is live");
                    self.reuse.remeasured += 1;
                }
            }
            None => {
                self.cut(index);
                self.mode = Mode::Appending;
                self.append(index, style, hang, leaf);
            }
        }
    }

    fn append(&mut self, index: usize, style: tf::Style, hang: Hang, leaf: &Leaf<'n>) {
        let id = self
            .tree
            .new_leaf_with_context(style, index)
            .expect("taffy new_leaf is infallible");
        hang.attach(self.tree, &self.kept.nodes, id);
        self.kept.nodes.push(KeptNode {
            id,
            hang,
            shaped: Shaped::Not,
            held: leaf.hold(),
        });
        self.reuse.created += 1;
    }

    /// Drops the kept nodes from `from` on. Each parent of a dropped node precedes it in pre-order, so every kept
    /// node left still has all its (kept) ancestors, and taffy forgets the dropped ones as children as it removes them.
    fn cut(&mut self, from: usize) {
        for dropped in self.kept.nodes.drain(from..) {
            self.tree.remove(dropped.id).expect("kept node is live");
            self.reuse.removed += 1;
        }
    }

    /// The frame's nodes are all in: whatever is left of an older, longer kept tree goes.
    fn finish(mut self) -> (Reuse, Vec<Measured<'n>>) {
        let built = self.leaves.len();
        self.cut(built);
        (self.reuse, self.leaves)
    }
}

impl Hang {
    /// Adds `id` (the node just created, whose parent is already in `nodes`) under its parent.
    fn attach(self, tree: &mut tf::TaffyTree<usize>, nodes: &[KeptNode], id: tf::NodeId) {
        match self {
            Hang::Top => {}
            Hang::Under(parent) => tree
                .add_child(nodes[parent].id, id)
                .expect("taffy add_child on live nodes"),
        }
    }
}

/// A leaf and every answer it has given this frame. Taffy asks the same leaf the same question many times per solve
/// (flex runs several sizing passes per item, and every ancestor row/column re-asks), and each ask through
/// `CosmicText::layout` builds and hashes a full cache key (text copy, span list, attrs). Measured on the 100-row
/// bench list (3 text leaves per row): 46 `layout` calls per row per frame, 24 us/row, all cache hits. Remembering
/// the answer per `Room` makes it one call per distinct room; without it list layout is ~10x slower.
struct Measured<'n> {
    leaf: Leaf<'n>,
    answers: Vec<(Room, Size)>,
}

impl<'n> Measured<'n> {
    fn new(leaf: Leaf<'n>) -> Self {
        Self {
            leaf,
            answers: Vec::new(),
        }
    }

    fn measure(&mut self, text: &mut CosmicText, room: Room) -> Size {
        let room = self.leaf.canonical(room);
        match self.answers.iter().find(|(asked, _)| *asked == room) {
            Some((_, size)) => *size,
            None => {
                let size = self.leaf.measure(text, room);
                self.answers.push((room, size));
                size
            }
        }
    }
}

/// Built node: either part of its parent's taffy tree (at this index of `Kept::nodes`) or a detached world box.
enum Built {
    InTree(usize),
    Detached,
}

impl<'n> Frame<'_, 'n> {
    /// Hands a node to the sink and returns its index.
    fn plan_node(&mut self, style: tf::Style, hang: Hang, leaf: Leaf<'n>) -> usize {
        self.sink.node(style, hang, leaf)
    }

    fn push_laid(&mut self, entry: usize, link: Link) -> usize {
        self.slots.push(Slot::Laid { entry, link });
        self.slots.len() - 1
    }

    fn cull<Msg>(&mut self, node: &Node<Msg>) {
        for _ in 0..node.count() {
            self.slots.push(Slot::Culled);
        }
    }

    /// Pre-order build of `node`. `root_content` is true along the root's wrapper chain down to its real container.
    fn build<Msg>(
        &mut self,
        node: &'n Node<Msg>,
        role: Role,
        root_content: bool,
        hang: Hang,
    ) -> Built {
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
                let mut t = tf::Style {
                    display: tf::Display::Flex,
                    flex_direction: tf::FlexDirection::Column,
                    ..tf::Style::DEFAULT
                };
                let link = self.place(&mut t, style, role);
                let entry = self.plan_node(t, hang, Leaf::Empty);
                let at = self.push_laid(entry, link);
                self.build(
                    child,
                    Role::Fill { parent: at },
                    root_content,
                    Hang::Under(entry),
                );
                Built::InTree(entry)
            }
            Kind::Leaf(leaf) => {
                let mut t = container_half(&style.layout);
                let link = self.place(&mut t, style, role);
                let entry = self.plan_node(t, hang, leaf);
                self.push_laid(entry, link);
                Built::InTree(entry)
            }
            Kind::Container(children) => {
                let mut t = container_half(&style.layout);
                self.pad(&mut t, safe);
                let link = self.place(&mut t, style, role);
                let host_kind = arrange(&mut t, &style.content, role);
                let entry = self.plan_node(t, hang, Leaf::Empty);
                let at = self.push_laid(entry, link);
                let host = Host {
                    kind: host_kind,
                    entry: at,
                    safe,
                };
                for child in children {
                    self.build(child, Role::In(host), false, Hang::Under(entry));
                }
                Built::InTree(entry)
            }
            Kind::Scroll { axis, child } => {
                let mut t = container_half(&style.layout);
                scrolling(&mut t, axis);
                self.pad(&mut t, safe);
                let link = self.place(&mut t, style, role);
                let entry = self.plan_node(t, hang, Leaf::Empty);
                let at = self.push_laid(entry, link);
                let host = Host {
                    kind: HostKind::Scroll,
                    entry: at,
                    safe: Insets::ZERO,
                };
                self.build(child, Role::In(host), false, Hang::Under(entry));
                Built::InTree(entry)
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
                if let Built::InTree(entry) = self.build(chain.child, role, false, Hang::Top) {
                    self.floating.push(entry);
                }
            }
        }
        Built::Detached
    }

    // -----------------------------------------------------------------------------------------------------------
    // Kit policy on top of the node's own taffy style

    /// Adds to `t` (how the box arranges its children) everything the role decides about the box itself: size
    /// limits, margin, flex/grid item fields, position. In place: a taffy `Style` is large, and every copy of it is
    /// paid per node per frame, whether or not the node changed.
    fn place(&self, t: &mut tf::Style, style: &Style, role: Role) -> Link {
        match role {
            Role::Root => {
                let Size { width, height } = self.viewport.size;
                t.size = tf::Size {
                    width: px(width),
                    height: px(height),
                };
                Link::Origin
            }
            Role::Float(center) => {
                set_item_half(t, &style.layout);
                t.margin = tf::Rect::zero();
                Link::FloatCenter(center)
            }
            Role::Portal => {
                // A portal never outgrows the screen; its margin is not part of where an anchor puts it.
                let Size { width, height } = self.viewport.size;
                set_item_half(t, &style.layout);
                t.margin = tf::Rect::zero();
                t.max_size = tf::Size {
                    width: px(width),
                    height: px(height),
                };
                Link::Origin
            }
            Role::Fill { parent } => {
                let full = Percent(100.0);
                t.size = tf::Size {
                    width: full.into(),
                    height: full.into(),
                };
                Link::Flow { parent }
            }
            Role::In(host) => match resolve(&style.placement, host.kind) {
                Resolved::Anchored(anchor) => {
                    // Only size and margin: `place_anchored` positions the box, so no flex/grid item field may move it.
                    set_item_half(t, &style.layout);
                    t.position = tf::Position::Absolute;
                    t.align_self = None;
                    t.justify_self = None;
                    t.grid_row = tf::Line::<tf::GridPlacement>::AUTO;
                    t.grid_column = tf::Line::<tf::GridPlacement>::AUTO;
                    Link::Anchored {
                        host: host.entry,
                        anchor,
                        safe: host.safe,
                    }
                }
                Resolved::Flow => {
                    set_item_half(t, &style.layout);
                    // A scroll child is laid out unconstrained on the scroll axis: it must not shrink to the viewport.
                    t.flex_shrink = match host.kind {
                        HostKind::Scroll => 0.0,
                        HostKind::Taffy | HostKind::Anchors => t.flex_shrink,
                    };
                    Link::Flow { parent: host.entry }
                }
            },
        }
    }

    /// Adds the safe area (non-zero only on the root's own box) to the padding. The root box is the viewport, so a
    /// percent padding is resolved against the viewport width first (CSS percent padding is width-relative).
    fn pad(&self, t: &mut tf::Style, safe: Insets) {
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
        let p = t.padding;
        t.padding = tf::Rect {
            left: add(p.left, safe.x0),
            right: add(p.right, safe.x1),
            top: add(p.top, safe.y0),
            bottom: add(p.bottom, safe.y1),
        };
    }
}

/// What is left of a frame once its nodes are in the kept tree: how to read the solved tree back into rects.
struct Solve {
    viewport: Viewport,
    slots: Vec<Slot>,
    /// `Kept::nodes` indices of the roots of detached world trees, laid out after the main tree.
    floating: Vec<usize>,
}

impl Solve {
    // -----------------------------------------------------------------------------------------------------------
    // Solve

    fn solve(
        &self,
        tree: &mut tf::TaffyTree<usize>,
        kept: &Kept,
        main: Built,
        leaves: &mut [Measured<'_>],
        text: &mut CosmicText,
    ) -> Vec<Placed> {
        let mut measure = |input: tf::LayoutInput,
                           _id: tf::NodeId,
                           ctx: Option<&mut usize>,
                           style: &tf::Style|
         -> tf::LayoutOutput {
            tf::compute_leaf_layout(
                input,
                style,
                |_, _| 0.0,
                |known, avail| {
                    let index = ctx.expect("every kit node is created with its index");
                    measure_leaf(&mut leaves[*index], text, known, avail)
                },
            )
        };
        let viewport_space = tf::Size {
            width: tf::AvailableSpace::Definite(self.viewport.size.width as f32),
            height: tf::AvailableSpace::Definite(self.viewport.size.height as f32),
        };
        match main {
            Built::InTree(root) => tree
                .compute_layout_with_measure(kept.nodes[root].id, viewport_space, &mut measure)
                .expect("taffy layout on a live tree"),
            // The root itself is a world box: there is no main tree, only floating ones.
            Built::Detached => {}
        }
        let floating_space = tf::Size {
            width: tf::AvailableSpace::MaxContent,
            height: tf::AvailableSpace::MaxContent,
        };
        for root in &self.floating {
            tree.compute_layout_with_measure(kept.nodes[*root].id, floating_space, &mut measure)
                .expect("taffy layout on a live tree");
        }
        self.collect(tree, kept)
    }

    fn collect(&self, tree: &tf::TaffyTree<usize>, kept: &Kept) -> Vec<Placed> {
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
                Slot::Laid { entry, link } => {
                    let layout = tree
                        .layout(kept.nodes[*entry].id)
                        .expect("laid slot is a live node");
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
    leaf: &mut Measured<'_>,
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

/// Copies the item half of `from` into `onto`: the fields by which a parent sizes and places a box (size limits,
/// margin, position, flex and grid item fields). Everything else (how the box arranges its children) stays `onto`'s.
fn set_item_half(onto: &mut tf::Style, from: &tf::Style) {
    onto.size = from.size;
    onto.min_size = from.min_size;
    onto.max_size = from.max_size;
    onto.aspect_ratio = from.aspect_ratio;
    onto.margin = from.margin;
    onto.position = from.position;
    onto.inset = from.inset;
    onto.flex_grow = from.flex_grow;
    onto.flex_shrink = from.flex_shrink;
    onto.flex_basis = from.flex_basis;
    onto.align_self = from.align_self;
    onto.justify_self = from.justify_self;
    onto.grid_row = from.grid_row.clone();
    onto.grid_column = from.grid_column.clone();
}

/// `s` with its item half at taffy's defaults: only how the box arranges its children (and its padding) remains.
fn container_half(s: &tf::Style) -> tf::Style {
    let mut t = s.clone();
    set_item_half(&mut t, &tf::Style::DEFAULT);
    t
}

/// A container's display and what kind of host it is for its children.
fn arrange(t: &mut tf::Style, content: &Content, role: Role) -> HostKind {
    match content {
        Content::Taffy => HostKind::Taffy,
        Content::Anchors => {
            // An overlay has no content size: an `Auto` axis fills the host (the root is already the viewport).
            let fill = |d: tf::Dimension| match (role, d.is_auto()) {
                (Role::In(_) | Role::Float(_) | Role::Portal, true) => tf::Dimension::stretch(),
                (Role::In(_) | Role::Float(_) | Role::Portal, false)
                | (Role::Root | Role::Fill { .. }, _) => d,
            };
            t.size = tf::Size {
                width: fill(t.size.width),
                height: fill(t.size.height),
            };
            t.display = tf::Display::Block;
            HostKind::Anchors
        }
    }
}

/// A scroll viewport: a flex line along the scroll axis whose overflow scrolls.
fn scrolling(t: &mut tf::Style, axis: Axis) {
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
    t.display = tf::Display::Flex;
    t.flex_direction = flex_direction;
    t.align_items = Some(items);
    t.overflow = tf::Point { x, y };
}
