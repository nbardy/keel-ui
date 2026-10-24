//! What a reducer returns: asks for the app, counted faults, and at most one wake-up. No other effect channel exists.
use core::fmt::Debug;
use kit_core::Time;

/// A reducer's complaint, counted in the Store (never swallowed, never a panic).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fault(pub &'static str);

/// A timer as data: deliver `Ev` to this same reducer and key at `Time`. `Keep` leaves the slot's pending wake-up as
/// it is (most steps do not touch timers); `Cancel` drops it.
#[derive(Debug)]
pub enum Wake<Ev> {
    Keep,
    At(Time, Ev),
    Cancel,
}

/// `Ev` is the reducer's own event type, so a wake-up is typed: it comes back as an ordinary event.
#[derive(Debug)]
pub struct Act<Ask, Ev> {
    pub(crate) asks: Vec<Ask>,
    pub(crate) faults: Vec<Fault>,
    pub(crate) wake: Wake<Ev>,
}

impl<Ask, Ev> Act<Ask, Ev> {
    pub fn none() -> Self {
        Self {
            asks: Vec::new(),
            faults: Vec::new(),
            wake: Wake::Keep,
        }
    }
    pub fn ask(ask: Ask) -> Self {
        Self {
            asks: vec![ask],
            ..Self::none()
        }
    }
    pub fn fault(why: &'static str) -> Self {
        Self {
            faults: vec![Fault(why)],
            ..Self::none()
        }
    }
    /// Wake this reducer with `ev` at `at` (one wake-up per key: a later one replaces it). The host fires due
    /// wake-ups with [`wake_due`](crate::wake_due) and learns when the next one is from `Store::next_due`; the
    /// wake-up dies with the key's life.
    pub fn wake(at: Time, ev: Ev) -> Self {
        Self {
            wake: Wake::At(at, ev),
            ..Self::none()
        }
    }
    /// Drop this key's pending wake-up (a hold released before its repeat).
    pub fn cancel_wake() -> Self {
        Self {
            wake: Wake::Cancel,
            ..Self::none()
        }
    }
    /// Both acts, `self`'s asks first; `other`'s wake-up decision replaces `self`'s unless it is `Keep`.
    pub fn and(mut self, other: Act<Ask, Ev>) -> Self {
        self.asks.extend(other.asks);
        self.faults.extend(other.faults);
        self.wake = match other.wake {
            Wake::Keep => self.wake,
            wake @ (Wake::At(..) | Wake::Cancel) => wake,
        };
        self
    }
}
