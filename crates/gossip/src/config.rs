//! Configuration parameters for the gossip module.

/// Configuration parameters for K2Gossip.
///
/// This will be set as a default by the [K2GossipFactory](crate::K2GossipFactory).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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
    #[cfg_attr(feature = "schema", schemars(default))]
    pub max_gossip_op_bytes: u32,

    /// The maximum value that this instance will accept for the `max_gossip_op_bytes` parameter
    /// from a remote peer.
    ///
    /// This prevents the remote peer from setting a high value for `max_gossip_op_bytes` and
    /// requesting all their op data from us in one batch. With this value set, we can limit how
    /// fast they'll be able to discover ops from us because it'll require multiple rounds to
    /// discover all the op ids they'd need to request from us.
    ///
    /// Default: 100MB
    #[cfg_attr(feature = "schema", schemars(default))]
    pub max_request_gossip_op_bytes: u32,

    /// The initial interval in milliseconds between initiating gossip rounds.
    ///
    /// This controls how often Kitsune will check for a peer to gossip with for its first gossip
    /// round. Once a gossip round has been successfully initiated, this interval will no longer be
    /// used. The value is used as an upper bound for an exponential backoff. The backoff runs from
    /// 10ms, with a factor of 1.2, up to this value. Once the maximum value is reached, it
    /// continues to be used as the upper bound for the backoff.
    ///
    /// Default: 5000 (5s)
    #[cfg_attr(feature = "schema", schemars(default))]
    pub initial_initiate_interval_ms: u32,

    /// The interval in milliseconds between initiating gossip rounds.
    ///
    /// This controls how often Kitsune will attempt to find a peer to gossip with.
    ///
    /// This can be set as low as you'd like, but you will still be limited by
    /// [K2GossipConfig::min_initiate_interval_ms]. So a low value for this will result in Kitsune
    /// doing its gossip initiation in a burst. Then, when it has run out of peers, it will idle
    /// for a while.
    ///
    /// Default: 120,000 (2m)
    #[cfg_attr(feature = "schema", schemars(default))]
    pub initiate_interval_ms: u32,

    /// How much jitter to add to the `initiate_interval_ms`.
    ///
    /// Gossip is asymmetric, so between a pair of nodes, one node should not always be the
    /// initiator. To make the initiation time unpredictable, we add some jitter to the initiate
    /// interval. Over time, this will naturally vary which node initiates a gossip round when
    /// two peers would otherwise want to initiate a round at the same time.
    ///
    /// The jitter is added to the `initiate_interval_ms` and is a random value between `0` and
    /// `initiate_jitter_ms`.
    ///
    /// This parameter can be set to `0` to disable jitter.
    ///
    /// Default: 10,000 (10s)
    #[cfg_attr(feature = "schema", schemars(default))]
    pub initiate_jitter_ms: u32,

    /// The minimum amount of time that must be allowed to pass before a gossip round can be
    /// initiated by a given peer.
    ///
    /// This is a rate-limiting mechanism to be enforced against incoming gossip and therefore must
    /// be respected when initiating too.
    ///
    /// Default: 300,000 (5m)
    #[cfg_attr(feature = "schema", schemars(default))]
    // #[deprecated(
    //     since = "0.1.9",
    //     note = "This is being replaced by a burst mechanism instead."
    // )]
    pub min_initiate_interval_ms: u32,

    /// A factor applied to the default gossip initiation rate to permit a burst of gossip rounds
    /// to be accepted.
    ///
    /// This is a rate-limiting mechanism that permits nodes that are joining the network and
    /// needing to sync, to initiate gossip faster than usual.
    ///
    /// Default: 3
    #[cfg_attr(feature = "schema", schemars(default))]
    pub initiate_burst_factor: u32,

    /// The number of initiation windows that the burst is calculated over.
    ///
    /// For example, if this is set to `5`, the initiate interval is set to `1m` and the burst
    /// factor is set to `3` then in 3 minutes, then in a given 3-minute window, 15 gossip rounds
    /// can be accepted.
    ///
    /// Default: 5
    #[cfg_attr(feature = "schema", schemars(default))]
    pub initiate_burst_window_count: u32,

    /// The timeout for a gossip round.
    ///
    /// This will be loosely enforced on both sides. Kitsune periodically checks for timed out
    /// rounds and will terminate them if they have been running for longer than this timeout. The
    /// periodic check cannot be configured and runs every 5 seconds.
    ///
    /// Default: 15,000 (15s)
    #[cfg_attr(feature = "schema", schemars(default))]
    pub round_timeout_ms: u32,

    /// The maximum number of concurrent accepted gossip rounds.
    ///
    /// These are gossip rounds initiated by a remote peer. Being involved in a gossip round
    /// requires us to use resources, so in an attempt to protect ourselves against resource
    /// exhaustion, we can limit the number of concurrent rounds we will accept.
    ///
    /// This value does not have to be set to the same value for all nodes on a network. It is
    /// acceptable for nodes with fewer resources to set a lower value than nodes with more.
    ///
    /// A value of `0` will disable this limit. This would represent a tradeoff between making a
    /// node available to participate in gossip rounds but potentially harming its availability to
    /// serve data if it is overwhelmed.
    ///
    /// Default: 10
    #[cfg_attr(feature = "schema", schemars(default))]
    pub max_concurrent_accepted_rounds: u32,
}

impl Default for K2GossipConfig {
    fn default() -> Self {
        Self {
            max_gossip_op_bytes: 100 * 1024 * 1024,
            max_request_gossip_op_bytes: 100 * 1024 * 1024,
            initial_initiate_interval_ms: 5000,
            initiate_interval_ms: 120_000,
            initiate_jitter_ms: 10_000,
            min_initiate_interval_ms: 300_000,
            initiate_burst_factor: 3,
            initiate_burst_window_count: 5,
            round_timeout_ms: 15_000,
            max_concurrent_accepted_rounds: 10,
        }
    }
}

impl K2GossipConfig {
    /// The initial interval in milliseconds between initiating gossip rounds.
    pub(crate) fn initial_initiate_interval(&self) -> std::time::Duration {
        std::time::Duration::from_millis(
            self.initial_initiate_interval_ms as u64,
        )
    }

    /// The interval between initiating gossip rounds.
    pub(crate) fn initiate_interval(&self) -> std::time::Duration {
        std::time::Duration::from_millis(self.initiate_interval_ms as u64)
    }

    /// The minimum amount of time that must be allowed to pass before a gossip round can be
    /// initiated by a given peer.
    // #[deprecated(
    //     since = "0.1.9",
    //     note = "This is being replaced by a burst mechanism instead."
    // )]
    pub(crate) fn min_initiate_interval(&self) -> std::time::Duration {
        std::time::Duration::from_millis(self.min_initiate_interval_ms as u64)
    }

    /// The timeout for a gossip round.
    pub(crate) fn round_timeout(&self) -> std::time::Duration {
        std::time::Duration::from_millis(self.round_timeout_ms as u64)
    }
}

/// Module-level configuration for K2Gossip.
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct K2GossipModConfig {
    /// CoreBootstrap configuration.
    pub k2_gossip: K2GossipConfig,
}
