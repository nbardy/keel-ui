//! `cx.track`: the server's verdict on the latest command of one kind sent for one key.
//!
//! The kit never talks to a network. A reducer says a command was sent (`Act::ask_tracked::<C>`); the host reports
//! what its server answered (`Store::settle`); a view reads the result (`Cx::track`). The record lives in the
//! `Store` under (command kind, key), so it has the key's life: collected with it, and a verdict for an ended key is
//! refused as `Late` and counted.
//!
//! DIFFERS FROM docs/DESIGN.md (L4): there is no ledger, no processed-up-to watermark and no `Withdrawn` here. The
//! kit matches replies to sends by order (see [`Tracked`]); an unsent command is not queued by the kit, so there is
//! nothing to withdraw.
use crate::ident::{AnyKey, Host};
use crate::intent::{Late, LateWhy};
use crate::store::{Deadlines, Slots, Store, slot_value};
use core::any::{Any, TypeId};
use core::fmt::Debug;
use core::time::Duration;
use kit_core::{Redraw, Time};
use std::rc::Rc;

/// A kind of command whose outcome the UI shows. A marker type (usually a unit struct), not the ask itself.
pub trait Command: 'static {
    /// What the server says when it refuses (a typed reason, not a string).
    type Why: Clone + Debug + PartialEq + 'static;
    /// How long a sent command may go unanswered before the UI calls its outcome `Unknown`.
    const WITHIN: Duration;
}

/// What the UI knows about the latest command of one kind for one key.
#[derive(Clone, Debug, PartialEq)]
pub enum Verdict<W> {
    /// Nothing of this kind was sent for this key (or its record was collected with the key).
    Unsent,
    Pending {
        since: Time,
    },
    Confirmed {
        at: Time,
    },
    Refused {
        why: W,
        at: Time,
    },
    /// No answer within `Command::WITHIN`. Not a failure: a late answer still settles it.
    Unknown {
        since: Time,
    },
}

/// What the host's server answered.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome<W> {
    Confirmed,
    Refused(W),
}

/// A verdict the store accepted.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Settled {
    /// It answered the newest send: `cx.track` now shows it.
    Shown,
    /// It answered an older send that a newer one replaced: `cx.track` still waits for the newest.
    Superseded,
}

/// The stored record. `owed` is the number of sends not yet answered.
///
/// Replies are matched to sends by ORDER, because the host's ask carries no ticket back to the kit: the Nth reply
/// answers the Nth send, so only the reply to the newest send changes the verdict. That holds for one ordered
/// connection (the protocol's processed-up-to watermark). A server that may answer out of order must use a distinct
/// key per send (`Part(key, n)`), which also gives each its own verdict. A send whose reply is lost for good leaves
/// `owed` too high until the key ends; `Unknown` shows it and the next reply is read as the older one.
///
/// The record holds no deadline: it is `since + C::WITHIN`, and the store keeps the newest unanswered send's deadline
/// in its own typed index (`Store::deadlines`) so `Store::redraw` wakes an idle host for it without downcasting slots.
#[derive(Clone)]
pub(crate) struct Tracked<W> {
    verdict: Verdict<W>,
    owed: u32,
}

/// When a send made at `since` reads `Unknown`.
fn deadline(since: Time, within: Duration) -> Time {
    Time(since.0 + within)
}

/// A send recorded by a reducer's `Act`, erased so `Act` stays generic only over `Ask` and `Ev`.
#[derive(Copy, Clone)]
pub(crate) struct Sent {
    pub(crate) kind: TypeId,
    within: Duration,
    begin: fn(Option<&dyn Any>, Time) -> Box<dyn Any>,
}
impl Sent {
    pub(crate) fn of<C: Command>() -> Self {
        Self {
            kind: TypeId::of::<C>(),
            within: C::WITHIN,
            begin: begin::<C>,
        }
    }
    pub(crate) fn begin(&self, previous: Option<&dyn Any>, now: Time) -> Box<dyn Any> {
        (self.begin)(previous, now)
    }
}
impl Debug for Sent {
    fn fmt(&self, out: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(out, "Sent({:?})", self.kind)
    }
}

/// The newest send is pending since `now`; earlier unanswered sends stay owed.
fn begin<C: Command>(previous: Option<&dyn Any>, now: Time) -> Box<dyn Any> {
    let owed = previous.map_or(0, |slot| slot_value::<Tracked<C::Why>>(slot).owed);
    Box::new(Rc::new(Tracked::<C::Why> {
        verdict: Verdict::Pending { since: now },
        owed: owed + 1,
    }))
}

/// Record the sends a step made, under the step's own key. The newest send's deadline replaces the slot's last one.
pub(crate) fn record_sent<A: Host>(
    slots: &mut Slots<A>,
    deadlines: &mut Deadlines<A>,
    key: &AnyKey<A>,
    sent: &[Sent],
    now: Time,
) {
    for send in sent {
        let id = (send.kind, AnyKey::clone(key));
        let next = send.begin(slots.get(&id).map(|slot| &**slot), now);
        slots.insert(id.clone(), next);
        deadlines.insert(id, deadline(now, send.within));
    }
}

/// The verdict as a view sees it at `now`, and the redraw it needs (the deadline, while still pending).
pub(crate) fn read<C: Command>(tracked: &Tracked<C::Why>, now: Time) -> (Verdict<C::Why>, Redraw) {
    match &tracked.verdict {
        Verdict::Pending { since } if now >= deadline(*since, C::WITHIN) => {
            (Verdict::Unknown { since: *since }, Redraw::Idle)
        }
        Verdict::Pending { since } => (
            tracked.verdict.clone(),
            Redraw::At(deadline(*since, C::WITHIN)),
        ),
        Verdict::Unsent
        | Verdict::Confirmed { .. }
        | Verdict::Refused { .. }
        | Verdict::Unknown { .. } => (tracked.verdict.clone(), Redraw::Idle),
    }
}

impl<A: Host> Store<A> {
    /// The host reports what its server answered for the latest command of kind `C` sent for `key`.
    ///
    /// Refused, counted in `late()` and returned: a key that is `Leaving` or `Over` (`Leaving`/`Gone`), and a reply
    /// with no send awaiting it (`Unsent`: never sent, or already answered). The caller decides what that means;
    /// the store never drops it silently.
    pub fn settle<C: Command, K: crate::ident::Ident<A>>(
        &mut self,
        app: &A,
        key: &K,
        outcome: Outcome<C::Why>,
    ) -> Result<Settled, LateWhy> {
        let answered = self.settle_live::<C, K>(app, key, outcome);
        if let Err(why) = answered {
            self.refuse(Late {
                why,
                intent: format!("Verdict({:?}, {key:?})", core::any::type_name::<C>()),
            });
        }
        answered
    }

    fn settle_live<C: Command, K: crate::ident::Ident<A>>(
        &mut self,
        app: &A,
        key: &K,
        outcome: Outcome<C::Why>,
    ) -> Result<Settled, LateWhy> {
        use crate::ident::Life;
        match key.life(app) {
            Life::Live => {}
            Life::Leaving(_) => return Err(LateWhy::Leaving),
            Life::Over => return Err(LateWhy::Gone),
        }
        let id = (TypeId::of::<C>(), crate::ident::AnyKey::new(key.clone()));
        let slot = self.slots.get_mut().get_mut(&id).ok_or(LateWhy::Unsent)?;
        let tracked = Rc::make_mut(crate::store::slot_value_mut::<Tracked<C::Why>>(&mut **slot));
        match tracked.owed {
            0 => Err(LateWhy::Unsent),
            1 => {
                tracked.owed = 0;
                // The newest send is answered: nothing is left for the host to wake for.
                self.deadlines.remove(&id);
                let at = app.now();
                tracked.verdict = match outcome {
                    Outcome::Confirmed => Verdict::Confirmed { at },
                    Outcome::Refused(why) => Verdict::Refused { why, at },
                };
                Ok(Settled::Shown)
            }
            _ => {
                tracked.owed -= 1;
                Ok(Settled::Superseded)
            }
        }
    }
}
