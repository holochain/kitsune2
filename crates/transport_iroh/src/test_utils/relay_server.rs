//! Test utilities associated with iroh relay servers.

use iroh::RelayUrl;
use iroh::test_utils::run_relay_server;
use iroh_relay::RelayMap;

pub use iroh_relay::server::Server;

/// Spawn an iroh relay server at 127.0.0.1.
pub async fn spawn_iroh_relay_server() -> (RelayMap, RelayUrl, Server) {
    run_relay_server().await.unwrap()
}
