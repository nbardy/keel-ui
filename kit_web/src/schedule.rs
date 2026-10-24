//! When the next frame runs. The kit reports demand as a [`Redraw`]; this turns it into what a browser host does about
//! it, as a value, so the rule "an idle page asks for nothing" is tested natively.
use kit_core::{Redraw, Time};
use std::time::Duration;

/// What the host does after a frame.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum Wake {
    /// Ask the browser for an animation frame now.
    Frame,
    /// Nothing until this long from now (`setTimeout`), then a frame. Input arriving sooner draws sooner.
    After(Duration),
    /// Nothing changes until input or an application change: no animation frame is requested.
    Sleep,
}

/// The wake for a frame's `redraw`, at app time `now`. `At` is in the app's clock: the sum of the `dt`s it was given.
pub fn wake(redraw: Redraw, now: Time) -> Wake {
    match redraw {
        Redraw::Now => Wake::Frame,
        Redraw::At(at) => match at.0.checked_sub(now.0) {
            Some(wait) if !wait.is_zero() => Wake::After(wait),
            Some(_) | None => Wake::Frame,
        },
        Redraw::Idle => Wake::Sleep,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(ms: u64) -> Time {
        Time(Duration::from_millis(ms))
    }

    /// The point of the crate's loop: `Idle` requests no frame, a future deadline waits exactly that long, and a deadline
    /// already reached draws at once (a missed `At` would freeze a blinking caret or a timer until the next input).
    #[test]
    fn idle_sleeps_a_future_deadline_waits_and_a_reached_one_draws() {
        assert_eq!(wake(Redraw::Idle, at(10)), Wake::Sleep);
        assert_eq!(wake(Redraw::Now, at(10)), Wake::Frame);
        assert_eq!(
            wake(Redraw::At(at(510)), at(10)),
            Wake::After(Duration::from_millis(500))
        );
        assert_eq!(wake(Redraw::At(at(10)), at(10)), Wake::Frame);
        assert_eq!(wake(Redraw::At(at(5)), at(10)), Wake::Frame);
    }
}
