//! Intents: what a node's binding carries. Plain data plus one ZST closure; applied only by `dispatch`.
use crate::act::{Fault, Wake};
use crate::cx::Reducer;
use crate::ident::{AnyKey, Host, Ident, Life};
use crate::store::{SlotId, Slots, Store, slot_value_mut};
use crate::track::{Sent, record_sent};
use core::any::{Any, TypeId};
use core::fmt::{self, Debug};
use core::marker::PhantomData;
use std::rc::Rc;

/// Why an intent was refused.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum LateWhy {
    /// The key is Over, was collected, or never stored (it was rendered while not Live).
    Gone,
    /// The key is Leaving: its state is kept inert for the exit.
    Leaving,
    /// A verdict arrived for a command that is not awaiting one: never sent for this key, or already answered.
    Unsent,
}

/// A refused intent, counted in the Store.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Late {
    pub why: LateWhy,
    /// `Debug` of the refused intent (declaration key and event).
    pub intent: String,
}

/// What one applied step asks of the dispatcher: the reducer's `Act` with its wake-up already made into the intent
/// that will deliver it to the same slot.
pub struct Effects<A: Host> {
    pub(crate) asks: Vec<A::Ask>,
    pub(crate) faults: Vec<Fault>,
    pub(crate) sent: Vec<Sent>,
    pub(crate) wake: Wake<Rc<dyn ErasedStep<A>>>,
    pub(crate) slot: SlotId<A>,
}

/// Unnameable outside the crate: only `Intent` and `dispatch` use it.
pub trait ErasedStep<A: Host>: Debug {
    fn apply(&self, slots: &mut Slots<A>, app: &A) -> Result<Effects<A>, LateWhy>;
    /// Same declaration, same key, same event. Lets a message type that embeds an `Intent` be `PartialEq`.
    fn same_as(&self, other: &dyn ErasedStep<A>) -> bool;
    fn as_any(&self) -> &dyn Any;
}

/// The ZST reducer `f` travels with the key and event; `TypeId::of::<F>()` is the declaration.
pub(crate) struct StepIntent<F: Reducer<M>, M> {
    pub(crate) key: F::K,
    pub(crate) ev: F::E,
    pub(crate) f: F,
    pub(crate) _marker: PhantomData<fn() -> M>,
}

impl<F: Reducer<M>, M> Debug for StepIntent<F, M> {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(out, "Step({:?}, {:?})", self.key, self.ev)
    }
}

impl<A, F, M> ErasedStep<A> for StepIntent<F, M>
where
    A: Host,
    F: Reducer<M, App = A>,
    M: 'static,
{
    fn as_any(&self) -> &dyn Any {
        self
    }
    /// Real equality: `Self`'s TypeId includes the reducer type `F` (the declaration), so a successful downcast means
    /// the same declaration, and then key and event compare with their own `PartialEq`. (It used to compare `Debug`
    /// text, which calls two events equal whenever their Debug output elides a field.)
    fn same_as(&self, other: &dyn ErasedStep<A>) -> bool {
        match other.as_any().downcast_ref::<Self>() {
            Some(other) => other.key == self.key && other.ev == self.ev,
            None => false, // another declaration: a different intent, not a fault
        }
    }
    fn apply(&self, slots: &mut Slots<A>, app: &A) -> Result<Effects<A>, LateWhy> {
        match self.key.life(app) {
            Life::Live => {
                let id: SlotId<A> = (TypeId::of::<F>(), AnyKey::new(self.key.clone()));
                let slot = slots.get_mut(&id).ok_or(LateWhy::Gone)?;
                let state = Rc::make_mut(slot_value_mut::<F::S>(&mut **slot));
                let act = self.f.step(state, &self.key, self.ev.clone(), app);
                let wake = match act.wake {
                    Wake::Keep => Wake::Keep,
                    Wake::Cancel => Wake::Cancel,
                    Wake::At(at, ev) => {
                        let step: Rc<dyn ErasedStep<A>> = Rc::new(StepIntent {
                            key: self.key.clone(),
                            ev,
                            f: self.f,
                            _marker: PhantomData,
                        });
                        Wake::At(at, step)
                    }
                };
                Ok(Effects {
                    asks: act.asks,
                    faults: act.faults,
                    sent: act.sent,
                    wake,
                    slot: id,
                })
            }
            Life::Leaving(_) => Err(LateWhy::Leaving),
            Life::Over => Err(LateWhy::Gone),
        }
    }
}

/// The `Msg` of a kit_view `Node<Msg>`. Cheap to clone.
pub enum Intent<A: Host> {
    /// Run a keyed state reducer.
    Step(Rc<dyn ErasedStep<A>>),
    /// Straight to the app.
    Ask(A::Ask),
    /// In order; each sees the previous one's effect.
    All(Vec<Intent<A>>),
}

impl<A: Host> Clone for Intent<A> {
    fn clone(&self) -> Self {
        match self {
            Intent::Step(step) => Intent::Step(Rc::clone(step)),
            Intent::Ask(ask) => Intent::Ask(ask.clone()),
            Intent::All(all) => Intent::All(all.clone()),
        }
    }
}
/// Structural equality: same declaration, key and event. Needed because hosts compare `Routing<HudAction>` (a derived
/// `PartialEq`) and `HudAction::Tray` embeds an `Intent`; events therefore carry a `PartialEq` bound.
impl<A: Host> PartialEq for Intent<A>
where
    A::Ask: PartialEq,
{
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Intent::Step(a), Intent::Step(b)) => a.same_as(&**b),
            (Intent::Ask(a), Intent::Ask(b)) => a == b,
            (Intent::All(a), Intent::All(b)) => a == b,
            (Intent::Step(_) | Intent::Ask(_) | Intent::All(_), _) => false,
        }
    }
}
impl<A: Host> Debug for Intent<A> {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Intent::Step(step) => step.fmt(out),
            Intent::Ask(ask) => write!(out, "Ask({ask:?})"),
            Intent::All(all) => out.debug_list().entries(all).finish(),
        }
    }
}

/// Fire every wake-up due at `app.now()`, earliest first (ties in scheduling order), each through [`dispatch`]. Only
/// wake-ups due when the call starts fire: one a reducer schedules for `<= now` while firing waits for the next call,
/// so a reducer that keeps rescheduling cannot spin the host. Call once per frame before building the view; ask
/// [`Store::next_due`] when to call next if the host sleeps between frames.
pub fn wake_due<A: Host>(app: &mut A, store: &mut Store<A>) {
    for seq in store.due(app.now()) {
        // A wake-up fired earlier in this loop may have replaced this one or ended its key.
        if let Some(step) = store.take_timer(seq) {
            dispatch(app, store, Intent::Step(step));
        }
    }
}

/// Apply one intent: steps in order, asks forwarded to `Host::ask`, then a collect pass after every event.
/// A refused step is recorded in `store.late()`; nothing is dropped silently.
pub fn dispatch<A: Host>(app: &mut A, store: &mut Store<A>, intent: Intent<A>) {
    match intent {
        Intent::Step(step) => {
            match step.apply(store.slots.get_mut(), app) {
                Ok(effects) => {
                    store.faults.extend(effects.faults);
                    record_sent(
                        store.slots.get_mut(),
                        &effects.slot.1,
                        &effects.sent,
                        app.now(),
                    );
                    match effects.wake {
                        Wake::Keep => {}
                        Wake::At(at, step) => store.schedule(effects.slot, at, step),
                        Wake::Cancel => store.unschedule(&effects.slot),
                    }
                    for ask in effects.asks {
                        app.ask(ask);
                    }
                }
                Err(why) => store.refuse(Late {
                    why,
                    intent: format!("{step:?}"),
                }),
            }
            store.collect(app);
        }
        Intent::Ask(ask) => {
            app.ask(ask);
            store.collect(app);
        }
        Intent::All(all) => {
            for intent in all {
                dispatch(app, store, intent);
            }
        }
    }
}
