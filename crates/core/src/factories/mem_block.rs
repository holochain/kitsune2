//! An example of a [`Block`] implementation that simply stores all blocked targets in memory.

use kitsune2_api::*;
use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};

/// A factory for creating an instance of a [`Block`] that stores blocked targets in memory.
///
/// This stores [`BlockTarget`]s in memory.
#[derive(Debug)]
pub struct MemBlockFactory {}

impl MemBlockFactory {
    /// Construct a new [`MemBlockFactory`]
    pub fn create() -> DynBlockFactory {
        let out: DynBlockFactory = Arc::new(Self {});
        out
    }
}

impl BlockFactory for MemBlockFactory {
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
    ) -> BoxFut<'static, K2Result<DynBlock>> {
        Box::pin(async move {
            let out: DynBlock = Arc::new(MemBlock::default());
            Ok(out)
        })
    }
}

#[derive(Default)]
struct MemBlock(Mutex<HashSet<BlockTarget>>);

impl std::fmt::Debug for MemBlock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MemBlock").finish()
    }
}

impl Block for MemBlock {
    fn block(&self, target: BlockTarget) -> BoxFut<'static, K2Result<()>> {
        self.0.lock().unwrap().insert(target);

        Box::pin(async { Ok(()) })
    }

    fn is_blocked(
        &self,
        target: &BlockTarget,
    ) -> BoxFut<'static, K2Result<bool>> {
        let is_blocked = self.0.lock().unwrap().contains(target);

        Box::pin(async move { Ok(is_blocked) })
    }

    fn are_all_blocked(
        &self,
        targets: &[BlockTarget],
    ) -> BoxFut<'static, K2Result<bool>> {
        let inner = self.0.lock().unwrap();
        let are_all_blocked =
            targets.iter().all(|target| inner.contains(target));

        Box::pin(async move { Ok(are_all_blocked) })
    }
}

#[cfg(test)]
mod test;
