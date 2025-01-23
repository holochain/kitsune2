#![deny(missing_docs)]
//! Test utilities to help with testing Kitsune2.

use rand::RngCore;

pub mod agent;
pub mod id;
pub mod space;

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

/// Repeat a code block after a pause until a timeout has elapsed.
/// The default timeout is 100 ms.
#[macro_export]
macro_rules! iter_check {
    ($millis:literal, $code:block) => {
        tokio::time::timeout(
            std::time::Duration::from_millis($millis),
            async {
                loop {
                    tokio::time::sleep(std::time::Duration::from_millis(1))
                        .await;
                    $code
                }
            },
        )
        .await
        .unwrap();
    };

    ($code:block) => {
        iter_check!(100, $code)
    };
}
