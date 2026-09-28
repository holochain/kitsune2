//! Factories for generating instances of Kitsune2 modules.
//!
//! Documentation for individual core modules can be found in [this crate's doc module](super::doc).

async fn wait_for_transport_url_available(
    transport_url_available: &mut tokio::sync::watch::Receiver<bool>,
) -> bool {
    transport_url_available
        .wait_for(|is_available| *is_available)
        .await
        .is_ok()
}

async fn wait_for_transport_url_recovery(
    transport_url_available: &mut tokio::sync::watch::Receiver<bool>,
) -> bool {
    let (state_changed, is_available) = {
        let current = transport_url_available.borrow_and_update();
        (current.has_changed(), *current)
    };
    if is_available
        && !state_changed
        && transport_url_available.changed().await.is_err()
    {
        return false;
    }
    wait_for_transport_url_available(transport_url_available).await
}

mod core_kitsune;
pub use core_kitsune::*;

mod core_space;
pub use core_space::*;

mod mem_peer_store;
pub use mem_peer_store::*;

mod mem_bootstrap;
pub use mem_bootstrap::*;

mod core_local_agent_store;
pub use core_local_agent_store::*;

mod core_bootstrap;
pub use core_bootstrap::*;

mod mem_peer_meta_store;
pub use mem_peer_meta_store::*;

mod core_fetch;
pub use core_fetch::*;

mod core_report;
pub use core_report::*;

mod core_gossip;
pub use core_gossip::*;

mod core_publish;
pub use core_publish::*;

mod mem_transport;
pub use mem_transport::*;

mod mem_op_store;
pub use mem_op_store::*;

mod mem_blocks;
pub use mem_blocks::*;

mod core_known_peers;
pub use core_known_peers::*;

mod core_access;
pub use core_access::*;
