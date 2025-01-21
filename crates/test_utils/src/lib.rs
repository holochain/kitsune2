#![deny(missing_docs)]
//! Test utilities to help with testing Kitsune2.

use rand::RngCore;

/// Enable tracing with the RUST_LOG environment variable.
///
/// This is intended to be used in tests, so it defaults to DEBUG level.
pub fn enable_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_test_writer()
        .with_env_filter(
            tracing_subscriber::EnvFilter::builder()
                .with_default_directive(tracing::Level::DEBUG.into())
                .from_env_lossy(),
        )
        .try_init();
}

/// Create random bytes of a specified length.
pub fn random_bytes(length: u16) -> Vec<u8> {
    let mut rng = rand::thread_rng();
    let mut bytes = vec![0; length as usize];
    rng.fill_bytes(&mut bytes);
    bytes
}

pub mod agent;
pub mod id;
pub mod space;
