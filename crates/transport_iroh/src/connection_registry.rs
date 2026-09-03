//! Connection lifecycle and the peer-URL → connection map.
//!
//! A connection is not usable the moment it is dialed or accepted: both peers
//! may dial at the same time, and only one of the two resulting connections
//! survives simultaneous-open resolution. This module owns the states a
//! connection moves through and, in [`ConnectionRegistry`], every mutation of
//! the map that decides which connection is the live one for a peer.

#[cfg(feature = "metrics")]
use crate::metrics::connection_counter_metric;
use kitsune2_api::Url;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use tokio::sync::watch;

/// Where a connection is in its life.
///
/// `Pending` is the only non-terminal state; `Superseded` and `Closed` are
/// absorbing, so a connection never comes back from either.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ConnectionState {
    /// Dialed or accepted, but preflight and simultaneous-open resolution
    /// have not finished. Application data must not be written yet.
    Pending,
    /// Preflight completed and this connection holds the peer's map slot.
    Active,
    /// A competing connection to the same peer was selected instead.
    Superseded,
    /// The connection ended.
    Closed,
}

/// How a wait for a connection's resolution ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ConnectionResolution {
    /// The connection completed preflight and is selected for the peer.
    Active,
    /// A competing connection was selected for the peer.
    Superseded,
    /// The connection ended before preflight completed.
    Closed,
}

/// The state machine of a single connection, observable by waiters.
///
/// Backed by a `watch` channel so a sender parked in
/// [`wait_for_resolution`](Self::wait_for_resolution) is woken the moment the
/// connection is activated or loses a race, with no polling.
#[derive(Debug)]
pub(crate) struct ConnectionLifecycle {
    state: watch::Sender<ConnectionState>,
}

impl ConnectionLifecycle {
    pub(crate) fn new() -> Self {
        Self {
            state: watch::Sender::new(ConnectionState::Pending),
        }
    }

    /// Waits until this connection is either usable or definitively not.
    pub(crate) async fn wait_for_resolution(&self) -> ConnectionResolution {
        let mut state = self.state.subscribe();
        loop {
            match *state.borrow_and_update() {
                ConnectionState::Active => {
                    return ConnectionResolution::Active;
                }
                ConnectionState::Superseded => {
                    return ConnectionResolution::Superseded;
                }
                ConnectionState::Closed => {
                    return ConnectionResolution::Closed;
                }
                ConnectionState::Pending => {}
            }

            if state.changed().await.is_err() {
                return ConnectionResolution::Closed;
            }
        }
    }

    pub(crate) fn is_active(&self) -> bool {
        *self.state.borrow() == ConnectionState::Active
    }

    /// Whether this connection has resolved as superseded by a preferred
    /// connection to the same peer, as opposed to closed for any other
    /// reason.
    pub(crate) fn is_superseded(&self) -> bool {
        *self.state.borrow() == ConnectionState::Superseded
    }

    /// Whether this connection may still take or hold a peer's map slot.
    pub(crate) fn is_live(&self) -> bool {
        matches!(
            *self.state.borrow(),
            ConnectionState::Pending | ConnectionState::Active
        )
    }

    /// Moves `Pending → Active`. Returns whether the transition happened.
    pub(crate) fn activate(&self) -> bool {
        let activated = self.state.send_if_modified(|state| {
            if *state == ConnectionState::Pending {
                *state = ConnectionState::Active;
                true
            } else {
                false
            }
        });

        #[cfg(feature = "metrics")]
        if activated {
            connection_counter_metric().add(1, &[]);
        }

        activated
    }

    pub(crate) fn mark_superseded(&self) {
        self.transition_to_terminal(ConnectionState::Superseded);
    }

    pub(crate) fn mark_closed(&self) {
        self.transition_to_terminal(ConnectionState::Closed);
    }

    /// Terminal states are absorbing: the first one to be set wins, and the
    /// metrics counter is decremented exactly once, only if the connection had
    /// been counted as active.
    fn transition_to_terminal(&self, terminal: ConnectionState) {
        #[cfg(feature = "metrics")]
        let mut was_active = false;
        self.state.send_if_modified(|state| match *state {
            ConnectionState::Pending | ConnectionState::Active => {
                #[cfg(feature = "metrics")]
                {
                    was_active = *state == ConnectionState::Active;
                }
                *state = terminal;
                true
            }
            ConnectionState::Superseded | ConnectionState::Closed => false,
        });

        #[cfg(feature = "metrics")]
        if was_active {
            connection_counter_metric().add(-1, &[]);
        }
    }
}

/// What the registry needs to know about a connection to arbitrate the slot.
///
/// Kept deliberately small so the arbitration can be tested without an iroh
/// endpoint, a reader task, or a handler.
pub(crate) trait RegistryEntry {
    /// This connection's state machine.
    fn lifecycle(&self) -> &ConnectionLifecycle;

    /// Whether this is the connection both peers converge on when a
    /// simultaneous dial produced two connections for the same pair.
    fn is_preferred(&self) -> bool;

    /// Close this connection because a competing one was selected. The
    /// connection's own reader observes the close and exits quietly.
    fn close_superseded(&self);
}

/// The live connection for each peer URL, and the rules for which connection
/// that is.
#[derive(Debug)]
pub(crate) struct ConnectionRegistry<E> {
    entries: Arc<RwLock<HashMap<Url, Arc<E>>>>,
}

impl<E> Clone for ConnectionRegistry<E> {
    fn clone(&self) -> Self {
        Self {
            entries: self.entries.clone(),
        }
    }
}

impl<E: RegistryEntry> Default for ConnectionRegistry<E> {
    fn default() -> Self {
        Self::new()
    }
}

impl<E: RegistryEntry> ConnectionRegistry<E> {
    pub(crate) fn new() -> Self {
        Self {
            entries: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub(crate) fn get(&self, peer: &Url) -> Option<Arc<E>> {
        self.entries.read().expect("poisoned").get(peer).cloned()
    }

    /// Claim `peer`'s slot for `candidate`, resolving any simultaneous-open
    /// race deterministically.
    ///
    /// - A candidate in a terminal state can never take the slot.
    /// - A free slot is taken.
    /// - Re-registering the current entry is a no-op.
    /// - An incumbent in a terminal state is evicted: it can never carry
    ///   traffic again, and defending it would reject the connection that
    ///   replaces it.
    /// - Otherwise the incumbent is only defended when it is itself the
    ///   preferred connection. Exactly one of the two connections in a genuine
    ///   simultaneous open is preferred, so a non-preferred incumbent is a
    ///   leftover in the same direction as the candidate and loses to it.
    ///
    /// Returns whether `candidate` holds the slot afterwards. A displaced
    /// incumbent is marked superseded and closed; a rejected candidate is
    /// marked superseded.
    pub(crate) fn register_candidate(
        &self,
        peer: &Url,
        candidate: &Arc<E>,
    ) -> bool {
        let displaced = {
            let mut entries = self.entries.write().expect("poisoned");
            if !candidate.lifecycle().is_live() {
                return false;
            }
            match entries.get(peer) {
                Some(existing) if Arc::ptr_eq(existing, candidate) => {
                    return true;
                }
                Some(existing) if !existing.lifecycle().is_live() => {
                    entries.insert(peer.clone(), candidate.clone())
                }
                Some(existing)
                    if existing.is_preferred() && !candidate.is_preferred() =>
                {
                    candidate.lifecycle().mark_superseded();
                    return false;
                }
                _ => entries.insert(peer.clone(), candidate.clone()),
            }
        };

        if let Some(displaced) = displaced {
            displaced.lifecycle().mark_superseded();
            displaced.close_superseded();
        }

        true
    }

    /// Mark `entry` active if it is still `peer`'s slot holder.
    ///
    /// Returns whether the entry is active afterwards, so an already-active
    /// entry reports success.
    pub(crate) fn activate(&self, peer: &Url, entry: &Arc<E>) -> bool {
        // A write lock is taken even though this method never writes to the
        // map: it serializes against `register_candidate` so the entry
        // cannot be displaced between the "is this still the current entry"
        // check and the lifecycle transition below, and holding it across
        // that transition keeps the critical section deliberately small.
        let entries = self.entries.write().expect("poisoned");
        let is_current = entries
            .get(peer)
            .is_some_and(|current| Arc::ptr_eq(current, entry));
        if !is_current {
            return false;
        }

        entry.lifecycle().activate() || entry.lifecycle().is_active()
    }

    /// Remove `entry` from the map if it is still `peer`'s slot holder.
    /// Returns whether it was.
    pub(crate) fn remove_if_current(&self, peer: &Url, entry: &Arc<E>) -> bool {
        let mut entries = self.entries.write().expect("poisoned");
        match entries.get(peer) {
            Some(current) if Arc::ptr_eq(current, entry) => {
                entries.remove(peer);
                true
            }
            _ => false,
        }
    }

    /// Remove and return whatever holds `peer`'s slot.
    pub(crate) fn take(&self, peer: &Url) -> Option<Arc<E>> {
        self.entries.write().expect("poisoned").remove(peer)
    }

    /// The peers with an active connection. A pending or superseded entry is
    /// not a connected peer.
    pub(crate) fn active_peers(&self) -> Vec<Url> {
        self.entries
            .read()
            .expect("poisoned")
            .iter()
            .filter(|(_, entry)| entry.lifecycle().is_active())
            .map(|(peer, _)| peer.clone())
            .collect()
    }

    /// The active entries with their peer URLs, for stats reporting.
    pub(crate) fn active_entries(&self) -> Vec<(Url, Arc<E>)> {
        self.entries
            .read()
            .expect("poisoned")
            .iter()
            .filter(|(_, entry)| entry.lifecycle().is_active())
            .map(|(peer, entry)| (peer.clone(), entry.clone()))
            .collect()
    }

    /// Empty the map and return everything that was in it, for shutdown.
    pub(crate) fn drain(&self) -> Vec<Arc<E>> {
        self.entries
            .write()
            .expect("poisoned")
            .drain()
            .map(|(_, entry)| entry)
            .collect()
    }
}
