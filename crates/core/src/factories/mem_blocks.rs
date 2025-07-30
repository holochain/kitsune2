//! A test implementation of the [`Blocks`] module that stores all blocked targets in memory.

use kitsune2_api::*;
use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};

/// A factory for creating an instance of a [`Blocks`] that stores blocked targets in memory.
///
/// This stores [`BlockTarget`]s in memory.
#[derive(Debug)]
pub struct MemBlocksFactory {}

impl MemBlocksFactory {
    /// Construct a new [`MemBlocksFactory`]
    pub fn create() -> DynBlocksFactory {
        let out: DynBlocksFactory = Arc::new(Self {});
        out
    }
}

impl BlocksFactory for MemBlocksFactory {
    fn default_config(&self, _config: &mut Config) -> K2Result<()> {
        Ok(())
    }

    fn validate_config(&self, _config: &Config) -> K2Result<()> {
        Ok(())
    }

    fn create(
        &self,
        _builder: Arc<Builder>,
        _space_id: SpaceId,
    ) -> BoxFut<'static, K2Result<DynBlocks>> {
        Box::pin(async move {
            let out: DynBlocks = Arc::new(MemBlocks::default());
            Ok(out)
        })
    }
}

/// A simple, in-memory implementation of the [`Blocks`] trait.
#[derive(Default)]
pub struct MemBlocks(Mutex<HashSet<BlockTarget>>);

impl std::fmt::Debug for MemBlocks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MemBlocks").finish()
    }
}

impl Blocks for MemBlocks {
    fn block(&self, target: BlockTarget) -> BoxFut<'static, K2Result<()>> {
        self.0
            .lock()
            .expect("MemBlocks inner Mutex is poisoned")
            .insert(target);

        Box::pin(async { Ok(()) })
    }

    fn is_blocked(
        &self,
        target: BlockTarget,
    ) -> BoxFut<'static, K2Result<bool>> {
        let is_blocked = self
            .0
            .lock()
            .expect("MemBlocks inner Mutex is poisoned")
            .contains(&target);

        Box::pin(async move { Ok(is_blocked) })
    }

    fn are_all_blocked(
        &self,
        targets: Vec<BlockTarget>,
    ) -> BoxFut<'static, K2Result<bool>> {
        let inner = self.0.lock().expect("MemBlocks inner Mutex is poisoned");
        let are_all_blocked =
            targets.iter().all(|target| inner.contains(target));

        Box::pin(async move { Ok(are_all_blocked) })
    }
}

#[cfg(test)]
mod test;
