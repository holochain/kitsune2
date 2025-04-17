#![deny(missing_docs)]
//! Test utilities to help with testing Kitsune2.

use rand::RngCore;

pub mod agent;
pub mod bootstrap;
pub mod id;
pub mod noop_bootstrap;
pub mod space;

/// Enable tracing with the RUST_LOG environment variable.
///
/// This is intended to be used in tests, so it defaults to DEBUG level.
pub fn enable_tracing() {
    enable_tracing_with_default_level(tracing::Level::DEBUG);
}

/// Enable tracing with the RUST_LOG environment variable.
pub fn enable_tracing_with_default_level(level: tracing::Level) {
    let _ = tracing_subscriber::fmt()
        .with_test_writer()
        .with_env_filter(
            tracing_subscriber::EnvFilter::builder()
                .with_default_directive(level.into())
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

/// Repeat a code block after a pause until a timeout has elapsed.
/// The default timeout is 100 ms.
#[macro_export]
macro_rules! iter_check {
    ($timeout_ms:literal, $sleep_ms:literal, $code:block) => {
        tokio::time::timeout(
            std::time::Duration::from_millis($timeout_ms),
            async {
                loop {
                    tokio::time::sleep(std::time::Duration::from_millis(
                        $sleep_ms,
                    ))
                    .await;
                    $code
                }
            },
        )
        .await
        .unwrap();
    };

    ($timeout_ms:literal, $code:block) => {
        iter_check!($timeout_ms, 1, $code)
    };

    ($code:block) => {
        iter_check!(100, $code)
    };
}
