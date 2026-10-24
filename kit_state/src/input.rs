//! The interaction seam: what a component may ask about the pointer and focus. Implemented by the input router's
//! memory (kit_input), so kit_state stays renderer- and router-agnostic and depends on nothing but kit_core ids.
use kit_core::kurbo::Vec2;
use kit_core::{ScrollView, WidgetId};

/// A pointer drag that began on this widget, in logical px.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Drag {
    Idle,
    /// `total`: from the press origin to the pointer now. `delta`: since the router's previous frame.
    Held {
        delta: Vec2,
        total: Vec2,
    },
}

/// Read-only interaction state as of the router's last pass (the previous layout). Ids are keyed `WidgetId`s.
pub trait Interaction {
    fn hovered(&self, id: WidgetId) -> bool;
    /// A pointer pressed this widget and is still captured by it (press or drag).
    fn pressed(&self, id: WidgetId) -> bool;
    fn focused(&self, id: WidgetId) -> bool;
    fn drag(&self, id: WidgetId) -> Drag;
    /// A scroll area's offset and sizes as the router last measured them.
    fn scroll(&self, id: WidgetId) -> ScrollView;
}

/// No input has been routed to this tree: nothing is hovered, pressed, focused or dragged. Named so a host that has
/// not wired its router says so, instead of the reads quietly answering "no".
#[derive(Copy, Clone, Debug)]
pub struct Unrouted;
impl Interaction for Unrouted {
    fn hovered(&self, _: WidgetId) -> bool {
        false
    }
    fn pressed(&self, _: WidgetId) -> bool {
        false
    }
    fn focused(&self, _: WidgetId) -> bool {
        false
    }
    fn drag(&self, _: WidgetId) -> Drag {
        Drag::Idle
    }
    fn scroll(&self, _: WidgetId) -> ScrollView {
        ScrollView::Unseen
    }
}
