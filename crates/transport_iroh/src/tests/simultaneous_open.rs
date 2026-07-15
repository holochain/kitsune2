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

use super::support::{build_recording_handler, spawn_active_reader};
use crate::close_code::CloseCode;
use crate::connection_context::SUPERSEDED_CLOSE_REASON;
use bytes::Bytes;
use kitsune2_test_utils::retry_fn_until_timeout;
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
