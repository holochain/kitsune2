//! Unit tests for the peer-URL → connection map and the simultaneous-open
//! tie-break, driven against fake entries so no iroh endpoint is involved.

use crate::connection_registry::{
    ConnectionLifecycle, ConnectionRegistry, ConnectionResolution,
    RegistryEntry,
};
use kitsune2_api::Url;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

struct FakeEntry {
    lifecycle: ConnectionLifecycle,
    preferred: bool,
    close_calls: AtomicUsize,
    /// Whether the lifecycle already read as superseded when
    /// `close_superseded` ran.
    superseded_at_close: AtomicBool,
}

impl FakeEntry {
    fn new(preferred: bool) -> Arc<Self> {
        Arc::new(Self {
            lifecycle: ConnectionLifecycle::new(),
            preferred,
            close_calls: AtomicUsize::new(0),
            superseded_at_close: AtomicBool::new(false),
        })
    }

    fn close_calls(&self) -> usize {
        self.close_calls.load(Ordering::SeqCst)
    }

    fn superseded_at_close(&self) -> bool {
        self.superseded_at_close.load(Ordering::SeqCst)
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
        self.superseded_at_close
            .store(self.lifecycle.is_superseded(), Ordering::SeqCst);
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

/// A send that is already writing to the displaced connection only learns the
/// write failed because of simultaneous-open resolution, rather than because
/// the peer is gone, by asking the lifecycle. So the displaced entry must
/// already read as superseded by the time its connection is closed, otherwise
/// that send reports a spurious error for a message it could have retried.
#[test]
fn a_displaced_entry_is_superseded_before_it_is_closed() {
    let registry = ConnectionRegistry::<FakeEntry>::new();
    let incumbent = FakeEntry::new(false);
    let candidate = FakeEntry::new(true);
    assert!(registry.register_candidate(&peer(), &incumbent));

    assert!(registry.register_candidate(&peer(), &candidate));
    assert_eq!(incumbent.close_calls(), 1);
    assert!(
        incumbent.superseded_at_close(),
        "the displaced connection must be marked superseded before it is closed"
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

/// Let a reader finish preflight while its replacement is about to mark it
/// superseded. Reader cleanup must never win by marking the loser closed.
#[test]
fn displacement_preserves_superseded_resolution_during_reader_cleanup() {
    use std::sync::{Barrier, mpsc};
    use std::time::Duration;

    struct PausedEntry {
        lifecycle: ConnectionLifecycle,
        accesses: AtomicUsize,
        preferred: bool,
        reached: mpsc::Sender<()>,
        resume: Arc<Barrier>,
    }

    impl RegistryEntry for PausedEntry {
        fn lifecycle(&self) -> &ConnectionLifecycle {
            // The incumbent is inspected when inserted, when challenged, and
            // then when marked superseded. Pause at that last boundary.
            if !self.preferred
                && self.accesses.fetch_add(1, Ordering::SeqCst) == 2
            {
                self.reached.send(()).expect("reader must be waiting");
                self.resume.wait();
            }
            &self.lifecycle
        }

        fn is_preferred(&self) -> bool {
            self.preferred
        }

        fn close_superseded(&self) {}
    }

    let (reached, receive) = mpsc::channel();
    let resume = Arc::new(Barrier::new(2));
    let incumbent = Arc::new(PausedEntry {
        lifecycle: ConnectionLifecycle::new(),
        accesses: AtomicUsize::new(0),
        preferred: false,
        reached: reached.clone(),
        resume: resume.clone(),
    });
    let candidate = Arc::new(PausedEntry {
        lifecycle: ConnectionLifecycle::new(),
        accesses: AtomicUsize::new(0),
        preferred: true,
        reached,
        resume: resume.clone(),
    });
    let registry = ConnectionRegistry::new();
    assert!(registry.register_candidate(&peer(), &incumbent));
    let writer = {
        let registry = registry.clone();
        std::thread::spawn(move || {
            assert!(registry.register_candidate(&peer(), &candidate));
        })
    };
    receive.recv_timeout(Duration::from_secs(5)).unwrap();

    let (finished, cleanup_finished) = mpsc::channel();
    let reader = {
        let incumbent = incumbent.clone();
        std::thread::spawn(move || {
            assert!(!registry.activate(&peer(), &incumbent));
            assert!(!registry.remove_if_current(&peer(), &incumbent));
            incumbent.lifecycle.mark_closed();
            finished.send(()).unwrap();
        })
    };
    // Give cleanup the opportunity to run. Correct arbitration holds the map
    // lock until the verdict is set, so cleanup must wait for the writer.
    let _ = cleanup_finished.recv_timeout(Duration::from_secs(1));
    resume.wait();
    writer.join().unwrap();
    reader.join().unwrap();

    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    assert_eq!(
        runtime.block_on(incumbent.lifecycle.wait_for_resolution()),
        ConnectionResolution::Superseded,
        "a waiting send must retry the replacement instead of dropping its payload"
    );
}
