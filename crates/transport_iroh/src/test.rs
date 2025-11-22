//! tx5 transport module tests

use super::*;
use iroh_relay::server::{AccessConfig, Limits, RelayConfig, ServerConfig};
use kitsune2_test_utils::space::TEST_SPACE_ID;
use std::{
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    time::Duration,
};

// Here we should only test the kitsune2 integration of iroh.
//
// Specifically:
//
// - That new_listening_address is called if the sbd server is restarted
// - That peer connect / disconnect are invoked appropriately.
// - That messages can be sent / received.
// - That preflight generation and checking work, which are a little weird
//   because in kitsune2 the check logic is handled in the same recv_data
//   callback, where iroh handles it as a special callback.

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
fn validate_plain_relay_url() {
    let builder = Builder {
        transport: IrohTransportFactory::create(),
        ..kitsune2_core::default_test_builder()
    };

    builder
        .config
        .set_module_config(&IrohTransportModConfig {
            iroh_transport: IrohTransportConfig {
                relay_url: Some("ws://test.url".into()),
                ..Default::default()
            },
        })
        .unwrap();

    let result = format!("{:?}", builder.validate_config());
    assert!(result.contains("disallowed plaintext relay url"));
}

struct MockTxHandler {
    /// Mock function to implement the new_listening_address() method of the
    /// TxBaseHandler trait
    pub current_url: Arc<Mutex<Url>>,
}

impl std::fmt::Debug for MockTxHandler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MockTxHandler").finish()
    }
}

impl TxBaseHandler for MockTxHandler {
    fn new_listening_address(&self, this_url: Url) -> BoxFut<'static, ()> {
        *self.current_url.lock().unwrap() = this_url;
        Box::pin(async {})
    }
    fn peer_connect(&self, peer: Url) -> K2Result<()> {
        println!("connecting to peer {peer:?}");
        Ok(())
    }
    fn peer_disconnect(&self, _peer: Url, _reason: Option<String>) {}
}

impl TxHandler for MockTxHandler {
    fn preflight_gather_outgoing(
        &self,
        _peer_url: Url,
    ) -> BoxFut<'_, K2Result<bytes::Bytes>> {
        Box::pin(async { Ok(Bytes::new()) })
    }
    fn preflight_validate_incoming(
        &self,
        _peer_url: Url,
        _data: bytes::Bytes,
    ) -> BoxFut<'_, K2Result<()>> {
        Box::pin(async { Ok(()) })
    }
}

#[tokio::test]
async fn connect_two_endpoints() {
    let relay_server =
        iroh_relay::server::Server::spawn::<(), ()>(ServerConfig {
            metrics_addr: None,
            quic: None,
            relay: Some(RelayConfig {
                http_bind_addr: SocketAddr::V4(SocketAddrV4::new(
                    Ipv4Addr::new(127, 0, 0, 1),
                    30000,
                )),
                tls: None,
                limits: Limits::default(),
                key_cache_capacity: None,
                access: AccessConfig::Everyone,
            }),
        })
        .await
        .unwrap();
    let relay_url = relay_server.http_addr().unwrap();
    println!("relay url {relay_url:?}");

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
                relay_url: Some(format!("http://{relay_url}")),
                relay_allow_plain_text: true,
            },
        })
        .unwrap();
    let builder = Arc::new(builder);

    let h1 = Arc::new(MockTxHandler {
        current_url: Arc::new(Mutex::new(
            Url::from_str("ws://url.not.set:0/0").unwrap(),
        )),
    });
    let ep1 = builder
        .transport
        .create(builder.clone(), h1.clone())
        .await
        .unwrap();

    let connected_peers = ep1.get_connected_peers().await.unwrap();
    println!("connected peers {connected_peers:?}");

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
                relay_url: Some(format!("http://{relay_url}")),
                relay_allow_plain_text: true,
            },
        })
        .unwrap();
    let builder = Arc::new(builder);

    let h2 = Arc::new(MockTxHandler {
        current_url: Arc::new(Mutex::new(
            Url::from_str("ws://url.not.set:0/0").unwrap(),
        )),
    });
    let _ep2 = builder
        .transport
        .create(builder.clone(), h2.clone())
        .await
        .unwrap();

    // Wait for URLs to be updated
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let ep1_url = h1.current_url.lock().unwrap().clone();
            let ep2_url = h2.current_url.lock().unwrap().clone();
            if ep1_url != Url::from_str("ws://url.not.set:0/0").unwrap()
                && ep2_url != Url::from_str("ws://url.not.set:0/0").unwrap()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("URLs not updated within 10 seconds");

    println!("ep1 current url {:?}", h1.current_url.lock().unwrap());
    println!("ep2 current url {:?}", h2.current_url.lock().unwrap());

    let sent = ep1
        .send_space_notify(
            h2.current_url.lock().unwrap().clone(),
            TEST_SPACE_ID,
            Bytes::from_static(b"hello"),
        )
        .await
        .unwrap();
    println!("sent {sent:?}");
}
