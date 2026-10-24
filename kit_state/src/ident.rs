//! Idents (data identity that knows when it lives), the `Host` seam to the app model, and type-erased keys.
use core::any::Any;
use core::fmt::Debug;
use core::hash::{Hash, Hasher};
use kit_core::{MotionPref, Time};
use std::rc::Rc;

/// How long a keyed thing belongs. No default: every key states its life.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Life {
    Live,
    /// Still stored (so an exit animation has data) but inert until this time; intents to it are `Late`.
    Leaving(Time),
    Over,
}

/// A data identity that knows when it lives, as a function of the app model.
pub trait Ident<App>: Hash + Eq + Clone + Debug + 'static {
    fn life(&self, app: &App) -> Life;
}

/// What kit_state needs from the app model: the clock (an input, never read from the OS) and where asks go.
pub trait Host: Sized + 'static {
    /// Commands a reducer wants the app to perform. The app decides what they mean.
    type Ask: Clone + Debug + 'static;
    fn now(&self) -> Time;
    /// The player's motion preference; `Reduced` makes `Cx::animate` show its target at once. No default: an app that
    /// has no setting says which one it means.
    fn motion(&self) -> MotionPref;
    fn ask(&mut self, ask: Self::Ask);
}

/// A named part of a key, with the key's life: two animated values (or two states of one reducer type) under one
/// data identity, e.g. `cx.animate(&Part(key.clone(), "knob"), ..)`.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct Part<K>(pub K, pub &'static str);
impl<App, K: Ident<App>> Ident<App> for Part<K> {
    fn life(&self, app: &App) -> Life {
        self.0.life(app)
    }
}

/// Object-safe view of an `Ident`. Method is `dyn_life` so it never collides with `Ident::life` on concrete keys.
pub(crate) trait DynKey<App>: Debug {
    fn as_any(&self) -> &dyn Any;
    fn dyn_eq(&self, other: &dyn DynKey<App>) -> bool;
    fn dyn_hash(&self, state: &mut dyn Hasher);
    fn dyn_life(&self, app: &App) -> Life;
}

impl<App, K: Ident<App>> DynKey<App> for K {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn dyn_eq(&self, other: &dyn DynKey<App>) -> bool {
        match other.as_any().downcast_ref::<K>() {
            Some(other) => other == self,
            None => false, // different key type: different key, not a fault
        }
    }
    fn dyn_hash(&self, mut state: &mut dyn Hasher) {
        self.hash(&mut state);
    }
    fn dyn_life(&self, app: &App) -> Life {
        self.life(app)
    }
}

/// A key of any `Ident` type, comparable and hashable across types, so one Store holds every declaration.
#[derive(Debug)]
pub struct AnyKey<App>(Rc<dyn DynKey<App>>);
// By hand: `derive(Clone)` would demand `App: Clone`, and the app model is never cloned.
impl<App> Clone for AnyKey<App> {
    fn clone(&self) -> Self {
        Self(Rc::clone(&self.0))
    }
}

impl<App> AnyKey<App> {
    pub(crate) fn new<K: Ident<App>>(key: K) -> Self {
        Self(Rc::new(key))
    }
    pub(crate) fn life(&self, app: &App) -> Life {
        self.0.dyn_life(app)
    }
}
impl<App> PartialEq for AnyKey<App> {
    fn eq(&self, other: &Self) -> bool {
        self.0.dyn_eq(&*other.0)
    }
}
impl<App> Eq for AnyKey<App> {}
impl<App> Hash for AnyKey<App> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.as_any().type_id().hash(state);
        self.0.dyn_hash(state);
    }
}
