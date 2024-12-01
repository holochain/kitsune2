//! The fetch queue is a helper module for fetching a set of kitsune ops from a peer.
//!
//! When a peer initiates a gossip session with another peer, it compares the ops
//! held by both and determines which ops it is missing. These ops must then be
//! fetched using the fetch queue.
//!
//! Gossip sessions are performed one after the other. Every session can have a set
//! of missing ops as an outcome.
//!
//! The fetch queue holds sets of ops and sources where to fetch ops from. The actual
//! fetching is executed by the fetch task.

pub mod fetch_queue;
