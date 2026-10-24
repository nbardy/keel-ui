//! Scroll physics: wheel and pan with nested hand-off, kinetic coasting, thumb drag, stick-to-end, reveal requests and
//! the bar geometry the painter draws.
//!
//! Ported from egui 0.36.2 (`containers/scroll_area.rs`, `emath::History::velocity`), MIT OR Apache-2.0,
//! Copyright (c) 2018-2021 Emil Ernerfeldt <emil.ernerfeldt@gmail.com>; see THIRD_PARTY.md. Kept: constant-friction
//! coasting (1000 px/s², stop under 20 px/s, dt capped at 0.1 s, an edge zeroes that axis), release velocity over the
//! last 0.1 s (>= 3 samples, > 10 ms), "consume until the edge, hand the rest to the parent", stick-to-end that a user
//! scroll unpins and reaching the end re-pins, Start/Center/End/Nearest reveal, floating bars (10 px sensed strip,
//! 12 px minimum handle, a press off the handle centres it). Changed: wheel lines/pages apply at once (egui smooths
//! them over 0.1 s) so routing needs no frame tick and replays exactly; a reveal glides exponentially (egui: a timed
//! ease) toward a target re-clamped every frame.
use crate::hit::{HitPath, hit_test};
use crate::{DeltaUnit, Gesture, GestureConfig, PointerEvent, PointerKind, Router, UiMemory};
use kit_core::kurbo::{Rect, Size, Vec2};
use kit_core::{Axis, ScrollView, Time, WidgetId};
use kit_view::{
    Bars, LaidKind, LaidNode, LaidOut, Reveal, RevealAlign, RevealTarget, ScrollSpec, Stick,
};
use std::collections::{BTreeMap, VecDeque};

const FRICTION: f64 = 1000.0; // px/s², egui `friction_coeff`
const STOP_SPEED: f64 = 20.0; // px/s
const MAX_DT: f64 = 0.1;
const VELOCITY_WINDOW: f64 = 0.1;
/// A glide covers 90% of the remaining distance in 0.1 s (egui's wheel smoothing factor).
const GLIDE_KEEP: f64 = 0.1;
const ARRIVED: f64 = 0.5;
/// Width of the sensed bar strip; the drawn bar is `BAR_THIN` until the pointer is on the strip.
pub const BAR_STRIP: f64 = 10.0;
pub const BAR_THIN: f64 = 4.0;
const HANDLE_MIN: f64 = 12.0;
/// Bars stay visible this long after the last movement, then fade over `BAR_FADE`.
const BAR_LINGER: f64 = 0.8;
const BAR_FADE: f64 = 0.3;

/// What a pan holds: the content (finger or mouse drag) or one bar's thumb.
#[derive(Copy, Clone, PartialEq, Debug)]
pub enum Grip {
    Content,
    /// `grab`: pointer distance from the thumb's start along the bar when it was pressed.
    Thumb {
        bar: Bar,
        grab: f64,
    },
}
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum Bar {
    Vertical,
    Horizontal,
}
impl Bar {
    fn along(self, v: Vec2) -> f64 {
        match self {
            Bar::Vertical => v.y,
            Bar::Horizontal => v.x,
        }
    }
    fn with(self, v: Vec2, value: f64) -> Vec2 {
        match self {
            Bar::Vertical => Vec2::new(v.x, value),
            Bar::Horizontal => Vec2::new(value, v.y),
        }
    }
}

#[derive(Copy, Clone, PartialEq, Debug)]
enum Motion {
    Rest,
    Coast { velocity: Vec2 },
    Glide { target: Vec2 },
}
#[derive(Copy, Clone, PartialEq, Debug)]
enum Revealed {
    Nothing,
    Ticket(u64),
}
/// When the offset last moved; a new area has not, so its bars do not flash on first sight.
#[derive(Copy, Clone, PartialEq, Debug)]
enum Moved {
    Never,
    At(Time),
}
#[derive(Copy, Clone, PartialEq, Debug)]
enum Clock {
    Unstarted,
    At(Time),
}

#[derive(Clone, PartialEq, Debug)]
struct State {
    offset: Vec2,
    motion: Motion,
    /// As last measured: the max offset decides "was at the end" before new content moves it.
    viewport: Size,
    content: Size,
    max: Vec2,
    revealed: Revealed,
    moved: Moved,
    /// Offsets during a pan, newest last, for the release velocity.
    trail: VecDeque<(Time, Vec2)>,
}

/// Every scroll area's offset and motion, keyed by the area's id.
#[derive(Clone, PartialEq, Debug)]
pub struct Scrolls {
    areas: BTreeMap<WidgetId, State>,
    clock: Clock,
}
impl Default for Scrolls {
    fn default() -> Self {
        Self {
            areas: BTreeMap::new(),
            clock: Clock::Unstarted,
        }
    }
}
impl Scrolls {
    pub(crate) fn offset(&self, id: WidgetId) -> Vec2 {
        self.areas.get(&id).map_or(Vec2::ZERO, |s| s.offset)
    }
}

/// A laid-out scroll node seen by the physics. `rect` is in the coordinate space its parent's content is hit in.
#[derive(Copy, Clone, Debug)]
pub(crate) struct Area {
    pub id: WidgetId,
    pub spec: ScrollSpec,
    pub rect: Rect,
    pub content: Rect,
}
impl Area {
    pub(crate) fn of<Msg>(node: &LaidNode<Msg>) -> Option<Area> {
        match &node.kind {
            LaidKind::Scroll { id, spec, child } => Some(Area {
                id: *id,
                spec: *spec,
                rect: node.rect,
                content: child.rect,
            }),
            LaidKind::Stack { .. }
            | LaidKind::Panel { .. }
            | LaidKind::Text { .. }
            | LaidKind::Shape { .. }
            | LaidKind::Interactive { .. }
            | LaidKind::TextField { .. }
            | LaidKind::Layer { .. }
            | LaidKind::World { .. }
            | LaidKind::Portal { .. } => None,
        }
    }
    fn max(&self) -> Vec2 {
        let over = Vec2::new(
            (self.content.width() - self.rect.width()).max(0.0),
            (self.content.height() - self.rect.height()).max(0.0),
        );
        match self.spec.axis {
            Axis::Vertical => Vec2::new(0.0, over.y),
            Axis::Horizontal => Vec2::new(over.x, 0.0),
            Axis::Both => over,
        }
    }
    /// The part of `delta` on axes this area can move along at all.
    fn masked(&self, delta: Vec2) -> Vec2 {
        let max = self.max();
        Vec2::new(
            if max.x > 0.0 { delta.x } else { 0.0 },
            if max.y > 0.0 { delta.y } else { 0.0 },
        )
    }
    fn clamp(&self, offset: Vec2) -> Vec2 {
        let max = self.max();
        Vec2::new(offset.x.clamp(0.0, max.x), offset.y.clamp(0.0, max.y))
    }
    fn bars(&self) -> Vec<Bar> {
        let max = self.max();
        match self.spec.bars {
            Bars::Hidden => Vec::new(),
            Bars::Auto => [(Bar::Vertical, max.y), (Bar::Horizontal, max.x)]
                .into_iter()
                .filter(|(_, m)| *m > 0.0)
                .map(|(bar, _)| bar)
                .collect(),
        }
    }
}

/// Track and thumb of one bar for an area drawn at `rect` (screen space).
fn bar_geometry(bar: Bar, rect: Rect, content: Size, offset: Vec2, max: Vec2) -> (Rect, Rect) {
    let (start, len, view, total) = match bar {
        Bar::Vertical => (rect.y0, rect.height(), rect.height(), content.height),
        Bar::Horizontal => (rect.x0, rect.width(), rect.width(), content.width),
    };
    let thumb_len = (len * view / total).max(HANDLE_MIN).min(len);
    let travel = len - thumb_len;
    let at = start + travel * (bar.along(offset) / bar.along(max)).clamp(0.0, 1.0);
    match bar {
        Bar::Vertical => (
            Rect::new(rect.x1 - BAR_STRIP, rect.y0, rect.x1, rect.y1),
            Rect::new(rect.x1 - BAR_STRIP, at, rect.x1, at + thumb_len),
        ),
        Bar::Horizontal => (
            Rect::new(rect.x0, rect.y1 - BAR_STRIP, rect.x1, rect.y1),
            Rect::new(at, rect.y1 - BAR_STRIP, at + thumb_len, rect.y1),
        ),
    }
}

/// One drawable bar: `strip` is the sensed track, `thumb` the handle at full strip width (the painter thins it).
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct BarView {
    pub bar: Bar,
    pub strip: Rect,
    pub thumb: Rect,
    /// 0..=1: shown while the area is hovered or moving, then faded.
    pub opacity: f32,
    /// The pointer is on the strip or holds the thumb: draw it wide.
    pub hot: bool,
}

impl UiMemory {
    /// The area as last measured, for views that build from it (`kit_state::Interaction::scroll`).
    pub fn scroll_view(&self, id: WidgetId) -> ScrollView {
        match self.scroll.areas.get(&id) {
            Some(s) => ScrollView::Seen {
                offset: s.offset,
                viewport: s.viewport,
                content: s.content,
            },
            None => ScrollView::Unseen,
        }
    }
    /// Drop the state of areas whose keys ended (`kit_state::Store::take_released`).
    pub fn forget_scrolls(&mut self, ids: &[WidgetId]) {
        for id in ids {
            self.scroll.areas.remove(id);
        }
    }
    /// Bars of the area `id` drawn at `rect` (screen space) over content of `content` size. Empty when the content
    /// fits, the spec hides bars, or the router has not measured the area.
    pub fn scroll_bars(
        &self,
        id: WidgetId,
        spec: &ScrollSpec,
        rect: Rect,
        content: Size,
        now: Time,
    ) -> Vec<BarView> {
        let Some(state) = self.scroll.areas.get(&id) else {
            return Vec::new();
        };
        let area = Area {
            id,
            spec: *spec,
            rect,
            content: Rect::from_origin_size(rect.origin(), content),
        };
        let held = |bar: Bar| {
            self.pointers.values().any(|p| match p.gesture {
                Gesture::Pan {
                    scroll,
                    grip: Grip::Thumb { bar: b, .. },
                    ..
                } => scroll == id && b == bar,
                Gesture::Pan { .. }
                | Gesture::Idle
                | Gesture::Press { .. }
                | Gesture::Backdrop { .. }
                | Gesture::Drag { .. }
                | Gesture::Track { .. }
                | Gesture::Select { .. } => false,
            })
        };
        let over = |r: Rect| {
            self.pointers.values().any(|p| match p.kind {
                PointerKind::Mouse | PointerKind::Pen => r.contains(p.at),
                PointerKind::Touch => false,
            })
        };
        let moving = match state.moved {
            Moved::Never => 0.0,
            Moved::At(t) => {
                let since = now.since(t).as_secs_f64();
                (1.0 - (since - BAR_LINGER).max(0.0) / BAR_FADE).clamp(0.0, 1.0)
            }
        };
        area.bars()
            .into_iter()
            .map(|bar| {
                let (strip, thumb) = bar_geometry(bar, rect, content, state.offset, area.max());
                let hot = held(bar) || over(strip);
                let shown = hot || over(rect);
                BarView {
                    bar,
                    strip,
                    thumb,
                    opacity: if shown { 1.0 } else { moving as f32 },
                    hot,
                }
            })
            .collect()
    }

    fn area(&mut self, area: &Area) -> &mut State {
        self.scroll.areas.entry(area.id).or_insert_with(|| {
            let max = area.max();
            State {
                // A chat opens at its end: stuck from the first frame, like egui's `scroll_stuck_to_end` default.
                offset: match area.spec.stick {
                    Stick::Free => Vec2::ZERO,
                    Stick::End => max,
                },
                motion: Motion::Rest,
                viewport: area.rect.size(),
                content: area.content.size(),
                max,
                revealed: Revealed::Nothing,
                moved: Moved::Never,
                trail: VecDeque::new(),
            }
        })
    }

    /// Move `area` by user input; returns the part of `delta` it could not take.
    fn push(&mut self, area: &Area, delta: Vec2, now: Time) -> Vec2 {
        let state = self.area(area);
        let next = area.clamp(state.offset + area.masked(delta));
        let applied = next - state.offset;
        state.offset = next;
        state.motion = Motion::Rest;
        if applied != Vec2::ZERO {
            state.moved = Moved::At(now);
        }
        delta - applied
    }
}

/// Wheel or trackpad: the innermost area under the pointer takes what it can, the rest goes outward. Taken when any
/// area under the pointer scrolls along the delta's axis, even at its edge, so the world behind does not zoom.
pub(crate) fn wheel<Msg>(
    config: &GestureConfig,
    laid: &LaidOut<Msg>,
    memory: &mut UiMemory,
    at: kit_core::kurbo::Point,
    delta: Vec2,
    unit: DeltaUnit,
    now: Time,
) -> bool {
    let path = hit_test(laid, memory, at);
    let Some(inner) = path.scrolls.first() else {
        return false;
    };
    let mut rest = match unit {
        DeltaUnit::Pixels => delta,
        DeltaUnit::Lines => delta * config.line_px,
        DeltaUnit::Pages => Vec2::new(delta.x * inner.rect.width(), delta.y * inner.rect.height()),
    };
    let mut taken = false;
    for area in &path.scrolls {
        taken |= area.masked(rest) != Vec2::ZERO;
        rest = memory.push(area, rest, now);
    }
    taken
}

/// The scroll areas containing `id`, innermost (`id` itself) first, each with the summed offsets of the areas
/// outside it (what moves it on screen).
fn chain<Msg>(laid: &LaidOut<Msg>, memory: &UiMemory, id: WidgetId) -> Vec<(Area, Vec2)> {
    fn walk<Msg>(node: &LaidNode<Msg>, id: WidgetId, stack: &mut Vec<Area>) -> bool {
        let area = Area::of(node);
        if let Some(a) = area {
            stack.push(a);
            if a.id == id {
                return true;
            }
        }
        if node.children().iter().any(|c| walk(c, id, stack)) {
            return true;
        }
        if area.is_some() {
            stack.pop();
        }
        false
    }
    let mut stack = Vec::new();
    walk(&laid.root, id, &mut stack);
    let mut outside = Vec2::ZERO;
    let mut out: Vec<(Area, Vec2)> = stack
        .into_iter()
        .map(|a| {
            let here = (a, outside);
            outside += memory.scroll.offset(a.id);
            here
        })
        .collect();
    out.reverse();
    out
}

/// One pointer move of a pan begun on `scroll`. Content: the finger drags the content, so moving down scrolls up,
/// and the remainder past an inner edge moves the outer areas. Thumb: the thumb follows the pointer.
pub(crate) fn pan<Msg>(
    laid: &LaidOut<Msg>,
    memory: &mut UiMemory,
    ev: PointerEvent,
    (scroll, grip, last): (WidgetId, Grip, kit_core::kurbo::Point),
    now: Time,
) {
    let areas = chain(laid, memory, scroll);
    match grip {
        Grip::Content => {
            let mut rest = last - ev.at;
            for (area, _) in &areas {
                rest = memory.push(area, rest, now);
                let state = memory.area(area);
                state.trail.push_back((now, state.offset));
                while state
                    .trail
                    .front()
                    .is_some_and(|(t, _)| now.since(*t).as_secs_f64() > VELOCITY_WINDOW)
                {
                    state.trail.pop_front();
                }
            }
        }
        Grip::Thumb { bar, grab } => {
            if let Some((area, outside)) = areas.first() {
                let rect = area.rect - *outside;
                let state = memory.area(area);
                let (strip, thumb) =
                    bar_geometry(bar, rect, area.content.size(), state.offset, area.max());
                let start = bar.along(strip.origin().to_vec2());
                let travel = bar.along(strip.size().to_vec2()) - bar.along(thumb.size().to_vec2());
                let at = bar.along(ev.at.to_vec2()) - grab;
                let fraction = ((at - start) / travel).clamp(0.0, 1.0);
                let target = bar.with(state.offset, fraction * bar.along(area.max()));
                let delta = target - state.offset;
                memory.push(area, delta, now);
            }
        }
    }
}

/// A pan let go: a content pan coasts each area with its own release velocity; a thumb stops where it is.
pub(crate) fn release<Msg>(
    laid: &LaidOut<Msg>,
    memory: &mut UiMemory,
    (scroll, grip): (WidgetId, Grip),
    now: Time,
) {
    match grip {
        Grip::Thumb { .. } => {}
        Grip::Content => {
            for (area, _) in chain(laid, memory, scroll) {
                let state = memory.area(&area);
                let offset = state.offset;
                state.trail.push_back((now, offset));
                state
                    .trail
                    .retain(|(t, _)| now.since(*t).as_secs_f64() <= VELOCITY_WINDOW);
                let velocity = match (state.trail.front(), state.trail.back()) {
                    (Some(&(t0, a)), Some(&(t1, b)))
                        if state.trail.len() >= 3 && t1.since(t0).as_secs_f64() > 0.01 =>
                    {
                        (b - a) / t1.since(t0).as_secs_f64()
                    }
                    _ => Vec2::ZERO,
                };
                state.trail.clear();
                state.motion = Motion::Coast { velocity };
            }
        }
    }
}

/// A press on a bar strip of an area under the pointer grabs that bar's thumb (innermost area first).
pub(crate) fn grab<Msg>(
    memory: &UiMemory,
    path: &HitPath<'_, Msg>,
    at: kit_core::kurbo::Point,
) -> Option<(WidgetId, Grip)> {
    let mut outside: Vec2 = path
        .scrolls
        .iter()
        .map(|a| memory.scroll.offset(a.id))
        .fold(Vec2::ZERO, |sum, o| sum + o);
    path.scrolls.iter().find_map(|area| {
        outside -= memory.scroll.offset(area.id);
        let local = at + outside;
        let offset = memory.scroll.offset(area.id);
        area.bars().into_iter().find_map(|bar| {
            let (strip, thumb) =
                bar_geometry(bar, area.rect, area.content.size(), offset, area.max());
            let on_thumb = thumb.contains(local);
            let thumb_len = bar.along(thumb.size().to_vec2());
            let grab = match on_thumb {
                true => bar.along(local.to_vec2()) - bar.along(thumb.origin().to_vec2()),
                false => thumb_len / 2.0,
            };
            strip
                .contains(local)
                .then_some((area.id, Grip::Thumb { bar, grab }))
        })
    })
}

impl Router {
    /// Once per frame, after layout and before painting: measure every area, apply new reveal requests, advance
    /// coasting and glides, keep stuck areas at their end and clamp offsets to content that shrank. Without it areas
    /// still scroll by wheel and drag, but nothing coasts, sticks or reveals. It also applies new focus claims
    /// (`FocusClaim`, keys:), which the fields hear about as `Edit::Focus`/`Blur` on the next routed event.
    pub fn frame<Msg>(&self, laid: &LaidOut<Msg>, memory: &mut UiMemory, now: Time) {
        crate::focus::claim(laid, memory, now);
        let dt = match memory.scroll.clock {
            Clock::Unstarted => 0.0,
            Clock::At(last) => now.since(last).as_secs_f64().min(MAX_DT),
        };
        memory.scroll.clock = Clock::At(now);
        let mut areas = Vec::new();
        collect(&laid.root, &mut areas);
        for area in areas {
            let target = reveal_target(laid, &area, memory.scroll.offset(area.id));
            let state = memory.area(&area);
            // Measured against last frame's max, before the new content moves it.
            let (end_x, end_y) = (
                state.max.x - state.offset.x <= ARRIVED,
                state.max.y - state.offset.y <= ARRIVED,
            );
            let max = area.max();
            (state.viewport, state.content, state.max) =
                (area.rect.size(), area.content.size(), max);
            if let Some((ticket, offset)) = target
                && state.revealed != Revealed::Ticket(ticket)
            {
                state.revealed = Revealed::Ticket(ticket);
                state.motion = Motion::Glide { target: offset };
            }
            let before = state.offset;
            state.motion = advance(state, &area, dt);
            match (area.spec.stick, state.motion) {
                (Stick::End, Motion::Rest | Motion::Coast { .. }) => {
                    state.offset = Vec2::new(
                        if end_x { max.x } else { state.offset.x },
                        if end_y { max.y } else { state.offset.y },
                    );
                }
                (Stick::End, Motion::Glide { .. }) | (Stick::Free, _) => {}
            }
            state.offset = area.clamp(state.offset);
            if state.offset != before {
                state.moved = Moved::At(now);
            }
        }
    }
}

fn collect<Msg>(node: &LaidNode<Msg>, out: &mut Vec<Area>) {
    out.extend(Area::of(node));
    for child in node.children() {
        collect(child, out);
    }
}

/// One frame of motion; returns the motion that continues.
fn advance(state: &mut State, area: &Area, dt: f64) -> Motion {
    match state.motion {
        Motion::Rest => Motion::Rest,
        Motion::Coast { velocity } => {
            let step = |v: f64| {
                let friction = FRICTION * dt;
                match friction > v.abs() || v.abs() < STOP_SPEED {
                    true => 0.0,
                    false => v - friction * v.signum(),
                }
            };
            let v = Vec2::new(step(velocity.x), step(velocity.y));
            let free = state.offset + v * dt;
            let held = area.clamp(free);
            state.offset = held;
            // An edge stops that axis (egui zeroes `vel` when the offset was clamped).
            let v = Vec2::new(
                if held.x == free.x { v.x } else { 0.0 },
                if held.y == free.y { v.y } else { 0.0 },
            );
            match v == Vec2::ZERO {
                true => Motion::Rest,
                false => Motion::Coast { velocity: v },
            }
        }
        Motion::Glide { target } => {
            let target = area.clamp(target);
            let keep = GLIDE_KEEP.powf(dt / 0.1);
            state.offset = target + (state.offset - target) * keep;
            match (target - state.offset).hypot() <= ARRIVED {
                true => {
                    state.offset = target;
                    Motion::Rest
                }
                false => Motion::Glide { target },
            }
        }
    }
}

/// The offset the area's reveal request asks for, with its ticket; `None` while there is no request or the target
/// is not laid out yet (the request stays pending).
fn reveal_target<Msg>(laid: &LaidOut<Msg>, area: &Area, current: Vec2) -> Option<(u64, Vec2)> {
    let Reveal::Request {
        target,
        align,
        ticket,
    } = area.spec.reveal
    else {
        return None;
    };
    // Content coordinates: 0 shows the content's first pixel at the viewport's start.
    let span = match target {
        RevealTarget::Child(id) => laid.find(id)?.rect - area.rect.origin().to_vec2(),
        RevealTarget::Content(r) => r + (area.content.origin() - area.rect.origin()),
    };
    let pick = |s: f64, e: f64, view: f64, o: f64| match align {
        RevealAlign::Start => s,
        RevealAlign::Center => (s + e) / 2.0 - view / 2.0,
        RevealAlign::End => e - view,
        RevealAlign::Nearest if s < o => s,
        RevealAlign::Nearest if e > o + view => (e - view).min(s),
        RevealAlign::Nearest => o,
    };
    let want = Vec2::new(
        pick(span.x0, span.x1, area.rect.width(), current.x),
        pick(span.y0, span.y1, area.rect.height(), current.y),
    );
    // Axes the area cannot move along keep their offset (zero).
    let max = area.max();
    let want = Vec2::new(
        if max.x > 0.0 { want.x } else { current.x },
        if max.y > 0.0 { want.y } else { current.y },
    );
    Some((ticket, area.clamp(want)))
}
