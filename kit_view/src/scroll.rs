//! What a scroll area asks of the router: axes, stick-to-end, scroll-to requests and bars. Plain data on the node;
//! the physics that act on it live in `kit_input::scroll`.
use core::hash::Hash;
use kit_core::kurbo::Rect;
use kit_core::{Axis, StableHasher, WidgetId};
use std::hash::Hasher;

#[derive(Copy, Clone, PartialEq, Debug)]
pub struct ScrollSpec {
    pub axis: Axis,
    pub stick: Stick,
    pub reveal: Reveal,
    pub bars: Bars,
}

/// Whether the area follows its content's end as it grows (chat, logs).
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum Stick {
    Free,
    /// Pinned to the end while the user leaves it there; a user scroll away unpins, scrolling back pins again.
    End,
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum Bars {
    /// Thin bars over the content, shown while hovered or scrolling, with a draggable thumb.
    Auto,
    Hidden,
}

/// Where the revealed target lands in the viewport.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum RevealAlign {
    Start,
    Center,
    End,
    /// The smallest move that brings the target fully into view; none if it already is.
    Nearest,
}

#[derive(Copy, Clone, PartialEq, Debug)]
pub enum RevealTarget {
    /// A keyed node inside the area.
    Child(WidgetId),
    /// A rect in content coordinates (offset zero): rows a virtual list has not built.
    Content(Rect),
}

/// A scroll-to request. Edge-triggered: the router applies each `ticket` once, so the user can scroll away
/// afterwards. A target that is not laid out yet stays pending until it is.
#[derive(Copy, Clone, PartialEq, Debug)]
pub enum Reveal {
    Nothing,
    Request {
        target: RevealTarget,
        align: RevealAlign,
        ticket: u64,
    },
}

impl ScrollSpec {
    pub fn new(axis: Axis) -> Self {
        Self {
            axis,
            stick: Stick::Free,
            reveal: Reveal::Nothing,
            bars: Bars::Auto,
        }
    }
    pub fn stick_to_end(self) -> Self {
        Self {
            stick: Stick::End,
            ..self
        }
    }
    pub fn bars(self, bars: Bars) -> Self {
        Self { bars, ..self }
    }
    /// Bring the child keyed by `key` into view whenever `key` changes (a keyboard selection, a search hit).
    pub fn reveal_child(self, key: &impl Hash, align: RevealAlign) -> Self {
        let id = WidgetId::of(key);
        self.reveal(RevealTarget::Child(id), align, &id)
    }
    /// Reveal `target` once per distinct `stamp` (a message count, a "jump" press counter).
    pub fn reveal(self, target: RevealTarget, align: RevealAlign, stamp: &impl Hash) -> Self {
        let mut hasher = StableHasher::new();
        stamp.hash(&mut hasher);
        Self {
            reveal: Reveal::Request {
                target,
                align,
                ticket: hasher.finish(),
            },
            ..self
        }
    }
}
