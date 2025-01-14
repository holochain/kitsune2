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
    /// The maximum size of an op is host dependant, but assuming a 1MB limit, this would allow
    /// for at least 100 ops to be requested in a single round.
    ///
    /// Default: 100MB
    pub max_gossip_op_bytes: u32,
}

impl Default for K2GossipConfig {
    fn default() -> Self {
        Self {
            max_gossip_op_bytes: 100 * 1024 * 1024,
        }
    }
}

/// Module-level configuration for K2Gossip.
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct K2GossipModConfig {
    /// CoreBootstrap configuration.
    pub k2_gossip: K2GossipConfig,
}
