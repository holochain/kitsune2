//! Iroh relay integration for kitsune2 bootstrap server.
//!
//! This module provides axum integration for the iroh relay server, allowing
//! it to be embedded as a standard route in the bootstrap server.
//!
//! # Rate Limiting
//!
//! Connection-level rate limiting is implemented using the `governor` crate with
//! a token bucket algorithm. The rate limiter is applied as tower middleware on
//! the relay routes.
//!
//! - `accept_conn_limit`: Maximum connections per second (sustained rate)
//! - `accept_conn_burst`: Maximum burst size (token bucket capacity)
//!
//! When rate limits are exceeded, clients receive a 429 Too Many Requests response.
//!
//! Note: Byte-level rate limiting (`client_rx`) is not supported in the axum
//! integration since WebSocket connections are already established when they
//! reach the handler.

// Re-export the public API from iroh_relay_axum
pub use crate::iroh_relay_axum::{
    create_relay_state, relay_handler, relay_probe_handler,
    ConnectionRateLimiter, Limits, RateLimitLayer, RateLimitService,
    RelayConfig, RelayState,
};
pub use iroh_relay::server::ClientRateLimit;
