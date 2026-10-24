//! The type-erased state table. Storage is keyed by (declaration TypeId, key); values are `Rc<S>` so a view's handle
//! stays valid while a reducer mutates (copy-on-write via `Rc::make_mut`).
use crate::act::Fault;
use crate::anim::Tween;
use crate::ident::{AnyKey, Host, Life};
use crate::intent::ErasedStep;
use crate::intent::Late;
use core::any::{Any, TypeId};
use kit_core::WidgetId;
use kit_core::{MotionPref, Redraw, Time};
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;

pub type SlotId<A> = (TypeId, AnyKey<A>);
pub type Slots<A> = HashMap<SlotId<A>, Box<dyn Any>>;

/// How many refused intents the Store remembers. Older ones are dropped from the log but still counted.
pub const LATE_LOG: usize = 64;

/// A pending wake-up: the intent that delivers the reducer's event to its slot. One per slot.
struct Timer<A: Host> {
    slot: SlotId<A>,
    at: Time,
    /// Scheduling order: breaks ties between timers due at the same time, and names a timer across replacement.
    seq: u64,
    step: Rc<dyn ErasedStep<A>>,
}

pub struct Store<A: Host> {
    /// RefCell because rendering (`&Cx`) lazily creates the slot of a first-seen key; dispatch uses `get_mut`.
    pub(crate) slots: RefCell<Slots<A>>,
    /// Ring of the newest `LATE_LOG` refusals (bounded: a stuck UI re-sending to a dead key must not grow memory).
    pub(crate) late: VecDeque<Late>,
    pub(crate) late_total: u64,
    pub(crate) faults: Vec<Fault>,
    /// Keys whose router-side memory (scroll offsets) is keyed by `WidgetId::of(key)`; collected with the key.
    pub(crate) widgets: RefCell<HashMap<AnyKey<A>, WidgetId>>,
    released: Vec<WidgetId>,
    timers: Vec<Timer<A>>,
    scheduled: u64,
}

impl<A: Host> Default for Store<A> {
    fn default() -> Self {
        Self {
            slots: RefCell::new(HashMap::new()),
            late: VecDeque::new(),
            late_total: 0,
            faults: Vec::new(),
            widgets: RefCell::new(HashMap::new()),
            released: Vec::new(),
            timers: Vec::new(),
            scheduled: 0,
        }
    }
}

impl<A: Host> Store<A> {
    pub fn new() -> Self {
        Self::default()
    }
    /// Number of stored state instances (all declarations, all keys).
    pub fn len(&self) -> usize {
        self.slots.borrow().len()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// The newest `LATE_LOG` refused intents, oldest first. Never silent: a refusal is data.
    pub fn late(&self) -> &VecDeque<Late> {
        &self.late
    }
    /// Every refusal ever, including those the ring has dropped.
    pub fn late_total(&self) -> u64 {
        self.late_total
    }
    pub(crate) fn refuse(&mut self, late: Late) {
        self.late_total += 1;
        if self.late.len() == LATE_LOG {
            self.late.pop_front();
        }
        self.late.push_back(late);
    }
    pub fn faults(&self) -> &[Fault] {
        &self.faults
    }
    /// Ask each stored key its life; `Over` removes the state. Leaving keys stay (inert) so exits have data.
    /// `dispatch` runs this after every event; a host that mutates the app outside `dispatch` calls it too.
    pub fn collect(&mut self, app: &A) {
        let live = |key: &AnyKey<A>| match key.life(app) {
            Life::Over => false,
            Life::Live | Life::Leaving(_) => true,
        };
        self.slots.get_mut().retain(|(_, key), _| live(key));
        // A wake-up needs a Live key: a Leaving key is inert (its intents are refused) and an Over one is gone.
        self.timers.retain(|timer| match timer.slot.1.life(app) {
            Life::Live => true,
            Life::Leaving(_) | Life::Over => false,
        });
        let released = &mut self.released;
        self.widgets.get_mut().retain(|key, id| {
            let keep = live(key);
            if !keep {
                released.push(*id);
            }
            keep
        });
    }
    /// Router memory to drop: ids of `cx.scroll` keys that ended since the last call, sorted. Hand them to
    /// `UiMemory::forget_scrolls` after dispatching, or the router keeps their offsets.
    pub fn take_released(&mut self) -> Vec<WidgetId> {
        let mut ids = core::mem::take(&mut self.released);
        ids.sort();
        ids
    }
    /// Timer-only deadline, across every key. Idle means no reducer timer is pending;
    /// use redraw(app) and combine view/input/paint demand before sleeping.
    pub fn next_due(&self) -> Redraw {
        self.timers
            .iter()
            .map(|timer| timer.at)
            .min()
            .map_or(Redraw::Idle, Redraw::At)
    }
    /// Store demand: reducer timers, live tweens and keys retained for their exit.
    /// Call after building the view, so newly declared/retargeted animations are included.
    pub fn redraw(&self, app: &A) -> Redraw {
        let slots = self.slots.borrow();
        let mut demand = self.next_due();
        for ((declaration, key), value) in slots.iter() {
            if let Life::Leaving(_) = key.life(app) {
                demand = Redraw::Now;
            }
            if *declaration == TypeId::of::<Tween>()
                && app.motion() == MotionPref::Full
                && slot_value::<Tween>(&**value).running(app.now())
            {
                demand = Redraw::Now;
            }
        }
        for key in self.widgets.borrow().keys() {
            if let Life::Leaving(_) = key.life(app) {
                demand = Redraw::Now;
            }
        }
        demand
    }
    pub(crate) fn unschedule(&mut self, slot: &SlotId<A>) {
        self.timers.retain(|timer| timer.slot != *slot);
    }
    /// The slot's wake-up, replacing any it had.
    pub(crate) fn schedule(&mut self, slot: SlotId<A>, at: Time, step: Rc<dyn ErasedStep<A>>) {
        self.unschedule(&slot);
        self.scheduled += 1;
        let seq = self.scheduled;
        self.timers.push(Timer {
            slot,
            at,
            seq,
            step,
        });
    }
    /// The wake-ups due at `now`, in firing order (by time, then scheduling order).
    pub(crate) fn due(&self, now: Time) -> Vec<u64> {
        let mut due: Vec<(Time, u64)> = self
            .timers
            .iter()
            .filter(|timer| timer.at <= now)
            .map(|timer| (timer.at, timer.seq))
            .collect();
        due.sort();
        due.into_iter().map(|(_, seq)| seq).collect()
    }
    /// Remove a wake-up to fire it; `None` when it was replaced or collected since `due` named it.
    pub(crate) fn take_timer(&mut self, seq: u64) -> Option<Rc<dyn ErasedStep<A>>> {
        let index = self.timers.iter().position(|timer| timer.seq == seq)?;
        Some(self.timers.remove(index).step)
    }
    pub(crate) fn read<S: 'static>(&self, id: &SlotId<A>) -> Option<Rc<S>> {
        // `&**slot`, not `slot`: `&Box<dyn Any>` would coerce to `&dyn Any` OF THE BOX and every downcast would fail.
        self.slots
            .borrow()
            .get(id)
            .map(|slot| slot_value::<S>(&**slot).clone())
    }
    pub(crate) fn insert<S: 'static>(&self, id: SlotId<A>, value: Rc<S>) {
        self.slots.borrow_mut().insert(id, Box::new(value));
    }
}

/// The declaration TypeId fixes `S` (a closure type has exactly one signature), so a mismatch is a kit bug.
pub(crate) fn slot_value<S: 'static>(slot: &dyn Any) -> &Rc<S> {
    slot.downcast_ref::<Rc<S>>()
        .expect("slot type is fixed by its declaration TypeId")
}
pub(crate) fn slot_value_mut<S: 'static>(slot: &mut dyn Any) -> &mut Rc<S> {
    slot.downcast_mut::<Rc<S>>()
        .expect("slot type is fixed by its declaration TypeId")
}
