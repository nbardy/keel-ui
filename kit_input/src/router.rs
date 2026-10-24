use crate::field::{Platform, Selecting, TextEvent};
use crate::focus::{self, Focused};
use crate::hit::{HitPath, Region, bindings_of, hit_test};
use crate::layers::{self, Press};
use crate::{
    AnimKey, DeltaUnit, Focus, Gesture, Hover, InputEvent, Key, KeyState, Modifiers, NamedKey,
    Phase, PointerButton, PointerEvent, PointerId, PointerState, UiMemory,
};
use kit_core::kurbo::{Point, Vec2};
use kit_core::{Time, WidgetId};
use kit_view::LaidOut;
use std::time::Duration;

#[derive(Copy, Clone, PartialEq, Debug)]
pub struct GestureConfig {
    /// Movement (logical px) before a press becomes a drag or pan; `keel_ui` used 6.
    pub slop: f64,
    /// Logical px per wheel "line".
    pub line_px: f64,
    /// Longest gap between presses on a text field that still makes a double (then triple) click.
    pub multi_click: Duration,
    /// Which modifier does what in text editing.
    pub platform: Platform,
}
impl Default for GestureConfig {
    fn default() -> Self {
        Self {
            slop: 6.0,
            line_px: 40.0,
            multi_click: Duration::from_millis(500),
            platform: Platform::native(),
        }
    }
}

/// What the UI did with one input event. The host asks nothing else: it never re-walks the tree to learn whether a
/// press landed on a control, because the router that decided already said.
#[derive(Clone, PartialEq, Debug)]
pub enum Routing<Msg> {
    /// The UI reacted and produced these messages (never empty; an event that produced none is `Captured`).
    /// A hover message does not make a move exclusive, so a host that binds hover messages must not treat a move as
    /// taken because of one.
    Handled(Vec<Msg>),
    /// The UI took the event but has nothing to send yet: a press began on a control, a held press moved or ended
    /// without activating, a focused field swallowed a key. The world behind the UI must not see it.
    Captured,
    /// Nothing in the UI wants this event; the world behind it may have it.
    Passed,
}

impl<Msg> Routing<Msg> {
    fn of(claim: Claim, messages: Vec<Msg>) -> Self {
        match (claim, messages.is_empty()) {
            (_, false) => Routing::Handled(messages),
            (Claim::Taken, true) => Routing::Captured,
            (Claim::Free, true) => Routing::Passed,
        }
    }
    /// The messages, whichever way the event was routed (tests; a host that needs the claim matches instead).
    pub fn messages(self) -> Vec<Msg> {
        match self {
            Routing::Handled(messages) => messages,
            Routing::Captured | Routing::Passed => Vec::new(),
        }
    }
    /// Whether the UI took the event (the world must not also act on it).
    pub fn is_taken(&self) -> bool {
        match self {
            Routing::Handled(_) | Routing::Captured => true,
            Routing::Passed => false,
        }
    }
}

/// One `InputEvent::Key`, carried whole.
#[derive(Copy, Clone)]
struct KeyEvent {
    key: Key,
    state: KeyState,
    mods: Modifiers,
}

/// Whether a handler took its event, before messages are known.
#[derive(Copy, Clone, Eq, PartialEq)]
pub(crate) enum Claim {
    Taken,
    Free,
}
impl Claim {
    fn when(taken: bool) -> Self {
        if taken { Claim::Taken } else { Claim::Free }
    }
}

/// Turns `InputEvent`s into app messages. Stateless apart from config: gesture/focus/scroll state is in `UiMemory`.
#[derive(Copy, Clone, Default, Debug)]
pub struct Router {
    pub config: GestureConfig,
}

impl Router {
    /// Route against the tree laid out for this frame. Keep that same tree for the
    /// whole input batch; dispatch this event's messages in order before routing
    /// the next event. State changes belong to the next frame's view. A keyed
    /// intent still checks its key's current life at dispatch (Leaving/Over refuse).
    pub fn route<Msg: Clone>(
        &self,
        laid: &LaidOut<Msg>,
        memory: &mut UiMemory,
        event: InputEvent,
        now: Time,
    ) -> Routing<Msg> {
        let mut out = Vec::new();
        self.prune_stale(laid, memory, &mut out);
        focus::restore(laid, memory, now);
        focus::prune(laid, memory);
        // keys: fields hear focus moves made outside routing (claims, host requests) before this event's edits.
        focus::tell(laid, memory, &mut out);
        // [overlay] A modal takes every input event the world would otherwise see.
        let modal_takes = layers::modal_open(laid) && layers::reaches_world(&event);
        let claim = match event {
            InputEvent::Pointer(p) => self.pointer(laid, memory, p, now, &mut out),
            InputEvent::Wheel { at, delta, unit } => self.wheel(laid, memory, at, delta, unit, now),
            InputEvent::Key { key, state, mods } => {
                self.key(laid, memory, KeyEvent { key, state, mods }, now, &mut out)
            }
            InputEvent::Modifiers(mods) => {
                memory.mods = mods;
                Claim::Free
            }
            InputEvent::Text(text) => {
                self.text_event(laid, memory, TextEvent::Typed(text), now, &mut out)
            }
            InputEvent::Ime(ime) => {
                self.text_event(laid, memory, TextEvent::Ime(ime), now, &mut out)
            }
            InputEvent::Paste { ticket, text } => self.text_event(
                laid,
                memory,
                TextEvent::Pasted { ticket, text },
                now,
                &mut out,
            ),
            InputEvent::Focus(focused) => {
                memory.window_focused = focused;
                if !focused {
                    cancel_all(laid, memory, &mut out);
                }
                Claim::Free
            }
            InputEvent::Resize(viewport) => {
                memory.viewport = viewport;
                cancel_all(laid, memory, &mut out);
                Claim::Free
            }
            InputEvent::ScaleFactor(scale) => {
                memory.viewport.scale_factor = scale;
                Claim::Free
            }
            InputEvent::SafeArea(insets) => {
                memory.viewport.safe_area = insets;
                Claim::Free
            }
        };
        let claim = if modal_takes { Claim::Taken } else { claim };
        // keys: and the focus moves this event made (a press elsewhere, Tab, Escape). A press that blurs a field is
        // therefore Handled (the Blur is a message) even when it landed on nothing.
        focus::tell(laid, memory, &mut out);
        Routing::of(claim, out)
    }

    /// Gestures begun against a replaced screen/turn never fire (the `keel_ui` `set_epoch` semantics); a tracking node
    /// still in the new tree hears `Cancel` (track:).
    fn prune_stale<Msg>(&self, laid: &LaidOut<Msg>, memory: &mut UiMemory, out: &mut Vec<Msg>) {
        let stale: Vec<PointerId> = memory
            .pointers
            .iter()
            .filter(|(_, s)| s.gesture.epoch().is_some_and(|e| e != laid.epoch))
            .map(|(id, _)| *id)
            .collect();
        for id in stale {
            crate::track::abandon(laid, memory, id, out);
        }
    }

    fn pointer<Msg: Clone>(
        &self,
        laid: &LaidOut<Msg>,
        memory: &mut UiMemory,
        ev: PointerEvent,
        now: Time,
        out: &mut Vec<Msg>,
    ) -> Claim {
        let state = memory.pointers.entry(ev.id).or_insert(PointerState {
            kind: ev.kind,
            at: ev.at,
            frame_prev: ev.at,
            gesture: Gesture::Idle,
            hover: Hover::Nothing,
        });
        state.kind = ev.kind;
        state.at = ev.at;
        // A pointer that already holds a press, drag or pan owns every event until it lets go.
        let held = !matches!(state.gesture, Gesture::Idle);
        match ev.phase {
            Phase::Down => {
                // [overlay] Click-outside dismissal comes first; a press it consumed never reaches the layers below.
                if let Press::Consumed = layers::press_outside(laid, ev.at, out) {
                    end_gesture(memory, ev.id);
                    return Claim::Taken;
                }
                match ev.button {
                    PointerButton::Primary => {
                        self.down(laid, memory, ev, now);
                        // keys: a field the press focused hears Focus before the press's caret placement.
                        focus::tell(laid, memory, out);
                        // track: a press that began a track reports itself (nothing otherwise).
                        crate::track::report(
                            laid,
                            memory,
                            ev.id,
                            kit_view::PointerPhase::Press,
                            out,
                        );
                        self.field_pressed(laid, memory, ev, now, out);
                        let gesture_begun =
                            !matches!(memory.pointers[&ev.id].gesture, Gesture::Idle);
                        Claim::when(gesture_begun || matches!(memory.focus, Focus::On(_)))
                    }
                    PointerButton::Secondary => self.context_down(laid, memory, ev, now),
                    // The middle button has no gesture in the UI (the world may use it); it still dismissed
                    // popups above, like any press outside.
                    PointerButton::Middle => Claim::Free,
                }
            }
            Phase::Move => {
                self.moved(laid, memory, ev, now, out);
                Claim::when(held)
            }
            Phase::Up => match ev.button {
                PointerButton::Primary => {
                    self.up(laid, memory, ev, now, out);
                    Claim::when(held)
                }
                PointerButton::Secondary => self.context_up(laid, memory, ev, out),
                PointerButton::Middle => Claim::Free,
            },
            Phase::Cancel => {
                crate::track::abandon(laid, memory, ev.id, out);
                Claim::when(held)
            }
            Phase::Leave => {
                end_hover(laid, memory, ev.id, out);
                Claim::Free
            }
        }
    }

    fn down<Msg>(&self, laid: &LaidOut<Msg>, memory: &mut UiMemory, ev: PointerEvent, now: Time) {
        end_gesture(memory, ev.id);
        let path = hit_test(laid, memory, ev.at);
        // scroll: a press on a scroll bar grabs its thumb (physics in scroll.rs).
        if let Some((scroll, grip)) = crate::scroll::grab(memory, &path, ev.at) {
            let pan = (scroll, grip, ev.at);
            let epoch = laid.epoch;
            let gesture = Gesture::Pan {
                scroll,
                grip,
                last: ev.at,
                epoch,
            };
            set_gesture(memory, ev.id, gesture);
            crate::scroll::pan(laid, memory, ev, pan, now);
            return;
        }
        memory.focus = match path.regions.first() {
            Some(Region::Field { id }) => {
                if memory.focus != Focus::On(*id) {
                    memory.anim.insert((*id, AnimKey::Focus), now);
                }
                Focus::On(*id)
            }
            Some(Region::Bound { .. }) | None => Focus::Unfocused,
        };
        // track: a node that binds pointer positions owns the press at once (no slop).
        if let Some((source, track)) = crate::track::begin(&path, ev.at, laid.epoch) {
            memory.anim.insert((source, AnimKey::Press), now);
            set_gesture(memory, ev.id, track);
            return;
        }
        let scroll = path.scrolls.first().map(|s| s.id);
        let capturing = first_capturing(&path);
        let epoch = laid.epoch;
        let gesture = match (capturing, scroll) {
            (Some(source), scroll) => {
                memory.anim.insert((source, AnimKey::Press), now);
                Gesture::Press {
                    source,
                    scroll,
                    origin: ev.at,
                    epoch,
                }
            }
            (None, Some(scroll)) => Gesture::Backdrop {
                scroll,
                origin: ev.at,
                epoch,
            },
            (None, None) => Gesture::Idle,
        };
        if let Some(state) = memory.pointers.get_mut(&ev.id) {
            state.gesture = gesture;
        }
    }

    /// Secondary press: begins a context gesture on the innermost control that binds one, and focuses it so the menu
    /// it opens has an opener to give focus back to. A pointer already holding a gesture ignores it (one gesture per
    /// pointer). Nothing else happens: no press animation, no drag, and `activate` is never involved.
    fn context_down<Msg>(
        &self,
        laid: &LaidOut<Msg>,
        memory: &mut UiMemory,
        ev: PointerEvent,
        now: Time,
    ) -> Claim {
        if !matches!(memory.pointers[&ev.id].gesture, Gesture::Idle) {
            return Claim::Taken;
        }
        let path = hit_test(laid, memory, ev.at);
        match first_context(&path) {
            Some(source) => {
                focus::focus_on(memory, source, now);
                set_gesture(
                    memory,
                    ev.id,
                    Gesture::Context {
                        source,
                        epoch: laid.epoch,
                    },
                );
                Claim::Taken
            }
            None => Claim::Free,
        }
    }

    /// Secondary release: fires the control's context message when it lets go over the control it pressed.
    fn context_up<Msg>(
        &self,
        laid: &LaidOut<Msg>,
        memory: &mut UiMemory,
        ev: PointerEvent,
        out: &mut Vec<Msg>,
    ) -> Claim {
        match memory.pointers[&ev.id].gesture {
            Gesture::Context { source, .. } => {
                end_gesture(memory, ev.id);
                let path = hit_test(laid, memory, ev.at);
                if first_context(&path) == Some(source)
                    && let Some(spec) = laid
                        .find(source)
                        .and_then(bindings_of)
                        .and_then(|on| on.context.as_ref())
                {
                    out.push((spec.on)(ev.at));
                }
                Claim::Taken
            }
            Gesture::Idle
            | Gesture::Press { .. }
            | Gesture::Backdrop { .. }
            | Gesture::Drag { .. }
            | Gesture::Pan { .. }
            | Gesture::Track { .. }
            | Gesture::Select { .. } => Claim::Free,
        }
    }

    fn moved<Msg: Clone>(
        &self,
        laid: &LaidOut<Msg>,
        memory: &mut UiMemory,
        ev: PointerEvent,
        now: Time,
        out: &mut Vec<Msg>,
    ) {
        let gesture = memory.pointers[&ev.id].gesture;
        let slop2 = self.config.slop * self.config.slop;
        let beyond = |origin: Point| (ev.at - origin).hypot2() >= slop2;
        match gesture {
            Gesture::Idle => update_hover(laid, memory, ev.id, now, out),
            // Capture holds: other regions neither hover nor fire while the pointer is captured.
            Gesture::Drag { .. } => {}
            // track: every move is reported to the tracking node, inside it or not.
            Gesture::Track { .. } => {
                crate::track::moved(memory, ev.id, self.config.slop);
                crate::track::report(laid, memory, ev.id, kit_view::PointerPhase::Drag, out);
            }
            Gesture::Press {
                source,
                scroll,
                origin,
                epoch,
            } => {
                if !beyond(origin) {
                    return;
                }
                let drag = laid
                    .find(source)
                    .and_then(bindings_of)
                    .and_then(|on| on.drag);
                let next = match (drag, scroll) {
                    (Some(spec), _) => Gesture::Drag {
                        source,
                        payload: spec.payload,
                        origin,
                        epoch,
                    },
                    (None, Some(scroll)) => Gesture::Pan {
                        scroll,
                        grip: crate::scroll::Grip::Content,
                        last: origin,
                        epoch,
                    },
                    (None, None) => return,
                };
                set_gesture(memory, ev.id, next);
                self.pan_step(laid, memory, ev, now);
            }
            Gesture::Backdrop {
                scroll,
                origin,
                epoch,
            } => {
                if beyond(origin) {
                    set_gesture(
                        memory,
                        ev.id,
                        Gesture::Pan {
                            scroll,
                            grip: crate::scroll::Grip::Content,
                            last: origin,
                            epoch,
                        },
                    );
                    self.pan_step(laid, memory, ev, now);
                }
            }
            Gesture::Pan { .. } => self.pan_step(laid, memory, ev, now),
            // The release decides (over the same control); moving the pointer changes nothing.
            Gesture::Context { .. } => {}
            Gesture::Select {
                field, anchor, by, ..
            } => {
                let drag = Selecting { field, anchor, by };
                self.field_dragged(laid, memory, ev, drag, now, out);
            }
        }
    }

    /// scroll: one move of a content or thumb pan (physics in scroll.rs).
    fn pan_step<Msg>(
        &self,
        laid: &LaidOut<Msg>,
        memory: &mut UiMemory,
        ev: PointerEvent,
        now: Time,
    ) {
        let Gesture::Pan {
            scroll,
            grip,
            last,
            epoch,
        } = memory.pointers[&ev.id].gesture
        else {
            return;
        };
        crate::scroll::pan(laid, memory, ev, (scroll, grip, last), now);
        let next = Gesture::Pan {
            scroll,
            grip,
            last: ev.at,
            epoch,
        };
        set_gesture(memory, ev.id, next);
    }

    fn up<Msg: Clone>(
        &self,
        laid: &LaidOut<Msg>,
        memory: &mut UiMemory,
        ev: PointerEvent,
        now: Time,
        out: &mut Vec<Msg>,
    ) {
        let gesture = memory.pointers[&ev.id].gesture;
        // track: the release reaches the tracking node wherever the pointer let go.
        crate::track::report(laid, memory, ev.id, kit_view::PointerPhase::Release, out);
        end_gesture(memory, ev.id);
        let path = hit_test(laid, memory, ev.at);
        match gesture {
            Gesture::Press { source, .. } => {
                let top = first_capturing(&path);
                // keys: the activation is built from the modifiers held (shift/cmd-click) when the node asks.
                if top == Some(source)
                    && let Some(msg) = laid
                        .find(source)
                        .and_then(bindings_of)
                        .and_then(|on| on.activation(memory.mods))
                {
                    out.push(msg);
                }
            }
            Gesture::Drag {
                source, payload, ..
            } => {
                // Innermost region first; only a region that declares a matching drop binding takes the payload.
                for region in &path.regions {
                    if let Region::Bound { id, on, .. } = region
                        && *id != source
                        && let Some(drop) = &on.drop
                        && drop.accepts == payload.kind
                    {
                        out.push((drop.on_drop)(kit_view::Dropped {
                            payload,
                            source,
                            target: *id,
                        }));
                        break;
                    }
                }
            }
            // scroll: a content pan let go coasts (scroll.rs).
            Gesture::Pan { scroll, grip, .. } => {
                crate::scroll::release(laid, memory, (scroll, grip), now);
            }
            Gesture::Idle
            | Gesture::Backdrop { .. }
            | Gesture::Select { .. }
            | Gesture::Context { .. }
            | Gesture::Track { .. } => {}
        }
        update_hover(laid, memory, ev.id, now, out);
    }

    fn wheel<Msg>(
        &self,
        laid: &LaidOut<Msg>,
        memory: &mut UiMemory,
        at: Point,
        delta: Vec2,
        unit: DeltaUnit,
        now: Time,
    ) -> Claim {
        // scroll: nested hand-off and clamping live in scroll.rs.
        let taken = crate::scroll::wheel(&self.config, laid, memory, at, delta, unit, now);
        Claim::when(taken)
    }

    fn key<Msg: Clone>(
        &self,
        laid: &LaidOut<Msg>,
        memory: &mut UiMemory,
        event: KeyEvent,
        now: Time,
        out: &mut Vec<Msg>,
    ) -> Claim {
        let KeyEvent { key, state, mods } = event;
        memory.mods = mods;
        // track: Escape first lets go of a tracked pointer (the tracking node hears Cancel) and is taken.
        if state == KeyState::Down
            && key == Key::Named(NamedKey::Escape)
            && crate::track::escape(laid, memory, out)
        {
            return Claim::Taken;
        }
        match (state, focus::classify(laid, memory)) {
            (KeyState::Up, Focused::Nothing | Focused::Control(_)) => Claim::Free,
            // A focused field owns the keyboard: no shortcut may fire while the player is typing.
            (KeyState::Up, Focused::Field(_)) => Claim::Taken,
            // Tab leaves a field; everything else is the field's (its own Escape included).
            (KeyState::Down, Focused::Field(_)) => match key {
                Key::Named(NamedKey::Tab) => self.tab(laid, memory, mods.shift, now),
                Key::Named(_) | Key::Character(_) => {
                    self.field_key(laid, memory, key, mods, now, out)
                }
            },
            // keys: Tab is the router's; Enter/Space activate when the control binds an activation; then the
            // control's own key binding (`Bindings::on_key`); only a key it passes walks focus or reaches hotkeys.
            (KeyState::Down, Focused::Control(id)) => {
                let own = match key {
                    Key::Named(NamedKey::Tab) => self.tab(laid, memory, mods.shift, now),
                    Key::Named(NamedKey::Enter | NamedKey::Space) => {
                        self.activate_focused(laid, id, mods, out)
                    }
                    Key::Named(_) | Key::Character(_) => Claim::Free,
                };
                match own {
                    Claim::Taken => Claim::Taken,
                    Claim::Free => match focus::key(laid, id, key, mods, out) {
                        Claim::Taken => Claim::Taken,
                        Claim::Free => self.unbound_key(laid, memory, key, mods, now, out),
                    },
                }
            }
            (KeyState::Down, Focused::Nothing) => {
                self.global_key(laid, memory, key, mods, now, out)
            }
        }
    }

    /// A key the focused control did not take. [pilots] Once the keyboard is navigating (a control has focus),
    /// Down/Up step through controls like Tab/Shift-Tab (egui moves focus with arrows too): how a list a combo box
    /// opened is walked. With nothing focused the arrows stay the world's (camera keys).
    fn unbound_key<Msg: Clone>(
        &self,
        laid: &LaidOut<Msg>,
        memory: &mut UiMemory,
        key: Key,
        mods: Modifiers,
        now: Time,
        out: &mut Vec<Msg>,
    ) -> Claim {
        match key {
            Key::Named(NamedKey::ArrowDown) => self.tab(laid, memory, false, now),
            Key::Named(NamedKey::ArrowUp) => self.tab(laid, memory, true, now),
            Key::Named(_) | Key::Character(_) => self.global_key(laid, memory, key, mods, now, out),
        }
    }

    /// A key pressed while no text field has focus: Tab and Escape navigate, anything else may be a shortcut.
    fn global_key<Msg: Clone>(
        &self,
        laid: &LaidOut<Msg>,
        memory: &mut UiMemory,
        key: Key,
        mods: Modifiers,
        now: Time,
        out: &mut Vec<Msg>,
    ) -> Claim {
        match key {
            Key::Named(NamedKey::Tab) => self.tab(laid, memory, mods.shift, now),
            Key::Named(NamedKey::Escape) => self.escape(laid, memory, out),
            Key::Named(_) | Key::Character(_) => self.shortcut(laid, key, mods, out),
        }
    }

    /// The enabled control bound to `key`, if any, activates. Chorded keys (Ctrl/Alt/Meta) belong to the platform.
    fn shortcut<Msg: Clone>(
        &self,
        laid: &LaidOut<Msg>,
        key: Key,
        mods: Modifiers,
        out: &mut Vec<Msg>,
    ) -> Claim {
        let chorded = mods.ctrl || mods.alt || mods.meta;
        if chorded {
            return Claim::Free;
        }
        // [overlay] Only the topmost scope: a modal's shortcuts, never those of the screen behind it.
        let bound = focus::scope(laid)
            .descendants()
            .into_iter()
            .filter(|node| node.shortcut() == Some(key))
            .find_map(|node| bindings_of(node).and_then(|on| on.activate.clone()));
        match bound {
            Some(msg) => {
                out.push(msg);
                Claim::Taken
            }
            None => Claim::Free,
        }
    }
}

fn first_capturing<Msg>(path: &HitPath<'_, Msg>) -> Option<WidgetId> {
    path.regions.iter().find_map(|r| match r {
        Region::Bound { id, on, .. } if on.captures_press() => Some(*id),
        Region::Bound { .. } | Region::Field { .. } => None,
    })
}

/// The innermost control under the point that answers a context click.
fn first_context<Msg>(path: &HitPath<'_, Msg>) -> Option<WidgetId> {
    path.regions.iter().find_map(|r| match r {
        Region::Bound { id, on, .. } if on.context.is_some() => Some(*id),
        Region::Bound { .. } | Region::Field { .. } => None,
    })
}

fn set_gesture(memory: &mut UiMemory, id: PointerId, gesture: Gesture) {
    if let Some(state) = memory.pointers.get_mut(&id) {
        state.gesture = gesture;
    }
}

pub(crate) fn end_gesture(memory: &mut UiMemory, id: PointerId) {
    let Some(state) = memory.pointers.get_mut(&id) else {
        return;
    };
    let source = match state.gesture {
        Gesture::Press { source, .. }
        | Gesture::Drag { source, .. }
        | Gesture::Track { source, .. } => Some(source),
        Gesture::Idle
        | Gesture::Backdrop { .. }
        | Gesture::Pan { .. }
        | Gesture::Context { .. }
        | Gesture::Select { .. } => None,
    };
    state.gesture = Gesture::Idle;
    if let Some(source) = source {
        memory.anim.remove(&(source, AnimKey::Press));
    }
}

fn cancel_all<Msg>(laid: &LaidOut<Msg>, memory: &mut UiMemory, out: &mut Vec<Msg>) {
    let ids: Vec<PointerId> = memory.pointers.keys().copied().collect();
    for id in ids {
        crate::track::abandon(laid, memory, id, out);
    }
}

fn hover_target<Msg>(path: &HitPath<'_, Msg>) -> Hover {
    path.regions
        .iter()
        .find_map(|r| match r {
            // Any press-capable or hover-bound region is a hover target (hover highlight needs no message).
            Region::Bound { id, on, .. } if on.hover.is_some() || on.captures_press() => {
                Some(Hover::Over(*id))
            }
            Region::Bound { .. } | Region::Field { .. } => None,
        })
        .unwrap_or(Hover::Nothing)
}

fn update_hover<Msg: Clone>(
    laid: &LaidOut<Msg>,
    memory: &mut UiMemory,
    id: PointerId,
    now: Time,
    out: &mut Vec<Msg>,
) {
    let state = memory.pointers[&id];
    let next = match state.kind {
        // Touch has no hover: a finger either presses or is absent.
        crate::PointerKind::Touch => Hover::Nothing,
        crate::PointerKind::Mouse | crate::PointerKind::Pen => {
            hover_target(&hit_test(laid, memory, state.at))
        }
    };
    if next == state.hover {
        return;
    }
    emit_hover(laid, state.hover, false, out);
    emit_hover(laid, next, true, out);
    if let Hover::Over(new) = next {
        memory.anim.insert((new, AnimKey::Hover), now);
    }
    if let Hover::Over(old) = state.hover {
        memory.anim.remove(&(old, AnimKey::Hover));
    }
    if let Some(s) = memory.pointers.get_mut(&id) {
        s.hover = next;
    }
}

fn end_hover<Msg: Clone>(
    laid: &LaidOut<Msg>,
    memory: &mut UiMemory,
    id: PointerId,
    out: &mut Vec<Msg>,
) {
    let Some(state) = memory.pointers.get(&id).copied() else {
        return;
    };
    emit_hover(laid, state.hover, false, out);
    if let Hover::Over(old) = state.hover {
        memory.anim.remove(&(old, AnimKey::Hover));
    }
    if let Some(s) = memory.pointers.get_mut(&id) {
        s.hover = Hover::Nothing;
    }
}

fn emit_hover<Msg: Clone>(laid: &LaidOut<Msg>, hover: Hover, enter: bool, out: &mut Vec<Msg>) {
    match hover {
        Hover::Nothing => {}
        Hover::Over(id) => {
            if let Some(spec) = laid
                .find(id)
                .and_then(bindings_of)
                .and_then(|on| on.hover.as_ref())
            {
                out.push(if enter {
                    spec.enter.clone()
                } else {
                    spec.leave.clone()
                });
            }
        }
    }
}
