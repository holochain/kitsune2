#![deny(missing_docs)]
//! Kitsune2 API contains kitsune module traits and the basic types required
//! to define the api of those traits.
//!
//! If you want to use Kitsune2 itself, please see the kitsune2 crate.

/// Boxed future type.
pub type BoxFut<'a, T> =
    std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send + 'a>>;

pub mod id;
pub use id::{AgentId, OpId, SpaceId};

mod timestamp;
pub use timestamp::*;

pub mod agent;
