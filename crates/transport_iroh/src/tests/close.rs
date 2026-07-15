//! Unit tests for graceful connection close (issue #496).
//!
//! A graceful close is signalled with [`CloseCode::Graceful`] and a
//! human-readable reason in the QUIC application close frame. The receiving
//! side must release the connection quietly — informing handlers via
//! `peer_disconnect(url, Some(reason))` — without marking the peer
//! unresponsive.

use super::support::{build_recording_handler, spawn_active_reader};
use crate::close_code::CloseCode;
use crate::connection_context::{ReaderCleanup, classify_exit};
use bytes::Bytes;
use kitsune2_test_utils::retry_fn_until_timeout;
use std::sync::atomic::Ordering;
use std::time::Duration;

#[test]
fn classify_not_active_is_quiet() {
    assert_eq!(classify_exit(false, None, true), ReaderCleanup::Quiet);
    assert_eq!(
        classify_exit(
            false,
            Some((CloseCode::Graceful, Bytes::from_static(b"bye"))),
            true
        ),
        ReaderCleanup::Quiet,
        "a non-active connection is torn down quietly even on graceful close"
    );
}

#[test]
fn classify_superseded_code_is_quiet() {
    assert_eq!(
        classify_exit(
            true,
            Some((CloseCode::Superseded, Bytes::from_static(b"whatever"))),
            true
        ),
        ReaderCleanup::Quiet
    );
}

#[test]
fn classify_graceful_close_carries_reason() {
    assert_eq!(
        classify_exit(
            true,
            Some((CloseCode::Graceful, Bytes::from_static(b"space closed"))),
            true
        ),
        ReaderCleanup::Graceful {
            reason: "space closed".to_string()
        }
    );
}

#[test]
fn classify_genuine_death_marks_unresponsive() {
    assert_eq!(
        classify_exit(true, None, true),
        ReaderCleanup::PeerGone {
            mark_unresponsive: true
        }
    );
}

#[test]
fn classify_temporary_error_skips_unresponsive() {
    assert_eq!(
        classify_exit(true, None, false),
        ReaderCleanup::PeerGone {
            mark_unresponsive: false
        }
    );
}

/// A remote graceful close must fire `peer_disconnect` with the remote's
/// reason and must NOT mark the peer unresponsive.
#[tokio::test(flavor = "multi_thread")]
async fn graceful_remote_close_releases_quietly_with_reason() {
    let recorder = build_recording_handler();

    let reader = spawn_active_reader(
        recorder.handler.clone(),
        Some((CloseCode::Graceful, Bytes::from_static(b"space closed"))),
    );
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
        0,
        "a graceful remote close must not mark the peer unresponsive"
    );
    let disconnects = recorder.disconnects.lock().unwrap();
    assert!(
        disconnects
            .iter()
            .all(|reason| reason.as_deref() == Some("space closed")),
        "peer_disconnect must carry the remote's close reason, got {disconnects:?}"
    );
}

/// A local `disconnect` must close the underlying connection with the
/// `Graceful` code and the caller's reason bytes, and inform local handlers
/// via `peer_disconnect` with the same reason.
#[tokio::test(flavor = "multi_thread")]
async fn local_disconnect_closes_with_graceful_code_and_reason() {
    let recorder = build_recording_handler();

    // Reader stays parked on the accept gate; only the explicit disconnect
    // call below is exercised.
    let reader = spawn_active_reader(recorder.handler.clone(), None);

    reader
        .ctx
        .disconnect(CloseCode::Graceful, "bye".to_string());

    assert_eq!(
        reader.close_calls.lock().unwrap().as_slice(),
        &[(CloseCode::Graceful, b"bye".to_vec())],
        "the connection must be closed with the Graceful code and reason"
    );
    let disconnects = recorder.disconnects.lock().unwrap();
    assert!(
        !disconnects.is_empty()
            && disconnects
                .iter()
                .all(|reason| reason.as_deref() == Some("bye")),
        "local handlers must get peer_disconnect with the reason, got {disconnects:?}"
    );
    assert_eq!(recorder.unresponsive_calls.load(Ordering::SeqCst), 0);
}

/// End-to-end: a graceful `Transport::disconnect` on one peer must inform
/// the other peer of the reason and must not get it marked unresponsive.
#[cfg(feature = "test-utils")]
#[tokio::test(flavor = "multi_thread")]
async fn graceful_disconnect_informs_remote_end_to_end() {
    use crate::test_utils::{IrohTransportTestHarness, MockTxHandler};
    use kitsune2_api::DynTxHandler;
    use kitsune2_test_utils::space::TEST_SPACE_ID;
    use std::sync::atomic::AtomicUsize;
    use std::sync::{Arc, Mutex};

    kitsune2_test_utils::enable_tracing();

    let harness = IrohTransportTestHarness::new().await;

    // Peer B records peer_disconnect reasons and set_unresponsive calls.
    let b_disconnects: Arc<Mutex<Vec<Option<String>>>> =
        Arc::new(Mutex::new(Vec::new()));
    let b_unresponsive = Arc::new(AtomicUsize::new(0));
    let b_got_notify = Arc::new(tokio::sync::Notify::new());
    let mock_b = {
        let disconnects = b_disconnects.clone();
        let unresponsive = b_unresponsive.clone();
        let got_notify = b_got_notify.clone();
        Arc::new(MockTxHandler {
            peer_disconnect: Arc::new(move |_peer, reason| {
                disconnects.lock().unwrap().push(reason);
            }),
            set_unresponsive: Arc::new(move |_peer, _ts| {
                unresponsive.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }),
            recv_space_notify: Arc::new(move |_peer, _space, _data| {
                got_notify.notify_one();
                Ok(())
            }),
            ..MockTxHandler::default()
        })
    };
    // `as` cannot unsize-coerce an Arc; a typed binding performs the
    // Arc<MockTxHandler> -> Arc<dyn TxHandler> coercion.
    let handler_b: DynTxHandler = mock_b.clone();
    let transport_b = harness.build_transport(handler_b).await;
    transport_b.register_space_handler(TEST_SPACE_ID, mock_b.clone());

    let mock_a = Arc::new(MockTxHandler::default());
    let handler_a: DynTxHandler = mock_a.clone();
    let transport_a = harness.build_transport(handler_a).await;
    transport_a.register_space_handler(TEST_SPACE_ID, mock_a.clone());

    // Wait until B has a real listening address (new_listening_address
    // updates MockTxHandler::current_url).
    let initial_url = crate::test_utils::dummy_url();
    retry_fn_until_timeout(
        || async { *mock_b.current_url.lock().unwrap() != initial_url },
        Some(30_000),
        Some(10),
    )
    .await
    .expect("condition not met within timeout");
    let url_b = mock_b.current_url.lock().unwrap().clone();

    // Establish a connection A -> B with a space notify and wait for B to
    // receive it.
    transport_a
        .send_space_notify(
            url_b.clone(),
            TEST_SPACE_ID,
            Bytes::from_static(b"hello"),
        )
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(30), b_got_notify.notified())
        .await
        .expect("B did not receive the space notify");

    // A disconnects gracefully with a reason.
    transport_a
        .disconnect(url_b, Some("test disconnect".to_string()))
        .await;

    // B must observe the disconnect with A's reason...
    retry_fn_until_timeout(
        || async { !b_disconnects.lock().unwrap().is_empty() },
        Some(30_000),
        Some(10),
    )
    .await
    .expect("condition not met within timeout");
    let disconnects = b_disconnects.lock().unwrap();
    assert!(
        disconnects
            .iter()
            .all(|reason| reason.as_deref() == Some("test disconnect")),
        "B must get the graceful close reason, got {disconnects:?}"
    );

    // ...and must NOT have marked A unresponsive.
    assert_eq!(
        b_unresponsive.load(Ordering::SeqCst),
        0,
        "a graceful disconnect must not mark the peer unresponsive"
    );
}
