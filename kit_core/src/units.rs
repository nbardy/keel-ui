use crate::id::StableHasher;
use core::hash::{Hash, Hasher};
use core::time::Duration;
use kurbo::{Insets, Rect, Size};

/// Monotonic time since an app-chosen origin. Time is always an input, never read from a clock, so tests replay.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Debug, Default)]
pub struct Time(pub Duration);

impl Time {
    pub fn since(self, earlier: Time) -> Duration {
        self.0.saturating_sub(earlier.0)
    }
}

/// Opaque gesture-validity generation (screen generation, turn generation). A gesture begun under one epoch never
/// fires under another.
#[derive(Copy, Clone, Eq, PartialEq, Debug, Default)]
pub struct Epoch(pub [u64; 2]);

impl Epoch {
    /// The epoch of `scope`: the value whose change must cancel in-flight gestures (a stage, a turn, a screen).
    /// Deterministic (FNV, not `RandomState`) so it agrees across frames and runs.
    pub fn of(scope: &impl Hash) -> Self {
        let mut low = StableHasher::new();
        scope.hash(&mut low);
        // A second, domain-separated pass makes an accidental 64-bit collision of two scopes need a 128-bit one.
        let mut high = StableHasher::new();
        high.write_u8(0xA5);
        scope.hash(&mut high);
        Self([low.finish(), high.finish()])
    }
}

/// Logical-pixel viewport. `size` is in logical px (physical = logical * scale_factor).
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct Viewport {
    pub size: Size,
    pub scale_factor: f64,
    /// Notch / home-indicator insets in logical px.
    pub safe_area: Insets,
}

impl Viewport {
    pub fn new(size: Size, scale_factor: f64) -> Self {
        Self {
            size,
            scale_factor,
            safe_area: Insets::ZERO,
        }
    }
    pub fn rect(&self) -> Rect {
        self.size.to_rect()
    }
    /// The viewport rect shrunk by the safe-area insets.
    pub fn safe_rect(&self) -> Rect {
        self.rect() - self.safe_area
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn epoch_of_is_stable_and_changes_with_the_scope() {
        #[derive(Hash)]
        enum Scope {
            Encounter { started: u64 },
            Route,
        }
        let a = Epoch::of(&Scope::Encounter { started: 7 });
        assert_eq!(a, Epoch::of(&Scope::Encounter { started: 7 }));
        assert_ne!(a, Epoch::of(&Scope::Encounter { started: 8 }));
        assert_ne!(a, Epoch::of(&Scope::Route));
        // Pinned FNV-1a of eight zero bytes: a per-process random hasher (RandomState) would fail this across runs.
        assert_eq!(Epoch::of(&0u64).0[0], 0xa8c7_f832_281a_39c5);
    }
}
