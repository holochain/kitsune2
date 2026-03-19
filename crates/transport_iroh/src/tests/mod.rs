//! iroh transport module tests

use super::*;

mod frame;
mod stream;
mod url;

#[test]
fn validate_bad_relay_url() {
    let builder = Builder {
        transport: IrohTransportFactory::create(),
        ..kitsune2_core::default_test_builder()
    };

    builder
        .config
        .set_module_config(&IrohTransportModConfig {
            iroh_transport: IrohTransportConfig {
                relay_url: Some("<bad-url>".into()),
                ..Default::default()
            },
        })
        .unwrap();

    assert!(builder.validate_config().is_err());
}

#[test]
fn validate_allowed_plain_text_relay_url() {
    let builder = Builder {
        transport: IrohTransportFactory::create(),
        ..kitsune2_core::default_test_builder()
    };

    builder
        .config
        .set_module_config(&IrohTransportModConfig {
            iroh_transport: IrohTransportConfig {
                relay_url: Some("http://test.url".into()),
                relay_allow_plain_text: true,
                ..Default::default()
            },
        })
        .unwrap();

    let result = builder.validate_config();
    assert!(result.is_ok());
}

/// Test that `insert_relay` dynamically adds a relay to a transport
/// that was created without one.
#[cfg(feature = "test-utils")]
#[tokio::test(flavor = "multi_thread")]
async fn insert_relay_adds_relay_to_transport() {
    use crate::test_utils::MockTxHandler;
    use kitsune2_test_utils::bootstrap::TestBootstrapSrv;
    use std::sync::Arc;

    kitsune2_test_utils::enable_tracing();

    // Create a bootstrap server with relay support
    let bootstrap_server = TestBootstrapSrv::new(false).await;
    let relay_url = format!("{}/relay", bootstrap_server.addr());

    // Create a transport WITHOUT a relay configured
    let builder = Builder {
        transport: IrohTransportFactory::create(),
        ..kitsune2_core::default_test_builder()
    }
    .with_default_config()
    .unwrap();

    // No relay_url set in config - transport starts without relay
    builder
        .config
        .set_module_config(&IrohTransportModConfig {
            iroh_transport: IrohTransportConfig {
                relay_url: None,
                relay_allow_plain_text: true,
                ..Default::default()
            },
        })
        .unwrap();

    let builder = Arc::new(builder);

    // Track when a listening address is assigned
    let got_address = Arc::new(tokio::sync::Notify::new());
    let got_address_clone = got_address.clone();
    let handler: DynTxHandler = Arc::new(MockTxHandler {
        new_listening_address: Arc::new(move |_url| {
            got_address_clone.notify_one();
        }),
        ..MockTxHandler::default()
    });

    let tx = builder
        .transport
        .create(builder.clone(), handler)
        .await
        .unwrap();

    // Dynamically add the relay (no auth material needed for open relay)
    tx.insert_relay(relay_url, None).await.unwrap();

    // Wait for the transport to receive a relay-assigned address
    tokio::time::timeout(
        std::time::Duration::from_secs(10),
        got_address.notified(),
    )
    .await
    .expect("Transport should receive a listening address after insert_relay");
}

#[test]
fn validate_disallowed_plain_text_relay_url() {
    let builder = Builder {
        transport: IrohTransportFactory::create(),
        ..kitsune2_core::default_test_builder()
    };

    builder
        .config
        .set_module_config(&IrohTransportModConfig {
            iroh_transport: IrohTransportConfig {
                relay_url: Some("http://test.url".into()),
                relay_allow_plain_text: false,
                ..Default::default()
            },
        })
        .unwrap();

    let result = builder.validate_config();
    assert!(result.is_err());
    assert!(format!("{result:?}").contains("Disallowed plaintext relay URL"));
}
