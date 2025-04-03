#![deny(missing_docs)]

//! Kitsune2's gossip module.

mod config;
pub use config::*;

mod constant;
pub use constant::*;

mod gossip;
pub use gossip::*;

mod error;
mod initiate;

mod peer_meta_store;
pub use peer_meta_store::K2PeerMetaStore;

mod protocol;
mod respond;
mod state;
mod storage_arc;
mod summary;
mod timeout;
mod update;

#[cfg(any(test, feature = "test-utils"))]
pub mod harness;
