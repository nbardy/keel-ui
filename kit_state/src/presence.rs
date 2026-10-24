//! Presence as a pure function of data: a Disclosure remembers when it changed, a Presence maps elapsed time to a Phase.
use core::time::Duration;
use kit_core::Time;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Since {
    /// Never touched: no entrance or exit is owed.
    Start,
    At(Time),
}

/// Open/closed that remembers when it changed, so an exit animation has data to run on.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Disclosure {
    Open { since: Since },
    Closed { since: Since },
}
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DisclosureEv {
    Open,
    Close,
    Toggle,
}

impl Disclosure {
    pub const CLOSED: Disclosure = Disclosure::Closed {
        since: Since::Start,
    };
    pub const OPEN: Disclosure = Disclosure::Open {
        since: Since::Start,
    };

    /// A repeated Open/Close is a no-op (keeps the original `since`); Toggle always transitions, so two presses in
    /// one frame are two toggles.
    pub fn step(&mut self, ev: DisclosureEv, now: Time) {
        let since = Since::At(now);
        *self = match (*self, ev) {
            (Disclosure::Open { .. }, DisclosureEv::Close | DisclosureEv::Toggle) => {
                Disclosure::Closed { since }
            }
            (Disclosure::Closed { .. }, DisclosureEv::Open | DisclosureEv::Toggle) => {
                Disclosure::Open { since }
            }
            (open @ Disclosure::Open { .. }, DisclosureEv::Open) => open,
            (closed @ Disclosure::Closed { .. }, DisclosureEv::Close) => closed,
        };
    }
    pub fn phase(&self, presence: Presence, now: Time) -> Phase {
        presence.phase(*self, now)
    }
}

/// Where a disclosure is in its enter/exit. The `f32` is LINEAR progress `0..1` of that transition (apply an
/// [`Easing`] to display it). `Exiting(1.0)` is the instant before `Gone`.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Phase {
    Entering(f32),
    Shown,
    Exiting(f32),
    Gone,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Presence {
    pub enter: Duration,
    pub exit: Duration,
}

/// Progress of `elapsed` through `total`. `Done` when `elapsed >= total`, which includes `total == 0` at
/// `elapsed == 0`: a zero-length transition is over from its first instant. (Regression: kit_widgets float.rs, where
/// a zero-length Tween was read as finished BEFORE it started and hid the floating number; and no 0/0 here.)
pub(crate) enum Progress {
    Done,
    Running(f32),
}
pub(crate) fn progress(elapsed: Duration, total: Duration) -> Progress {
    match elapsed >= total {
        true => Progress::Done,
        false => Progress::Running(elapsed.as_secs_f32() / total.as_secs_f32()), // total > elapsed >= 0, so total > 0
    }
}

impl Presence {
    pub fn phase(&self, disclosure: Disclosure, now: Time) -> Phase {
        match disclosure {
            Disclosure::Open {
                since: Since::Start,
            } => Phase::Shown,
            Disclosure::Closed {
                since: Since::Start,
            } => Phase::Gone,
            Disclosure::Open {
                since: Since::At(at),
            } => match progress(now.since(at), self.enter) {
                Progress::Done => Phase::Shown,
                Progress::Running(x) => Phase::Entering(x),
            },
            Disclosure::Closed {
                since: Since::At(at),
            } => match progress(now.since(at), self.exit) {
                Progress::Done => Phase::Gone,
                Progress::Running(x) => Phase::Exiting(x),
            },
        }
    }
}

/// Easing curves on `t` in `0..=1` (clamped): `at(0) == 0`, `at(1) == 1`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Easing {
    Linear,
    OutQuad,
    OutCubic,
}
impl Easing {
    pub fn at(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Easing::Linear => t,
            Easing::OutQuad => 1.0 - (1.0 - t) * (1.0 - t),
            Easing::OutCubic => 1.0 - (1.0 - t).powi(3),
        }
    }
}
