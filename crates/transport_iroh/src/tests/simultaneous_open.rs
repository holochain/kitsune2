//! Unit tests for the connection-reader exit cleanup, which decides whether a
//! reader loop ending means "the peer is gone" (mark unresponsive + fire
//! `peer_disconnect`) or "this connection was deliberately superseded" (close
//! quietly).
//!
//! Regression context: simultaneous-open resolution produces two connections
//! for a pair and tears the losing one down. The teardown of the loser is
//! driven by the *remote* peer, so on the node whose losing connection is
//! still its active map entry — because the winning connection has not yet
//! finished its preflight and registered — that remote close must not be
//! mistaken for a genuine peer disconnect. Doing so spuriously marks the peer
//! unresponsive (which lasts until the agent info expires) and stalls gossip.

use super::support::{
    build_parked_context, build_parked_context_with_remote_close,
    build_recording_handler, remote_url, spawn_active_reader,
};
use crate::Connections;
use crate::close_code::CloseCode;
use crate::connection_context::SUPERSEDED_CLOSE_REASON;
use crate::connection_registry::RegistryEntry;
use bytes::Bytes;
use kitsune2_test_utils::retry_fn_until_timeout;
use std::sync::Arc;
use std::sync::atomic::Ordering;

/// When the active connection is closed by the remote with the superseded
/// close code — i.e. the remote preferred a different connection during
/// simultaneous-open resolution — the reader exit must NOT mark the peer
/// unresponsive or fire `peer_disconnect`.
#[tokio::test(flavor = "multi_thread")]
async fn superseded_remote_close_does_not_mark_unresponsive() {
    let recorder = build_recording_handler();

    let reader = spawn_active_reader(
        recorder.handler.clone(),
        Some((
            CloseCode::Superseded,
            Bytes::from_static(SUPERSEDED_CLOSE_REASON),
        )),
    );
    reader.accept_gate.notify_one();

    // The exit cleanup finishes by closing the connection.
    retry_fn_until_timeout(
        || async { !reader.close_calls.lock().unwrap().is_empty() },
        Some(5_000),
        Some(10),
    )
    .await
    .expect("condition not met within timeout");

    assert_eq!(
        recorder.unresponsive_calls.load(Ordering::SeqCst),
        0,
        "a connection superseded by the remote must not mark the peer unresponsive"
    );
    assert!(
        recorder.disconnects.lock().unwrap().is_empty(),
        "a connection superseded by the remote must not fire peer_disconnect"
    );
}

/// When the active connection dies without a graceful or superseded close
/// (a genuine peer disconnect), the reader exit must still mark the peer
/// unresponsive and fire `peer_disconnect`. Guards against over-suppressing
/// real disconnects.
#[tokio::test(flavor = "multi_thread")]
async fn genuine_remote_close_marks_unresponsive() {
    let recorder = build_recording_handler();

    let reader = spawn_active_reader(recorder.handler.clone(), None);
    reader.accept_gate.notify_one();

    retry_fn_until_timeout(
        || async { !recorder.disconnects.lock().unwrap().is_empty() },
        Some(5_000),
        Some(10),
    )
    .await
    .expect("condition not met within timeout");

    assert_eq!(
        recorder.unresponsive_calls.load(Ordering::SeqCst),
        1,
        "a genuine peer disconnect must mark the peer unresponsive"
    );
}

/// A remote that supersedes a connection says so only in its close code. The
/// local lifecycle catches up when this connection's reader runs its exit
/// cleanup, which races a send already writing to the connection: when the
/// reader loses that race, a send asking the lifecycle alone would report a
/// spurious error for a message the winning connection can still carry. So the
/// close code must be enough on its own.
#[tokio::test(flavor = "multi_thread")]
async fn a_remote_supersede_is_visible_before_the_reader_reacts() {
    let recorder = build_recording_handler();

    let ctx = build_parked_context_with_remote_close(
        recorder.handler.clone(),
        Connections::new(),
        true,
        [0xff; 32],
        Some((
            CloseCode::Superseded,
            Bytes::from_static(SUPERSEDED_CLOSE_REASON),
        )),
    );

    assert!(
        ctx.lifecycle().is_live(),
        "the reader is parked, so the lifecycle has not reacted to the close"
    );
    assert!(
        ctx.is_superseded(),
        "a remote supersede must be visible to a send that is already writing"
    );
}

/// The close code must not be read so broadly that an ordinary remote close
/// looks like a supersede, which would retry a send that should have failed.
#[tokio::test(flavor = "multi_thread")]
async fn an_ordinary_remote_close_is_not_a_supersede() {
    let recorder = build_recording_handler();

    let ctx = build_parked_context_with_remote_close(
        recorder.handler.clone(),
        Connections::new(),
        true,
        [0xff; 32],
        Some((CloseCode::Graceful, Bytes::from_static(b"goodbye"))),
    );

    assert!(!ctx.is_superseded());
}

/// A reconnect arrives while the previous connection to the same peer is still
/// the map entry, because its reader has not run its exit cleanup yet. Both
/// connections were *accepted*, so neither is the preferred one. The tie-break
/// must not defend the older connection: the newcomer takes the slot.
///
/// Regression: rejecting the newcomer here made a send immediately after a
/// disconnect close the fresh connection with `CloseCode::Superseded`, and the
/// message was lost.
#[tokio::test(flavor = "multi_thread")]
async fn newcomer_replaces_a_same_direction_incumbent() {
    let recorder = build_recording_handler();
    let connections: Connections = Connections::new();
    let url = remote_url();

    // `[0xff; 32]` > the `0xaa` remote id, so the preferred connection is the
    // one *we* dialed. Both of these were accepted, so neither is preferred.
    let incumbent = build_parked_context(
        recorder.handler.clone(),
        connections.clone(),
        false,
        [0xff; 32],
    );
    assert!(connections.register_candidate(&url, &incumbent));

    let newcomer = build_parked_context(
        recorder.handler.clone(),
        connections.clone(),
        false,
        [0xff; 32],
    );
    assert!(
        connections.register_candidate(&url, &newcomer),
        "a same-direction newcomer must replace the stale incumbent"
    );

    assert!(
        Arc::ptr_eq(&connections.get(&url).unwrap(), &newcomer),
        "the newcomer must hold the slot"
    );
}

/// A genuine simultaneous open: the incumbent is the connection we dialed and
/// is the preferred one, the newcomer is the inbound duplicate. The newcomer
/// must lose and be marked superseded, leaving the incumbent in the slot.
#[tokio::test(flavor = "multi_thread")]
async fn preferred_incumbent_defeats_an_inbound_duplicate() {
    let recorder = build_recording_handler();
    let connections: Connections = Connections::new();
    let url = remote_url();

    let incumbent = build_parked_context(
        recorder.handler.clone(),
        connections.clone(),
        true,
        [0xff; 32],
    );
    assert!(connections.register_candidate(&url, &incumbent));

    let newcomer = build_parked_context(
        recorder.handler.clone(),
        connections.clone(),
        false,
        [0xff; 32],
    );
    assert!(
        !connections.register_candidate(&url, &newcomer),
        "the inbound duplicate must lose to the preferred incumbent"
    );
    assert!(
        !newcomer.lifecycle().is_live(),
        "the losing newcomer must be marked superseded"
    );

    assert!(
        Arc::ptr_eq(&connections.get(&url).unwrap(), &incumbent),
        "the preferred incumbent must keep the slot"
    );
}

/// An incumbent that has already reached a terminal state is a corpse waiting
/// for its reader to be reaped. Even when it is the preferred direction, it
/// must not block a live newcomer.
#[tokio::test(flavor = "multi_thread")]
async fn terminal_incumbent_does_not_block_a_newcomer() {
    let recorder = build_recording_handler();
    let connections: Connections = Connections::new();
    let url = remote_url();

    let incumbent = build_parked_context(
        recorder.handler.clone(),
        connections.clone(),
        true,
        [0xff; 32],
    );
    assert!(connections.register_candidate(&url, &incumbent));
    incumbent.mark_closed();

    let newcomer = build_parked_context(
        recorder.handler.clone(),
        connections.clone(),
        false,
        [0xff; 32],
    );
    assert!(
        connections.register_candidate(&url, &newcomer),
        "a terminal incumbent must be evicted, not defended"
    );

    assert!(Arc::ptr_eq(&connections.get(&url).unwrap(), &newcomer));
}
