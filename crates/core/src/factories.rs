//! Factories for generating instances of Kitsune2 modules.

mod core_kitsune;
pub use core_kitsune::*;

mod core_space;
pub use core_space::*;

pub mod mem_peer_store;
pub use mem_peer_store::MemPeerStoreFactory;

#[cfg(test)]
pub(crate) use mem_peer_store::test_utils;

pub mod mem_bootstrap;
pub use mem_bootstrap::MemBootstrapFactory;

pub mod core_bootstrap;
pub use core_bootstrap::CoreBootstrapFactory;

pub mod core_fetch;
pub use core_fetch::CoreFetchFactory;

mod mem_transport;
pub use mem_transport::*;
