use crate::Time;

/// Earliest time a host must draw again without input or an application change.
/// The kit reports demand; the host owns scheduling and the event loop.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum Redraw {
    Now,
    At(Time),
    /// Nothing changes until input or an application change.
    #[default]
    Idle,
}

impl Redraw {
    pub fn earliest(self, other: Self) -> Self {
        self.min(other)
    }
}
