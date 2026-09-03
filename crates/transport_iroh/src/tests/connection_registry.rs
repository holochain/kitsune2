//! Unit tests for the peer-URL → connection map and the simultaneous-open
//! tie-break, driven against fake entries so no iroh endpoint is involved.

use crate::connection_registry::{
    ConnectionLifecycle, ConnectionRegistry, RegistryEntry,
};
use kitsune2_api::Url;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

struct FakeEntry {
    lifecycle: ConnectionLifecycle,
    preferred: bool,
    close_calls: AtomicUsize,
}

impl FakeEntry {
    fn new(preferred: bool) -> Arc<Self> {
        Arc::new(Self {
            lifecycle: ConnectionLifecycle::new(),
            preferred,
            close_calls: AtomicUsize::new(0),
        })
    }

    fn close_calls(&self) -> usize {
        self.close_calls.load(Ordering::SeqCst)
    }
}

impl RegistryEntry for FakeEntry {
    fn lifecycle(&self) -> &ConnectionLifecycle {
        &self.lifecycle
    }

    fn is_preferred(&self) -> bool {
        self.preferred
    }

    fn close_superseded(&self) {
        self.close_calls.fetch_add(1, Ordering::SeqCst);
    }
}

fn peer() -> Url {
    Url::from_str(
        "https://relay.example.com:443/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    )
    .unwrap()
}

#[test]
fn a_free_slot_is_taken() {
    let registry = ConnectionRegistry::<FakeEntry>::new();
    let entry = FakeEntry::new(false);
    assert!(registry.register_candidate(&peer(), &entry));
    assert!(Arc::ptr_eq(&registry.get(&peer()).unwrap(), &entry));
}

#[test]
fn re_registering_the_same_entry_is_a_no_op() {
    let registry = ConnectionRegistry::<FakeEntry>::new();
    let entry = FakeEntry::new(true);
    assert!(registry.register_candidate(&peer(), &entry));
    assert!(registry.register_candidate(&peer(), &entry));
    assert_eq!(entry.close_calls(), 0, "the entry must not close itself");
}

#[test]
fn a_preferred_incumbent_defeats_a_non_preferred_candidate() {
    let registry = ConnectionRegistry::<FakeEntry>::new();
    let incumbent = FakeEntry::new(true);
    let candidate = FakeEntry::new(false);
    assert!(registry.register_candidate(&peer(), &incumbent));

    assert!(!registry.register_candidate(&peer(), &candidate));
    assert!(!candidate.lifecycle().is_live(), "the loser is superseded");
    assert!(Arc::ptr_eq(&registry.get(&peer()).unwrap(), &incumbent));
}

#[test]
fn a_preferred_candidate_displaces_a_non_preferred_incumbent() {
    let registry = ConnectionRegistry::<FakeEntry>::new();
    let incumbent = FakeEntry::new(false);
    let candidate = FakeEntry::new(true);
    assert!(registry.register_candidate(&peer(), &incumbent));

    assert!(registry.register_candidate(&peer(), &candidate));
    assert!(Arc::ptr_eq(&registry.get(&peer()).unwrap(), &candidate));
    assert!(!incumbent.lifecycle().is_live());
    assert_eq!(
        incumbent.close_calls(),
        1,
        "the displaced connection must be closed exactly once"
    );
}

#[test]
fn a_same_direction_newcomer_replaces_the_incumbent() {
    let registry = ConnectionRegistry::<FakeEntry>::new();
    let incumbent = FakeEntry::new(false);
    let newcomer = FakeEntry::new(false);
    assert!(registry.register_candidate(&peer(), &incumbent));

    assert!(
        registry.register_candidate(&peer(), &newcomer),
        "neither is preferred, so the newer connection wins"
    );
    assert!(Arc::ptr_eq(&registry.get(&peer()).unwrap(), &newcomer));
}

#[test]
fn a_terminal_incumbent_is_evicted_even_when_preferred() {
    let registry = ConnectionRegistry::<FakeEntry>::new();
    let incumbent = FakeEntry::new(true);
    assert!(registry.register_candidate(&peer(), &incumbent));
    incumbent.lifecycle().mark_closed();

    let newcomer = FakeEntry::new(false);
    assert!(registry.register_candidate(&peer(), &newcomer));
    assert!(Arc::ptr_eq(&registry.get(&peer()).unwrap(), &newcomer));
}

#[test]
fn a_terminal_candidate_cannot_take_the_slot() {
    let registry = ConnectionRegistry::<FakeEntry>::new();
    let candidate = FakeEntry::new(true);
    candidate.lifecycle().mark_superseded();
    assert!(!registry.register_candidate(&peer(), &candidate));
    assert!(registry.get(&peer()).is_none());
}

#[test]
fn activate_only_succeeds_for_the_current_entry() {
    let registry = ConnectionRegistry::<FakeEntry>::new();
    let incumbent = FakeEntry::new(true);
    let stranger = FakeEntry::new(true);
    assert!(registry.register_candidate(&peer(), &incumbent));

    assert!(registry.activate(&peer(), &incumbent));
    assert!(incumbent.lifecycle().is_active());
    assert!(!registry.activate(&peer(), &stranger));
    assert!(!stranger.lifecycle().is_active());
}

#[test]
fn activate_is_idempotent() {
    let registry = ConnectionRegistry::<FakeEntry>::new();
    let entry = FakeEntry::new(true);
    assert!(registry.register_candidate(&peer(), &entry));
    assert!(registry.activate(&peer(), &entry));
    assert!(
        registry.activate(&peer(), &entry),
        "activating an already-active entry must still report success"
    );
}

#[test]
fn activate_does_not_revive_a_superseded_entry() {
    let registry = ConnectionRegistry::<FakeEntry>::new();
    let entry = FakeEntry::new(true);
    assert!(registry.register_candidate(&peer(), &entry));
    entry.lifecycle().mark_superseded();
    assert!(!registry.activate(&peer(), &entry));
    assert!(!entry.lifecycle().is_active());
}

#[test]
fn remove_if_current_only_removes_the_current_entry() {
    let registry = ConnectionRegistry::<FakeEntry>::new();
    let incumbent = FakeEntry::new(true);
    let stranger = FakeEntry::new(true);
    assert!(registry.register_candidate(&peer(), &incumbent));

    assert!(!registry.remove_if_current(&peer(), &stranger));
    assert!(registry.get(&peer()).is_some());
    assert!(registry.remove_if_current(&peer(), &incumbent));
    assert!(registry.get(&peer()).is_none());
}

#[test]
fn only_active_entries_are_reported_as_peers() {
    let registry = ConnectionRegistry::<FakeEntry>::new();
    let entry = FakeEntry::new(true);
    assert!(registry.register_candidate(&peer(), &entry));
    assert!(
        registry.active_peers().is_empty(),
        "a pending entry is not a connected peer"
    );

    assert!(registry.activate(&peer(), &entry));
    assert_eq!(registry.active_peers(), vec![peer()]);
}
