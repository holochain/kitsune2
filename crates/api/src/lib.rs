#![deny(missing_docs)]
//! Kitsune2 API contains kitsune module traits and the basic types required
//! to define the api of those traits.
//!
//! If you want to use Kitsune2 itself, please see the kitsune2 crate.

/// Boxed future type.
pub type BoxFut<'a, T> =
    std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send + 'a>>;

pub(crate) mod serde_bytes_base64 {
    pub fn serialize<S>(
        b: &bytes::Bytes,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use base64::prelude::*;
        serializer.serialize_str(&BASE64_URL_SAFE_NO_PAD.encode(b))
    }

    pub fn deserialize<'de, D, T: From<bytes::Bytes>>(
        deserializer: D,
    ) -> Result<T, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use base64::prelude::*;
        let s: &'de str = serde::Deserialize::deserialize(deserializer)?;
        BASE64_URL_SAFE_NO_PAD
            .decode(s)
            .map(|v| bytes::Bytes::copy_from_slice(&v).into())
            .map_err(serde::de::Error::custom)
    }
}

pub mod agent;
pub mod builder;
pub mod config;
pub mod peer_store;

mod error;
pub use error::*;

pub mod id;
pub use id::{AgentId, OpId, SpaceId};

mod timestamp;
pub use timestamp::*;
