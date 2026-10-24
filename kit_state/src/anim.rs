//! Value animation for `Cx::animate`: a retargetable tween stored by key, read against `Host::now`.
use crate::presence::{Easing, Progress, progress};
use core::time::Duration;
use kit_core::Time;

#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) struct Tween {
    from: f32,
    pub(crate) to: f32,
    start: Time,
    over: Duration,
}

impl Tween {
    /// Already at `to`: first sight has no entrance.
    pub(crate) fn settled(to: f32, now: Time) -> Self {
        Self {
            from: to,
            to,
            start: now,
            over: Duration::ZERO,
        }
    }
    /// Head for `to` from wherever this tween is now, so a reversal mid-flight never jumps.
    pub(crate) fn retarget(&self, to: f32, now: Time, over: Duration) -> Self {
        Self {
            from: self.at(now),
            to,
            start: now,
            over,
        }
    }
    /// Shares `presence::progress`, so a zero-length tween is `to` from its first instant (never 0/0).
    pub(crate) fn at(&self, now: Time) -> f32 {
        match progress(now.since(self.start), self.over) {
            Progress::Done => self.to,
            Progress::Running(x) => self.from + (self.to - self.from) * Easing::OutCubic.at(x),
        }
    }
}
