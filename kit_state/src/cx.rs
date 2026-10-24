//! `Cx`: what a component function receives. Declares state, derives values, animates, reads the app, the clock and
//! the pointer.
use crate::act::Act;
use crate::anim::Tween;
use crate::ident::{AnyKey, Host, Ident, Life};
use crate::input::{Drag, Interaction, Unrouted};
use crate::intent::{Intent, StepIntent};
use crate::read::{Loaded, Resource};
use crate::store::{SlotId, Store};
use crate::track::{Command, Tracked, Verdict, read as read_verdict};
use core::any::TypeId;
use core::cell::Cell;
use core::fmt::Debug;
use core::hash::Hash;
use core::marker::PhantomData;
use core::ops::Deref;
use core::time::Duration;
use kit_core::{MotionPref, Redraw, ScrollView, Time, WidgetId};
use std::rc::Rc;

/// A non-capturing reducer closure `Fn(&mut S, &K, E, &App) -> Act<Ask, E>`, seen through its signature.
///
/// `M = (App, S, K, E)` is a marker that only names the signature. It is a TRAIT parameter, not only associated types,
/// because a blanket impl whose `S, K, E` appear nowhere but in the `Fn` bound is rejected (E0207). A closure has one
/// signature, so `M` is always inferred; it shortens `State<A, S, K, E, F>` to `State<F, M>` and every bound to
/// `F: Reducer<M>`. Helpers that take a state write `fn f<F: Reducer<M>, M>(s: &State<F, M>)`.
pub trait Reducer<M>: Copy + 'static {
    type App: Host;
    type S: Clone + 'static;
    type K: Ident<Self::App>;
    type E: Clone + Debug + PartialEq + 'static;
    fn step(
        &self,
        state: &mut Self::S,
        key: &Self::K,
        ev: Self::E,
        app: &Self::App,
    ) -> Act<<Self::App as Host>::Ask, Self::E>;
}
impl<A, S, K, E, F> Reducer<(A, S, K, E)> for F
where
    A: Host,
    S: Clone + 'static,
    K: Ident<A>,
    E: Clone + Debug + PartialEq + 'static,
    F: Fn(&mut S, &K, E, &A) -> Act<A::Ask, E> + Copy + 'static,
{
    type App = A;
    type S = S;
    type K = K;
    type E = E;
    fn step(&self, state: &mut S, key: &K, ev: E, app: &A) -> Act<A::Ask, E> {
        self(state, key, ev, app)
    }
}

pub struct Cx<'a, A: Host> {
    app: &'a A,
    store: &'a Store<A>,
    input: &'a dyn Interaction,
    redraw: Cell<Redraw>,
}

impl<'a, A: Host> Cx<'a, A> {
    /// A tree no input has been routed to (headless renders, tests, the HUD until the router is wired): nothing is
    /// hovered, pressed, focused or dragged. The `Unrouted` value says so; it is not a guess about the pointer.
    pub fn new(app: &'a A, store: &'a Store<A>) -> Self {
        Self::routed(app, store, &Unrouted)
    }
    /// A tree whose interaction reads come from the router's memory after the previous layout.
    pub fn routed(app: &'a A, store: &'a Store<A>, input: &'a dyn Interaction) -> Self {
        Self {
            app,
            store,
            input,
            redraw: Cell::new(Redraw::Idle),
        }
    }
    pub fn app(&self) -> &'a A {
        self.app
    }
    /// Read an unrestricted clock: conservatively redraw continuously for this view.
    pub fn now(&self) -> Time {
        self.redraw.set(Redraw::Now);
        self.app.now()
    }
    /// Read the clock for output that is constant until `at`. Registers that deadline only
    /// while it is in the future. After it is reached, the component must register its next
    /// change (or use `now` if it is animating). This never clears another reader's demand.
    pub fn redraw_at(&self, at: Time) -> Time {
        let now = self.app.now();
        if now < at {
            self.redraw.set(self.redraw.get().earliest(Redraw::At(at)));
        }
        now
    }
    /// Demand from this view build; read after the component tree has been built.
    pub fn redraw(&self) -> Redraw {
        self.redraw.get()
    }
    fn observe_life(&self, key: &impl Ident<A>) {
        if let Life::Leaving(_) = key.life(self.app) {
            self.redraw.set(Redraw::Now);
        }
    }

    /// Declare keyed state in place. `step` is the reducer; it is also the declaration's identity (its closure
    /// TYPE), so one closure used for N keys is one declaration with N states, and two closures are two declarations
    /// even with equal bodies. A capturing closure is rejected at compile time (the `const` below), because a captured
    /// value would make two calls of the same type behave differently and the type could not stand for the
    /// declaration. `init` runs only when the key has no stored value (the first frame it is seen, and every frame while it is not Live,
    /// because such a key is shown but never stored).
    ///
    /// The const assertion is evaluated at monomorphization, so `cargo check` does not report it; `cargo build` and
    /// `cargo test` do.
    ///
    /// ```compile_fail,E0080
    /// use kit_state::*;
    /// use kit_core::{MotionPref, Time};
    /// struct App;
    /// impl Host for App {
    ///     type Ask = ();
    ///     fn now(&self) -> Time { Time::default() }
    ///     fn motion(&self) -> MotionPref { MotionPref::Full }
    ///     fn ask(&mut self, _: ()) {}
    /// }
    /// #[derive(Clone, PartialEq, Eq, Hash, Debug)] struct K;
    /// impl Ident<App> for K { fn life(&self, _: &App) -> Life { Life::Live } }
    /// let (app, store) = (App, Store::<App>::new());
    /// let cx = Cx::new(&app, &store);
    /// let step_size = 5_u32;
    /// let _ = cx.state(&K, || 0_u32, move |n: &mut u32, _: &K, _: (), _: &App| { *n += step_size; Act::none() });
    /// ```
    pub fn state<S, K, E, F>(
        &self,
        key: &K,
        init: impl FnOnce() -> S,
        step: F,
    ) -> State<F, (A, S, K, E)>
    where
        S: Clone + 'static,
        K: Ident<A>,
        E: Clone + Debug + PartialEq + 'static,
        // The direct `Fn` bound (not only `Reducer`) is what lets rustc deduce unannotated closure parameters.
        F: Fn(&mut S, &K, E, &A) -> Act<A::Ask, E> + Copy + 'static,
    {
        const {
            assert!(
                size_of::<F>() == 0,
                "state reducers must not capture: put captured values in the key, the event or the app"
            )
        };
        self.observe_life(key);
        let id: SlotId<A> = (TypeId::of::<F>(), AnyKey::new(key.clone()));
        let value = match self.store.read::<S>(&id) {
            Some(value) => value,
            None => self.keep(key, id, Rc::new(init())),
        };
        State {
            value,
            sender: Sender {
                key: key.clone(),
                f: step,
                _marker: PhantomData,
            },
        }
    }

    /// Store a first-seen value. A key that is not Live is shown with it but never stored: nothing could collect it.
    fn keep<K: Ident<A>, S: 'static>(&self, key: &K, id: SlotId<A>, value: Rc<S>) -> Rc<S> {
        match key.life(self.app) {
            Life::Live => self.store.insert(id, Rc::clone(&value)),
            Life::Leaving(_) | Life::Over => {}
        }
        value
    }

    /// The server's verdict on the latest command of kind `C` that a reducer of `key` sent with `Act::ask_tracked`.
    /// A pending one past `C::WITHIN` reads `Unknown`; while it is still pending this view asks to be redrawn at the
    /// deadline, so an idle app wakes for it. A key that is not Live has no record: `Unsent`.
    pub fn track<C: Command>(&self, key: &impl Ident<A>) -> Verdict<C::Why> {
        self.observe_life(key);
        let id: SlotId<A> = (TypeId::of::<C>(), AnyKey::new(key.clone()));
        match self.store.read::<Tracked<C::Why>>(&id) {
            None => Verdict::Unsent,
            Some(tracked) => {
                let (verdict, demand) = read_verdict(&tracked, self.app.now());
                self.redraw.set(self.redraw.get().earliest(demand));
                verdict
            }
        }
    }

    /// Read a resource: its state in the app now, and a subscription that keeps it loaded while any view reads it.
    /// The app is told to `want` it the first frame it is read and to `release` it after a frame in which nothing
    /// read it (see [`sync_reads`](crate::sync_reads)).
    pub fn read<R: Resource<A>>(&self, resource: &R) -> Loaded<R::Value, R::Error> {
        self.store.reads.borrow_mut().read(resource);
        resource.state(self.app)
    }

    /// A value computed from the app and other state, recomputed every call (no memo, no dependency list). The same
    /// non-capture rule as `state`, so a derivation is a named, reusable function of (key, cx).
    pub fn derive<K, T, F>(&self, key: &K, f: F) -> T
    where
        F: Fn(&K, &Cx<'_, A>) -> T + Copy + 'static,
    {
        const {
            assert!(
                size_of::<F>() == 0,
                "derive closures must not capture: put captured values in the key"
            )
        };
        f(key, self)
    }

    /// `target`, approached over `over` (OutCubic) from wherever the value was when the target last changed. First
    /// sight shows `target` with no entrance (an entrance is a `Disclosure`). One animated value per key: animate two
    /// things of one key through [`Part`](crate::Part). Zero `over` and `MotionPref::Reduced` show the target at once.
    pub fn animate<K: Ident<A>>(&self, key: &K, target: f32, over: Duration) -> f32 {
        self.observe_life(key);
        let now = self.app.now();
        let id: SlotId<A> = (TypeId::of::<Tween>(), AnyKey::new(key.clone()));
        let tween = match self.store.read::<Tween>(&id) {
            Some(tween) if tween.to == target => tween,
            Some(tween) => {
                let retarget = Rc::new(tween.retarget(target, now, over));
                self.store.insert(id, Rc::clone(&retarget));
                retarget
            }
            None => self.keep(key, id, Rc::new(Tween::settled(target, now))),
        };
        match self.app.motion() {
            MotionPref::Full => tween.at(now),
            MotionPref::Reduced => tween.to,
        }
    }
    /// `animate` of 1.0 while `on`, 0.0 while off: a hover glow, a check mark, a switch knob.
    pub fn animate_bool<K: Ident<A>>(&self, key: &K, on: bool, over: Duration) -> f32 {
        let target = match on {
            true => 1.0,
            false => 0.0,
        };
        self.animate(key, target, over)
    }

    /// Interaction reads, as of the router's last pass. `key` is the same data the node was keyed with
    /// (`.key(&key)` uses `WidgetId::of`, and so do these); a positional node cannot be asked about.
    pub fn hovered(&self, key: &impl Hash) -> bool {
        self.input.hovered(WidgetId::of(key))
    }
    pub fn pressed(&self, key: &impl Hash) -> bool {
        self.input.pressed(WidgetId::of(key))
    }
    pub fn focused(&self, key: &impl Hash) -> bool {
        self.input.focused(WidgetId::of(key))
    }
    pub fn drag(&self, key: &impl Hash) -> Drag {
        self.input.drag(WidgetId::of(key))
    }
    /// The scroll area keyed `WidgetId::of(key)` as last measured. Also ties the router's memory of that area to
    /// the key's life: when the key ends, `Store::take_released` hands its id back for `UiMemory::forget_scrolls`.
    pub fn scroll<K: Ident<A>>(&self, key: &K) -> ScrollView {
        self.observe_life(key);
        let id = WidgetId::of(key);
        match key.life(self.app) {
            Life::Live => {
                let mut widgets = self.store.widgets.borrow_mut();
                widgets.entry(AnyKey::new(key.clone())).or_insert(id);
            }
            Life::Leaving(_) | Life::Over => {}
        }
        self.input.scroll(id)
    }
}

/// Owned, `'static`, `Clone` sender holding only the key and the ZST reducer, for closures the kit stores.
pub struct Sender<F: Reducer<M>, M> {
    key: F::K,
    f: F,
    _marker: PhantomData<fn() -> M>,
}
impl<F: Reducer<M>, M> Clone for Sender<F, M> {
    fn clone(&self) -> Self {
        Self {
            key: self.key.clone(),
            f: self.f,
            _marker: PhantomData,
        }
    }
}
impl<F: Reducer<M>, M: 'static> Sender<F, M> {
    /// Build (never apply) an intent addressed to this instance by data key.
    pub fn send(&self, ev: F::E) -> Intent<F::App> {
        Intent::Step(Rc::new(StepIntent {
            key: self.key.clone(),
            ev,
            f: self.f,
            _marker: PhantomData,
        }))
    }
}

/// The handle a component gets: derefs to the current value, builds intents for it.
pub struct State<F: Reducer<M>, M> {
    value: Rc<F::S>,
    sender: Sender<F, M>,
}
impl<F: Reducer<M>, M> Deref for State<F, M> {
    type Target = F::S;
    fn deref(&self) -> &F::S {
        &self.value
    }
}
impl<F: Reducer<M>, M: 'static> State<F, M> {
    pub fn send(&self, ev: F::E) -> Intent<F::App> {
        self.sender.send(ev)
    }
    pub fn sender(&self) -> Sender<F, M> {
        self.sender.clone()
    }
    /// A part of this state with its own event type: reads through `get`, sends `wrap(inner_event)` to this
    /// reducer, which routes it to the part in one arm.
    pub fn zoom<P, PE>(&self, get: fn(&F::S) -> &P, wrap: fn(PE) -> F::E) -> Zoom<'_, F, M, P, PE> {
        Zoom {
            state: self,
            get,
            wrap,
        }
    }
}

pub struct Zoom<'z, F: Reducer<M>, M, P, PE> {
    state: &'z State<F, M>,
    get: fn(&F::S) -> &P,
    wrap: fn(PE) -> F::E,
}
impl<F: Reducer<M>, M, P, PE> Deref for Zoom<'_, F, M, P, PE> {
    type Target = P;
    fn deref(&self) -> &P {
        (self.get)(&self.state.value)
    }
}
impl<F: Reducer<M>, M: 'static, P, PE> Zoom<'_, F, M, P, PE> {
    pub fn send(&self, ev: PE) -> Intent<F::App> {
        self.state.send((self.wrap)(ev))
    }
}
