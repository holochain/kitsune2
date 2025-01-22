//! Configuration parameters for the gossip module.

/// Configuration parameters for K2Gossip.
///
/// This will be set as a default by the [K2GossipFactory](crate::K2GossipFactory).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct K2GossipConfig {
    /// The maximum number of bytes of op data to request in a single gossip round.
    ///
    /// This applies to both "new ops" which is the incremental sync of newly created data, and
    /// to a DHT mismatch in existing data. The new ops are synced first, so the remaining capacity
    /// is used for the mismatch sync.
    ///
    /// The maximum size of an op is host dependant, but assuming a 1 MB limit, this would allow
    /// for at least 100 ops to be requested in a single round.
    ///
    /// Default: 100MB
    pub max_gossip_op_bytes: u32,

    /// The interval in seconds between initiating gossip rounds.
    ///
    /// This controls how often Kitsune will attempt to find a peer to gossip with.
    ///
    /// This can be set as low as you'd like, but you will still be limited by
    /// [K2GossipConfig::min_initiate_interval_ms]. So a low value for this will result in Kitsune
    /// doing its gossip initiation in a burst. Then, when it has run out of peers, it will idle
    /// for a while.
    ///
    /// Default: 120,000 (2m)
    pub initiate_interval_ms: u32,

    /// The minimum amount of time that must be allowed to pass before a gossip round can be
    /// initiated by a given peer.
    ///
    /// This is a rate-limiting mechanism to be enforced against incoming gossip and therefore must
    /// be respected when initiating too.
    ///
    /// Default: 300,000 (5m)
    pub min_initiate_interval_ms: u32,

    /// The timeout for a gossip round.
    ///
    /// This will be loosely enforced on both sides. Kitsune periodically checks for timed out
    /// rounds and will terminate them if they have been running for longer than this timeout.
    ///
    /// There is no point setting this lower than 5s because that is how often Kitsune checks for
    /// timed out rounds.
    ///
    /// Default: 60,000 (1m)
    pub gossip_timeout_ms: u32,
}

impl Default for K2GossipConfig {
    fn default() -> Self {
        Self {
            max_gossip_op_bytes: 100 * 1024 * 1024,
            initiate_interval_ms: 120_000,
            min_initiate_interval_ms: 300_000,
            gossip_timeout_ms: 60_000,
        }
    }
}

impl K2GossipConfig {
    /// The interval between initiating gossip rounds.
    pub(crate) fn initiate_interval(&self) -> std::time::Duration {
        std::time::Duration::from_millis(self.initiate_interval_ms as u64)
    }

    /// The minimum amount of time that must be allowed to pass before a gossip round can be
    /// initiated by a given peer.
    pub(crate) fn min_initiate_interval(&self) -> std::time::Duration {
        std::time::Duration::from_millis(self.min_initiate_interval_ms as u64)
    }

    /// The timeout for a gossip round.
    pub(crate) fn gossip_timeout(&self) -> std::time::Duration {
        std::time::Duration::from_millis(self.gossip_timeout_ms as u64)
    }
}

/// Module-level configuration for K2Gossip.
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct K2GossipModConfig {
    /// CoreBootstrap configuration.
    pub k2_gossip: K2GossipConfig,
}
