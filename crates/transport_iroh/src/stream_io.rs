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

/// Abstraction for receive stream operations.
///
/// This trait allows injecting mock implementations for testing
/// without requiring async trait syntax.
pub(crate) trait RecvStreamTrait: Send + Sync {
    /// Read exact number of bytes into the buffer.
    fn read_exact<'a>(&'a self, buf: &'a mut [u8]) -> BoxFut<'a, K2Result<()>>;
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

/// Production implementation wrapping iroh's RecvStream.
pub(crate) struct IrohRecvStream {
    inner: Mutex<iroh::endpoint::RecvStream>,
}

impl IrohRecvStream {
    /// Create a new wrapper around an iroh RecvStream.
    pub fn new(stream: iroh::endpoint::RecvStream) -> Self {
        Self {
            inner: Mutex::new(stream),
        }
    }
}

impl RecvStreamTrait for IrohRecvStream {
    fn read_exact<'a>(&'a self, buf: &'a mut [u8]) -> BoxFut<'a, K2Result<()>> {
        Box::pin(async move {
            self.inner
                .lock()
                .await
                .read_exact(buf)
                .await
                .map_err(|err| K2Error::other_src("stream read failed", err))
        })
    }
}

pub(crate) type DynIrohSendStream = Arc<dyn SendStreamTrait>;
pub(crate) type DynIrohRecvStream = Arc<dyn RecvStreamTrait>;

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

    /// Mock receive stream for testing.
    ///
    /// Returns pre-configured data and can be configured to fail.
    #[derive(Clone)]
    pub struct MockRecvStream {
        pub data_to_read: Arc<Mutex<Vec<u8>>>,
        pub read_position: Arc<Mutex<usize>>,
        pub should_fail: bool,
    }

    impl MockRecvStream {
        pub fn new(data: Vec<u8>) -> Self {
            Self {
                data_to_read: Arc::new(Mutex::new(data)),
                read_position: Arc::new(Mutex::new(0)),
                should_fail: false,
            }
        }

        pub fn with_failure() -> Self {
            Self {
                data_to_read: Arc::new(Mutex::new(Vec::new())),
                read_position: Arc::new(Mutex::new(0)),
                should_fail: true,
            }
        }

        pub fn get_read_position(&self) -> usize {
            *self.read_position.lock().unwrap()
        }
    }

    impl RecvStreamTrait for MockRecvStream {
        fn read_exact<'a>(&'a self, buf: &'a mut [u8]) -> BoxFut<'a, K2Result<()>> {
            let should_fail = self.should_fail;
            let data_to_read = self.data_to_read.clone();
            let read_position = self.read_position.clone();

            Box::pin(async move {
                if should_fail {
                    Err(K2Error::other("mock stream read failure"))
                } else {
                    let data = data_to_read.lock().unwrap();
                    let mut pos = read_position.lock().unwrap();

                    if *pos + buf.len() > data.len() {
                        return Err(K2Error::other("not enough data to read"));
                    }

                    buf.copy_from_slice(&data[*pos..*pos + buf.len()]);
                    *pos += buf.len();
                    Ok(())
                }
            })
        }
    }
}
