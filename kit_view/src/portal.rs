//! Portals: subtrees drawn above the normal tree, in layers, placed against an anchor instead of in their parent's flow.
//!
//! The vocabulary is here (what a component writes); the stacking rules every consumer must agree on (paint order,
//! which portals a click can reach, which one a modal blocks) are in [`Stacking`], computed once from a `LaidOut`.
use crate::{Anchor, LaidKind, LaidNode, LaidOut, Shown};
use kit_core::WidgetId;
use kit_core::kurbo::{Point, Rect};

/// Which layer a portal lives on. Declaration order is paint order: a later variant paints above an earlier one, and the
/// normal tree is below all of them.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub enum Layer {
    Popup,
    Modal,
    Toast,
    Tooltip,
}
impl Layer {
    /// Whether presses inside a portal on this layer stop at it even where the content is not interactive (a popup's
    /// background must not let a click fall through to the control behind it). Tooltips and toasts are click-through
    /// except where they hold controls of their own.
    pub fn is_opaque(self) -> bool {
        match self {
            Layer::Popup | Layer::Modal => true,
            Layer::Toast | Layer::Tooltip => false,
        }
    }
}

/// Which side of the anchor a node-anchored portal prefers. When it does not fit there the portal flips to the opposite
/// side if that fits, and is clamped into the viewport if neither does.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum Side {
    Below,
    Above,
    Right,
    Left,
}

/// What a portal is placed against.
#[derive(Copy, Clone, PartialEq, Debug)]
pub enum PortalAnchor {
    /// The viewport (inside its safe area unless the anchor says otherwise): modals, toasts.
    Viewport(Anchor),
    /// The laid rect of the node keyed `id`, `gap` logical px away on `side`. The rect is the node's layout rect: a
    /// scroll offset is not applied, so anchor to something outside a scrolled area.
    Node { id: WidgetId, side: Side, gap: f64 },
}

/// What dismisses a portal. Dismissal is data: the router emits the component's own intent, the component decides
/// what closing means (clear its open flag).
#[derive(Clone, PartialEq, Debug)]
pub enum Dismissal<Msg> {
    /// Stays until the component stops building it (tooltips, toasts that expire on their own).
    Never,
    /// A press outside the portal, or Escape while it is the topmost dismissible portal, sends this.
    Intent(Msg),
}
impl<Msg: 'static> Dismissal<Msg> {
    pub(crate) fn map_rc<M2: 'static>(self, f: &std::rc::Rc<dyn Fn(Msg) -> M2>) -> Dismissal<M2> {
        match self {
            Dismissal::Never => Dismissal::Never,
            Dismissal::Intent(msg) => Dismissal::Intent(f(msg)),
        }
    }
}

/// One portal of a laid-out tree, seen from outside.
pub struct PortalRef<'a, Msg> {
    /// The `LaidKind::Portal` node; its rect is its child's rect.
    pub node: &'a LaidNode<Msg>,
    pub layer: Layer,
    /// `max(layer, the enclosing portal's effective layer)`: a popup opened from inside a modal paints above the modal.
    pub effective: Layer,
    pub dismissal: &'a Dismissal<Msg>,
    pub child: &'a LaidNode<Msg>,
    /// Index (into [`Stacking::portals`]) of the portal this one is nested in. `Root` is the normal tree.
    pub parent: Parent,
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum Parent {
    Root,
    Portal(usize),
}

/// Every portal of a tree in paint order (bottom to top; ties keep tree order, so a nested portal paints after the one
/// it sits in). The one place the stacking rules live.
pub struct Stacking<'a, Msg> {
    pub portals: Vec<PortalRef<'a, Msg>>,
}

impl<'a, Msg> Stacking<'a, Msg> {
    pub fn of(laid: &'a LaidOut<Msg>) -> Self {
        let mut found: Vec<PortalRef<'a, Msg>> = Vec::new();
        collect(&laid.root, Layer::Popup, Parent::Root, &mut found);
        // `collect` numbered each portal's parent by discovery order; sorting by layer (stable, so ties keep tree
        // order) renumbers, so remember where everything went.
        let mut numbered: Vec<(usize, PortalRef<'a, Msg>)> =
            found.into_iter().enumerate().collect();
        numbered.sort_by_key(|(_, p)| p.effective);
        let mut position = vec![0; numbered.len()];
        for (new, (old, _)) in numbered.iter().enumerate() {
            position[*old] = new;
        }
        let portals = numbered
            .into_iter()
            .map(|(_, mut portal)| {
                portal.parent = match portal.parent {
                    Parent::Root => Parent::Root,
                    Parent::Portal(old) => Parent::Portal(position[old]),
                };
                portal
            })
            .collect();
        Self { portals }
    }

    /// Index of the topmost portal that is itself a modal. Everything painted before it is blocked: it is the focus
    /// scope, takes the Escape and receives every press; the normal tree is below all of them.
    pub fn modal(&self) -> ModalFloor {
        self.portals
            .iter()
            .rposition(|p| p.layer == Layer::Modal)
            .map_or(ModalFloor::None, ModalFloor::At)
    }

    /// Indices of the portals a pointer or key can reach, topmost first: everything from the modal up, or all of them
    /// when no modal is open.
    pub fn reachable(&self) -> Vec<usize> {
        let floor = match self.modal() {
            ModalFloor::None => 0,
            ModalFloor::At(i) => i,
        };
        (floor..self.portals.len()).rev().collect()
    }

    /// Whether `point` is inside portal `index` or a portal nested in it.
    pub fn inside(&self, index: usize, point: Point) -> bool {
        self.portals
            .iter()
            .enumerate()
            .any(|(j, p)| p.child.rect.contains(point) && self.nested_in(j, index))
    }

    /// Whether any opaque portal contains `point`.
    pub fn opaque_at(&self, point: Point) -> bool {
        self.reachable().into_iter().any(|i| {
            let p = &self.portals[i];
            p.layer.is_opaque() && p.child.rect.contains(point)
        })
    }

    fn nested_in(&self, mut j: usize, ancestor: usize) -> bool {
        loop {
            if j == ancestor {
                return true;
            }
            match self.portals[j].parent {
                Parent::Root => return false,
                Parent::Portal(p) => j = p,
            }
        }
    }
}

/// Whether a modal is open and where it sits in paint order.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum ModalFloor {
    None,
    At(usize),
}

fn collect<'a, Msg>(
    node: &'a LaidNode<Msg>,
    enclosing: Layer,
    parent: Parent,
    out: &mut Vec<PortalRef<'a, Msg>>,
) {
    match &node.kind {
        LaidKind::Portal {
            layer,
            dismissal,
            child,
            ..
        } => {
            let index = out.len();
            out.push(PortalRef {
                node,
                layer: *layer,
                effective: (*layer).max(enclosing),
                dismissal,
                child,
                parent,
            });
            collect(child, (*layer).max(enclosing), Parent::Portal(index), out);
        }
        LaidKind::World {
            shown: Shown::Culled,
            ..
        } => {}
        LaidKind::Stack { .. }
        | LaidKind::Panel { .. }
        | LaidKind::Text { .. }
        | LaidKind::Image { .. }
        | LaidKind::Shape { .. }
        | LaidKind::Interactive { .. }
        | LaidKind::Scroll { .. }
        | LaidKind::TextField { .. }
        | LaidKind::Layer { .. }
        | LaidKind::World { .. } => {
            for child in node.children() {
                collect(child, enclosing, parent, out);
            }
        }
    }
}

/// The rect a node-anchored portal of `size` occupies: on `side` of `anchor` (a `gap` away), flipped to the opposite
/// side when it does not fit inside `bounds` there and the opposite does, clamped into `bounds` on both axes.
pub fn place_beside(
    anchor: Rect,
    size: kit_core::kurbo::Size,
    bounds: Rect,
    side: Side,
    gap: f64,
) -> Rect {
    let (vertical, first, second) = match side {
        Side::Below => (true, Side::Below, Side::Above),
        Side::Above => (true, Side::Above, Side::Below),
        Side::Right => (false, Side::Right, Side::Left),
        Side::Left => (false, Side::Left, Side::Right),
    };
    let origin_on = |s: Side| -> Point {
        match s {
            Side::Below => Point::new(anchor.x0, anchor.y1 + gap),
            Side::Above => Point::new(anchor.x0, anchor.y0 - gap - size.height),
            Side::Right => Point::new(anchor.x1 + gap, anchor.y0),
            Side::Left => Point::new(anchor.x0 - gap - size.width, anchor.y0),
        }
    };
    // Room along the main axis on each side, to choose a side when neither fits.
    let room = |s: Side| -> f64 {
        match s {
            Side::Below => bounds.y1 - (anchor.y1 + gap),
            Side::Above => anchor.y0 - gap - bounds.y0,
            Side::Right => bounds.x1 - (anchor.x1 + gap),
            Side::Left => anchor.x0 - gap - bounds.x0,
        }
    };
    let need = if vertical { size.height } else { size.width };
    let chosen = if room(first) >= need {
        first
    } else if room(second) >= need {
        second
    } else if room(first) >= room(second) {
        first
    } else {
        second
    };
    let at = origin_on(chosen);
    let clamp = |v: f64, lo: f64, hi: f64, len: f64| v.clamp(lo, (hi - len).max(lo));
    Rect::from_origin_size(
        Point::new(
            clamp(at.x, bounds.x0, bounds.x1, size.width),
            clamp(at.y, bounds.y0, bounds.y1, size.height),
        ),
        size,
    )
}
