#![deny(missing_docs)]
//! Kitsune2 p2p / dht communication framework.

use kitsune2_api::{builder::Builder, config::Config};

/// Construct a production-ready default builder.
///
/// - `peer_store` - The default peer store is [factories::MemPeerStoreFactory].
pub fn default_builder() -> Builder {
    Builder {
        config: Config::default(),
        peer_store: factories::MemPeerStoreFactory::create(),
    }
}

pub mod factories;
