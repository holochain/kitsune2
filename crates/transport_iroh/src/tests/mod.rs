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

/// Test that `configure_for_space` reads the per-space iroh transport
/// config and dynamically adds a relay to the transport.
#[cfg(feature = "test-utils")]
#[tokio::test(flavor = "multi_thread")]
async fn configure_for_space_adds_relay() {
    use crate::test_utils::MockTxHandler;
    use kitsune2_test_utils::bootstrap::TestBootstrapSrv;
    use std::sync::Arc;

    kitsune2_test_utils::enable_tracing();

    let bootstrap_server = TestBootstrapSrv::new(false).await;
    let relay_url = format!("{}/relay", bootstrap_server.addr());

    let builder = Builder {
        transport: IrohTransportFactory::create(),
        ..kitsune2_core::default_test_builder()
    }
    .with_default_config()
    .unwrap();

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

    // Build a per-space config containing the relay URL
    let per_space_config = Config::default();
    per_space_config
        .set_module_config(&IrohTransportPerSpaceModConfig {
            iroh_transport_per_space: IrohTransportPerSpaceConfig {
                relay_url: Some(relay_url),
            },
        })
        .unwrap();

    let space_id = SpaceId(Id(bytes::Bytes::from_static(b"test-space")));

    // configure_for_space should read the per-space config and insert the relay
    tx.configure_for_space(space_id, &per_space_config)
        .await
        .unwrap();

    // The transport should receive a relay-assigned listening address
    tokio::time::timeout(
        std::time::Duration::from_secs(10),
        got_address.notified(),
    )
    .await
    .expect(
        "Transport should receive a listening address after configure_for_space",
    );
}

/// Test that `configure_for_space` is a no-op when no per-space config
/// is provided.
#[cfg(feature = "test-utils")]
#[tokio::test(flavor = "multi_thread")]
async fn configure_for_space_noop_without_config() {
    use crate::test_utils::MockTxHandler;
    use std::sync::Arc;

    let builder = Builder {
        transport: IrohTransportFactory::create(),
        ..kitsune2_core::default_test_builder()
    }
    .with_default_config()
    .unwrap();

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
    let handler: DynTxHandler = Arc::new(MockTxHandler::default());

    let tx = builder
        .transport
        .create(builder.clone(), handler)
        .await
        .unwrap();

    // Empty config -- no per-space relay configured
    let empty_config = Config::default();
    let space_id = SpaceId(Id(bytes::Bytes::from_static(b"test-space")));

    // Should succeed as a no-op when no per-space relay is configured
    tx.configure_for_space(space_id, &empty_config)
        .await
        .unwrap();
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
