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

mod agent;
pub use agent::*;

mod arc;
pub use arc::*;

mod bootstrap;
pub use bootstrap::*;

mod builder;
pub use builder::*;

mod config;
pub use config::*;

mod kitsune;
pub use kitsune::*;

mod peer_store;
pub use peer_store::*;

mod space;
pub use space::*;

mod transport;
pub use transport::*;

mod error;
pub use error::*;

mod id;
pub use id::*;

mod timestamp;
pub use timestamp::*;

mod fetch;
pub use fetch::*;

mod peer_meta_store;
pub use peer_meta_store::*;

mod gossip;
pub use gossip::*;

mod local_agent_store;
pub use local_agent_store::*;

mod op_store;
pub use op_store::*;

mod protocol;
pub use protocol::*;

mod publish;
pub use publish::*;

mod url;
pub use url::*;

#[cfg(any(doc, docsrs))]
pub mod doc;
