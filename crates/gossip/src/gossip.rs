use kitsune2_api::peer_store::DynPeerStore;
use kitsune2_api::transport::DynTransport;
use kitsune2_api::{DynGossip, Gossip};
use std::sync::Arc;

#[derive(Debug)]
pub struct K2Gossip {
    peer_store: DynPeerStore,
    transport: DynTransport,
}

impl K2Gossip {
    pub fn create(
        peer_store: DynPeerStore,
        transport: DynTransport,
    ) -> DynGossip {
        Arc::new(K2Gossip {
            peer_store,
            transport,
        })
    }
}

impl Gossip for K2Gossip {}

#[cfg(test)]
mod test {}
