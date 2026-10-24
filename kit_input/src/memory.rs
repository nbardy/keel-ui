use crate::clipboard::ClipboardRequest;
use crate::field::{ClickRun, Granularity};
use crate::{PointerId, PointerKind};
use kit_core::kurbo::{Point, Vec2};
use kit_core::{Epoch, Time, Viewport, WidgetId};
use kit_state::{Drag, Interaction};
use kit_view::DragPayload;
use kit_view::kit_text::Selection;
use std::collections::BTreeMap;

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum Focus {
    Unfocused,
    On(WidgetId),
}
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum Hover {
    Nothing,
    Over(WidgetId),
}

/// What one pointer is doing. Every variant remembers the epoch it began under; the router drops it the moment the
/// laid-out screen's epoch differs.
#[derive(Copy, Clone, PartialEq, Debug)]
pub enum Gesture {
    Idle,
    /// Pressed on a region that can activate or drag. `scroll` is the innermost scroll area, if any.
    Press {
        source: WidgetId,
        scroll: Option<WidgetId>,
        origin: Point,
        epoch: Epoch,
    },
    /// Pressed inside a scroll area on nothing capturable.
    Backdrop {
        scroll: WidgetId,
        origin: Point,
        epoch: Epoch,
    },
    Drag {
        source: WidgetId,
        payload: DragPayload,
        /// Where the press that became this drag began (`Interaction::drag` measures `total` from it).
        origin: Point,
        epoch: Epoch,
    },
    Pan {
        scroll: WidgetId,
        /// Content (drag the content) or a bar's thumb (`kit_input::scroll`).
        grip: crate::scroll::Grip,
        last: Point,
        epoch: Epoch,
    },
    /// track: a pointer captured by a node with a pointer binding (`Bindings::on_pointer`), from press to release.
    /// `shift` = screen point minus the node's layout-space point at the press (the scroll offsets above it), so
    /// later positions map into the node even when the pointer is outside it.
    Track {
        source: WidgetId,
        shift: Vec2,
        origin: Point,
        travel: kit_view::Travel,
        epoch: Epoch,
    },
    /// A secondary-button press on a control that binds a context action (`Bindings::on_context`). It fires on
    /// release over the same control; it never activates, drags or takes focus.
    Context {
        source: WidgetId,
        epoch: Epoch,
    },
    /// A press that began in a text field and grows its selection from `anchor`, a word or a line at a time.
    Select {
        field: WidgetId,
        anchor: Selection,
        by: Granularity,
        epoch: Epoch,
    },
}
impl Gesture {
    pub fn epoch(&self) -> Option<Epoch> {
        match self {
            Gesture::Idle => None,
            Gesture::Press { epoch, .. }
            | Gesture::Backdrop { epoch, .. }
            | Gesture::Drag { epoch, .. }
            | Gesture::Pan { epoch, .. }
            | Gesture::Track { epoch, .. }
            | Gesture::Context { epoch, .. }
            | Gesture::Select { epoch, .. } => Some(*epoch),
        }
    }
}

#[derive(Copy, Clone, PartialEq, Debug)]
pub struct PointerState {
    pub kind: PointerKind,
    pub at: Point,
    /// Where the pointer was when the host last called [`UiMemory::end_frame`]: the base of `Drag::Held::delta`.
    pub frame_prev: Point,
    pub gesture: Gesture,
    pub hover: Hover,
}

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Debug)]
pub enum AnimKey {
    Hover,
    Press,
    Focus,
    /// The last edit of a focused field: the caret stops blinking and shows.
    Caret,
}

/// A drag in flight, for painting the ghost.
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct DragView {
    pub pointer: PointerId,
    pub source: WidgetId,
    pub at: Point,
}

/// The one retained UI store: scroll offsets, focus, pointer capture/drag state, the click run, the clipboard outbox, animation clocks.
/// Ordered maps only, so iteration (and therefore replay) is deterministic.
#[derive(Clone, Debug)]
pub struct UiMemory {
    pub(crate) viewport: Viewport,
    pub(crate) window_focused: bool,
    pub(crate) scroll: crate::scroll::Scrolls,
    pub(crate) focus: Focus,
    /// Last completed frame's focus, for detecting host requests/claims after view reads.
    pub(crate) frame_focus: Focus,
    /// Frame maintenance changed a value the view may already have read. One follow-up build is owed.
    pub(crate) frame_changed: bool,
    pub(crate) pointers: BTreeMap<PointerId, PointerState>,
    pub(crate) clicks: ClickRun,
    pub(crate) clipboard: Vec<ClipboardRequest>,
    pub(crate) anim: BTreeMap<(WidgetId, AnimKey), Time>,
    pub(crate) focus_misses: u64,
    /// keys: the modifiers held, as the last `Key` or `Modifiers` event said (read by modifier-aware presses).
    pub(crate) mods: kit_core::Modifiers,
    /// keys: focus as text fields were last told it (`Edit::Focus`/`Blur` are the difference to `focus`).
    pub(crate) told: Focus,
    /// keys: the focus-claim ticket last applied per node that carries one (edge-triggered, see `FocusClaim`).
    pub(crate) claims: BTreeMap<WidgetId, u64>,
    /// The openers of the portals that were open (and said where focus returns) when focus restoration last looked,
    /// in portal order. One that is missing from the next look closed; see `focus::restore`.
    pub(crate) openers: Vec<WidgetId>,
    /// Closed portals whose opener could not take focus back, so the fallback rule chose (see `focus::Restore`).
    pub(crate) restore_fallbacks: u64,
}

impl UiMemory {
    pub fn new(viewport: Viewport) -> Self {
        Self {
            viewport,
            window_focused: true,
            scroll: crate::scroll::Scrolls::default(),
            focus: Focus::Unfocused,
            frame_focus: Focus::Unfocused,
            frame_changed: false,
            pointers: BTreeMap::new(),
            clicks: ClickRun::Nothing,
            clipboard: Vec::new(),
            anim: BTreeMap::new(),
            focus_misses: 0,
            mods: kit_core::Modifiers::default(),
            told: Focus::Unfocused,
            claims: BTreeMap::new(),
            openers: Vec::new(),
            restore_fallbacks: 0,
        }
    }
    /// Forget everything except the viewport (screen change, tests).
    pub fn reset(&mut self) {
        *self = Self::new(self.viewport);
    }
    /// Viewport as last reported by `Resize` / `ScaleFactor` / `SafeArea` events; feed it to the layout engine.
    pub fn viewport(&self) -> Viewport {
        self.viewport
    }
    pub fn window_focused(&self) -> bool {
        self.window_focused
    }
    /// Scroll offset in logical px; a never-scrolled area is at zero (that is its state, not a missing value).
    pub fn scroll_offset(&self, id: WidgetId) -> Vec2 {
        self.scroll.offset(id)
    }
    pub fn focus(&self) -> Focus {
        self.focus
    }
    /// Focus requests that named nothing focusable (see `Router::request_focus`). A host or test that expects its
    /// request to land asserts this stayed zero; the count is how a dead key is noticed.
    pub fn focus_misses(&self) -> u64 {
        self.focus_misses
    }
    /// Popups that closed while their opener could not take focus back (it was removed, disabled or behind a modal),
    /// so focus went to the fallback instead (see `Router::frame`). A host or test that expects the opener to still
    /// be there asserts this stayed zero.
    pub fn restore_fallbacks(&self) -> u64 {
        self.restore_fallbacks
    }
    /// Close the frame: every pointer's position becomes the base of next frame's `Drag::Held::delta`. Call once per
    /// frame, after the view that reads `drag` was built.
    pub fn end_frame(&mut self) {
        for state in self.pointers.values_mut() {
            if matches!(state.gesture, Gesture::Drag { .. } | Gesture::Track { .. })
                && state.frame_prev != state.at
            {
                self.frame_changed = true;
            }
            state.frame_prev = state.at;
        }
    }
    /// Copy and paste requests the UI made since the last call, for the host's `Clipboard` (see `service`).
    pub fn take_clipboard(&mut self) -> Vec<ClipboardRequest> {
        std::mem::take(&mut self.clipboard)
    }
    pub fn anim_start(&self, id: WidgetId, key: AnimKey) -> Option<Time> {
        self.anim.get(&(id, key)).copied()
    }
    pub fn pointer(&self, id: PointerId) -> Option<&PointerState> {
        self.pointers.get(&id)
    }
    pub fn is_hovered(&self, id: WidgetId) -> bool {
        self.pointers.values().any(|p| p.hover == Hover::Over(id))
    }
    /// True while a pointer that pressed this widget is still captured by it (press or drag).
    pub fn is_pressed(&self, id: WidgetId) -> bool {
        self.pointers.values().any(|p| match p.gesture {
            Gesture::Press { source, .. }
            | Gesture::Drag { source, .. }
            | Gesture::Track { source, .. } => source == id,
            Gesture::Idle
            | Gesture::Backdrop { .. }
            | Gesture::Pan { .. }
            | Gesture::Context { .. }
            | Gesture::Select { .. } => false,
        })
    }
    /// Drags that are valid for `epoch` (a stale drag shows no ghost even before the router has pruned it).
    pub fn live_drags(&self, epoch: Epoch) -> Vec<DragView> {
        self.pointers
            .iter()
            .filter_map(|(pointer, state)| match state.gesture {
                Gesture::Drag {
                    source, epoch: e, ..
                } if e == epoch => Some(DragView {
                    pointer: *pointer,
                    source,
                    at: state.at,
                }),
                Gesture::Idle
                | Gesture::Press { .. }
                | Gesture::Backdrop { .. }
                | Gesture::Pan { .. }
                | Gesture::Select { .. }
                | Gesture::Context { .. }
                | Gesture::Track { .. }
                | Gesture::Drag { .. } => None,
            })
            .collect()
    }
}

/// The component-facing reads, as of the router's last pass (the previous layout).
impl Interaction for UiMemory {
    fn hovered(&self, id: WidgetId) -> bool {
        self.is_hovered(id)
    }
    fn pressed(&self, id: WidgetId) -> bool {
        self.is_pressed(id)
    }
    fn focused(&self, id: WidgetId) -> bool {
        self.focus == Focus::On(id)
    }
    /// Several pointers dragging one widget: the lowest `PointerId` answers, so the read is deterministic.
    fn drag(&self, id: WidgetId) -> Drag {
        self.pointers
            .values()
            .find_map(|p| match p.gesture {
                Gesture::Drag { source, origin, .. } | Gesture::Track { source, origin, .. }
                    if source == id =>
                {
                    Some(Drag::Held {
                        delta: p.at - p.frame_prev,
                        total: p.at - origin,
                    })
                }
                Gesture::Idle
                | Gesture::Track { .. }
                | Gesture::Press { .. }
                | Gesture::Backdrop { .. }
                | Gesture::Pan { .. }
                | Gesture::Context { .. }
                | Gesture::Select { .. }
                | Gesture::Drag { .. } => None,
            })
            .unwrap_or(Drag::Idle)
    }
    // scroll: the area as last measured (scroll.rs).
    fn scroll(&self, id: WidgetId) -> kit_core::ScrollView {
        self.scroll_view(id)
    }
}
