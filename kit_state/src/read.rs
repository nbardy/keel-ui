//! `cx.read`: reading a resource subscribes to it, and a resource nobody read for a frame is released.
//!
//! Data only. The app holds the resource's value (`Resource::state`); the host does the fetching. The kit's part is
//! to notice WHO is reading: the first frame a view reads a resource the app is told to `want` it, and after a frame
//! in which no view read it the app is told to `release` it. Both are ordinary asks, produced by [`sync_reads`],
//! so the view needs no cleanup code: closing the panel that reads is the release.
use crate::ident::{AnyKey, Host, Ident, Life};
use crate::store::Store;
use core::fmt::Debug;

/// A resource a view can read: a key for something the host fetches, whose current state the app holds.
pub trait Resource<A: Host>: Ident<A> {
    type Value;
    type Error;
    /// What the app holds for it now.
    fn state(&self, app: &A) -> Loaded<Self::Value, Self::Error>;
    /// Asked once when the first view reads it.
    fn want(&self) -> A::Ask;
    /// Asked once after a frame in which no view read it (or its key ended).
    fn release(&self) -> A::Ask;
}

#[derive(Clone, Debug, PartialEq)]
pub enum Loaded<T, E> {
    Loading,
    Ready(T),
    Failed(E),
}

/// Whether the app has been told to load the resource yet.
enum Told<Ask> {
    NotYet(Ask),
    Wanted,
}

/// One live subscription.
pub(crate) struct Subscription<A: Host> {
    /// Subscription order, so asks come out in a stable order (the table is a hash map).
    order: u64,
    told: Told<A::Ask>,
    release: A::Ask,
    /// A view read it since the last `sync_reads`.
    read: bool,
}

pub(crate) struct Reads<A: Host> {
    subscriptions: std::collections::HashMap<AnyKey<A>, Subscription<A>>,
    next: u64,
}
impl<A: Host> Default for Reads<A> {
    fn default() -> Self {
        Self {
            subscriptions: std::collections::HashMap::new(),
            next: 0,
        }
    }
}
impl<A: Host> Reads<A> {
    /// A view read `key` this frame.
    pub(crate) fn read<R: Resource<A>>(&mut self, resource: &R) {
        let next = &mut self.next;
        let sub = self
            .subscriptions
            .entry(AnyKey::new(resource.clone()))
            .or_insert_with(|| {
                *next += 1;
                Subscription {
                    order: *next,
                    told: Told::NotYet(resource.want()),
                    release: resource.release(),
                    read: false,
                }
            });
        sub.read = true;
    }
    pub(crate) fn len(&self) -> usize {
        self.subscriptions.len()
    }
}

impl<A: Host> Store<A> {
    /// How many resources are subscribed right now.
    pub fn subscribed(&self) -> usize {
        self.reads.borrow().len()
    }
}

/// End of a view build: tell the app which resources became wanted and which stopped being read, then start the
/// next frame's reads from nothing. Call once after every build of the view, with the same `Store`; a host that
/// builds twice per frame would release what only the first build read. A resource whose key is no longer `Live` is
/// released even if read (nothing may keep it loaded).
pub fn sync_reads<A: Host>(app: &mut A, store: &mut Store<A>) {
    let mut asks: Vec<(u64, A::Ask)> = Vec::new();
    store.reads.get_mut().subscriptions.retain(|key, sub| {
        let keep = sub.read
            && match key.life(app) {
                Life::Live => true,
                Life::Leaving(_) | Life::Over => false,
            };
        let told = core::mem::replace(&mut sub.told, Told::Wanted);
        match (keep, told) {
            (true, Told::NotYet(want)) => asks.push((sub.order, want)),
            (true, Told::Wanted) => {}
            // Dropped before the app was ever told to load it: there is nothing to release.
            (false, Told::NotYet(_)) => {}
            (false, Told::Wanted) => asks.push((sub.order, sub.release.clone())),
        }
        sub.read = false;
        keep
    });
    asks.sort_by_key(|(order, _)| *order);
    for (_, ask) in asks {
        app.ask(ask);
    }
}
