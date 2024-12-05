/// Enable tracing with the RUST_LOG environment variable.
///
/// This is intended to be used in tests, so it defaults to DEBUG level.
pub fn enable_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::builder()
                .with_default_directive(tracing::Level::DEBUG.into())
                .from_env_lossy(),
        )
        .try_init();
}
