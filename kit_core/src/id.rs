use core::hash::{Hash, Hasher};

/// Stable widget identity derived from keys/paths. Same path => same id across frames, runs and platforms
/// (FNV-1a, not `std`'s randomised hasher), so `UiMemory` entries survive rebuilding the tree each frame.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub struct WidgetId(u64);

/// Where an interactive node's `WidgetId` comes from. `LaidOut::assemble` resolves it once.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum Identity {
    /// Derived from the node's position in the tree: stable while the tree's shape is stable. The default.
    Positional,
    /// Chosen by the app: use for list items that reorder or appear and disappear, where position would move.
    Keyed(WidgetId),
}

impl Identity {
    /// The id of a node whose tree path hashes to `path`.
    pub fn resolve(self, path: WidgetId) -> WidgetId {
        match self {
            Identity::Positional => path,
            Identity::Keyed(id) => id,
        }
    }
}
impl From<WidgetId> for Identity {
    fn from(id: WidgetId) -> Self {
        Identity::Keyed(id)
    }
}

/// FNV-1a as a `Hasher`: the deterministic stand-in for `RandomState` wherever a hash must agree across frames, runs
/// and platforms (widget keys, gesture epochs). Byte-order-sensitive `Hash` impls (`usize`) agree per platform width.
#[derive(Copy, Clone, Debug)]
pub struct StableHasher(u64);

impl StableHasher {
    pub const fn new() -> Self {
        Self(FNV_OFFSET)
    }
}
impl Default for StableHasher {
    fn default() -> Self {
        Self::new()
    }
}
impl Hasher for StableHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 = (self.0 ^ u64::from(*b)).wrapping_mul(FNV_PRIME);
        }
    }
}

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

fn mix(mut h: u64, tag: u8, bytes: &[u8]) -> u64 {
    h = (h ^ u64::from(tag)).wrapping_mul(FNV_PRIME);
    for b in bytes {
        h = (h ^ u64::from(*b)).wrapping_mul(FNV_PRIME);
    }
    // length terminator so ("ab","c") and ("a","bc") differ
    (h ^ bytes.len() as u64).wrapping_mul(FNV_PRIME)
}

impl WidgetId {
    /// Root id from a string key.
    pub fn key(key: &str) -> Self {
        Self(mix(FNV_OFFSET, 1, key.as_bytes()))
    }
    /// Root id from any hashable key (an enum, a tuple): `.key(&(Row::Sell, kind))`. The key's TYPE is part of the id:
    /// `Hash` writes only a variant's index and fields, so two screens' key enums with an equally shaped variant (the
    /// room's `Keyed::Vote(0)` and the single-variant `Key::Helper { Gunnery, boat 0 }`, which hashes no variant tag)
    /// gave equal ids, and the Bnav route vote page
    /// failed layout with `DuplicateIdentity` in the browser (kit flip, 2026-10-23; test `keys_of_different_types_differ`).
    pub fn of<K: Hash>(key: &K) -> Self {
        let mut hasher = StableHasher::new();
        core::any::type_name::<K>().hash(&mut hasher);
        key.hash(&mut hasher);
        Self(mix(hasher.finish(), 3, &[]))
    }
    /// Child id from a string key (a path segment).
    pub fn child(self, key: &str) -> Self {
        Self(mix(self.0, 1, key.as_bytes()))
    }
    /// Child id from an index (list items, grid cells). Distinct from any string segment.
    pub fn index(self, index: u64) -> Self {
        Self(mix(self.0, 2, &index.to_le_bytes()))
    }
    pub fn raw(self) -> u64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hashed_keys_are_deterministic_and_distinguish_values() {
        #[derive(Hash)]
        enum Row {
            Sell(u8),
            Buy(u8),
        }
        assert_eq!(WidgetId::of(&Row::Sell(1)), WidgetId::of(&Row::Sell(1)));
        assert_ne!(WidgetId::of(&Row::Sell(1)), WidgetId::of(&Row::Buy(1)));
        assert_ne!(WidgetId::of(&Row::Sell(1)), WidgetId::of(&Row::Sell(2)));
    }
    /// Regression (kit flip, 2026-10-23): two screens' key enums whose variants hash alike collided, and a page that
    /// showed both failed layout with `DuplicateIdentity`. The key's type is part of the id.
    #[test]
    fn keys_of_different_types_differ() {
        #[derive(Hash)]
        enum Room {
            _Take,
            _Switch,
            Vote(u8),
        }
        #[derive(Hash)]
        enum Dock {
            _Open,
            _Tab,
            Row(u8),
        }
        assert_ne!(WidgetId::of(&Room::Vote(0)), WidgetId::of(&Dock::Row(0)));
    }
    #[test]
    fn path_segments_do_not_collide_by_concatenation_or_kind() {
        let root = WidgetId::key("hud");
        assert_ne!(root.child("ab").child("c"), root.child("a").child("bc"));
        assert_ne!(root.child("1"), root.index(1));
        assert_eq!(
            root.child("x").index(3),
            WidgetId::key("hud").child("x").index(3)
        );
    }
}
