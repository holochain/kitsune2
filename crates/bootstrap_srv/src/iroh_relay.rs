//! Iroh relay integration for kitsune2 bootstrap server.
//!
//! This module provides axum integration for the iroh relay server, allowing
//! it to be embedded as a standard route in the bootstrap server.

// Re-export the public API from iroh_relay_axum
pub use crate::iroh_relay_axum::{
    RelayState, create_relay_state, relay_handler, relay_probe_handler,
};
