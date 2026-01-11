//! I/O abstractions for stream operations, enabling unit testing.

use kitsune2_api::{BoxFut, K2Error, K2Result};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Abstraction for send stream operations.
///
/// This trait allows injecting mock implementations for testing
/// without requiring async trait syntax.
pub(crate) trait SendStreamTrait: Send + Sync {
    /// Write all data to the stream.
    fn write_all<'a>(&'a self, data: &'a [u8]) -> BoxFut<'a, K2Result<()>>;
}

/// Production implementation wrapping iroh's SendStream.
pub(crate) struct IrohSendStream {
    inner: Mutex<iroh::endpoint::SendStream>,
}

impl IrohSendStream {
    /// Create a new wrapper around an iroh SendStream.
    pub fn new(stream: iroh::endpoint::SendStream) -> Self {
        Self {
            inner: Mutex::new(stream),
        }
    }
}

impl SendStreamTrait for IrohSendStream {
    fn write_all<'a>(&'a self, data: &'a [u8]) -> BoxFut<'a, K2Result<()>> {
        Box::pin(async move {
            self.inner
                .lock()
                .await
                .write_all(data)
                .await
                .map_err(|err| K2Error::other_src("stream write failed", err))
        })
    }
}

pub(crate) type DynIrohSendStream = Arc<dyn SendStreamTrait>;

#[cfg(test)]
pub(crate) mod mock {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// Mock send stream for testing.
    ///
    /// Captures written data and can be configured to fail.
    #[derive(Clone)]
    pub struct MockSendStream {
        pub written_data: Arc<Mutex<Vec<Vec<u8>>>>,
        pub should_fail: bool,
    }

    impl MockSendStream {
        pub fn new() -> Self {
            Self {
                written_data: Arc::new(Mutex::new(Vec::new())),
                should_fail: false,
            }
        }

        pub fn with_failure() -> Self {
            Self {
                written_data: Arc::new(Mutex::new(Vec::new())),
                should_fail: true,
            }
        }

        pub fn get_written_data(&self) -> Vec<Vec<u8>> {
            self.written_data.lock().unwrap().clone()
        }
    }

    impl SendStreamTrait for MockSendStream {
        fn write_all<'a>(&'a self, data: &'a [u8]) -> BoxFut<'a, K2Result<()>> {
            let should_fail = self.should_fail;
            let written_data = self.written_data.clone();

            Box::pin(async move {
                if should_fail {
                    Err(K2Error::other("mock stream write failure"))
                } else {
                    written_data.lock().unwrap().push(data.to_vec());
                    Ok(())
                }
            })
        }
    }
}
