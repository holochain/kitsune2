//! Kitsune2 report types.

use crate::{
    builder, config, BoxFut, DynLocalAgentStore, K2Result, OpId, SpaceId, Url,
};
use std::sync::Arc;

/// Trait for implementing a report module in kitsune2.
///
/// The first implemented report gathers op count and bytes transferred
/// about ops fetched either as part of gossip or publish operations.
pub trait Report: 'static + Send + Sync + std::fmt::Debug {
    /// To aid in concrete downcasting.
    fn as_any(&self) -> &dyn std::any::Any;

    /// Notify that a space was created.
    fn space(
        &self,
        _space_id: SpaceId,
        _local_agent_store: DynLocalAgentStore,
    ) {
        // provided impl is a no-op
    }

    /// Notify that we have fetched op data from a remote peer.
    fn fetched_op(
        &self,
        _space_id: SpaceId,
        _source: Url,
        _op_id: OpId,
        _size_bytes: u64,
    ) {
        // provided impl is a no-op
    }
}

/// Trait object [Report].
pub type DynReport = Arc<dyn Report>;

/// A factory for creating report instances.
pub trait ReportFactory: 'static + Send + Sync + std::fmt::Debug {
    /// Help the builder construct a default config from the chosen
    /// module factories.
    fn default_config(&self, config: &mut config::Config) -> K2Result<()>;

    /// Validate configuration.
    fn validate_config(&self, config: &config::Config) -> K2Result<()>;

    /// Construct a Report instance.
    fn create(
        &self,
        builder: Arc<builder::Builder>,
        tx: crate::DynTransport,
    ) -> BoxFut<'static, K2Result<DynReport>>;
}

/// Trait object [ReportFactory].
pub type DynReportFactory = Arc<dyn ReportFactory>;
