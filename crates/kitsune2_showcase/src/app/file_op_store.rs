use std::sync::Arc;

use bytes::Bytes;
use kitsune2_api::{
    BoxFut, Builder, Config, DhtArc, DynOpStore, DynOpStoreFactory, K2Result,
    MetaOp, OpId, OpStore, OpStoreFactory, SpaceId, Timestamp,
};
use kitsune2_core::factories::MemOpStoreFactory;

#[derive(Debug)]
pub struct FileOpStoreFactory {
    mem_op_store_factory: Arc<MemOpStoreFactory>,
}

impl FileOpStoreFactory {
    pub fn create() -> DynOpStoreFactory {
        let out: DynOpStoreFactory = Arc::new(FileOpStoreFactory {
            mem_op_store_factory: Arc::new(MemOpStoreFactory {}),
        });
        out
    }
}

impl OpStoreFactory for FileOpStoreFactory {
    fn default_config(&self, config: &mut Config) -> K2Result<()> {
        self.mem_op_store_factory.default_config(config)
    }

    fn validate_config(&self, config: &Config) -> K2Result<()> {
        self.mem_op_store_factory.validate_config(config)
    }

    fn create(
        &self,
        builder: Arc<Builder>,
        space: SpaceId,
    ) -> BoxFut<'static, K2Result<DynOpStore>> {
        let mem_op_store_factory = self.mem_op_store_factory.clone();
        Box::pin(async move {
            let out: DynOpStore = Arc::new(FileOpStore {
                mem_op_store: mem_op_store_factory
                    .create(builder, space)
                    .await?,
            });
            Ok(out)
        })
    }
}

#[derive(Debug)]
struct FileOpStore {
    mem_op_store: DynOpStore,
}

impl OpStore for FileOpStore {
    fn process_incoming_ops(
        &self,
        op_list: Vec<Bytes>,
    ) -> BoxFut<'_, K2Result<Vec<OpId>>> {
        self.mem_op_store.process_incoming_ops(op_list)
    }

    fn retrieve_op_hashes_in_time_slice(
        &self,
        arc: DhtArc,
        start: Timestamp,
        end: Timestamp,
    ) -> BoxFut<'_, K2Result<(Vec<OpId>, u32)>> {
        self.mem_op_store
            .retrieve_op_hashes_in_time_slice(arc, start, end)
    }

    fn retrieve_ops(
        &self,
        op_ids: Vec<OpId>,
    ) -> BoxFut<'_, K2Result<Vec<MetaOp>>> {
        self.mem_op_store.retrieve_ops(op_ids)
    }

    fn filter_out_existing_ops(
        &self,
        op_ids: Vec<OpId>,
    ) -> BoxFut<'_, K2Result<Vec<OpId>>> {
        self.mem_op_store.filter_out_existing_ops(op_ids)
    }

    fn retrieve_op_ids_bounded(
        &self,
        arc: DhtArc,
        start: Timestamp,
        limit_bytes: u32,
    ) -> BoxFut<'_, K2Result<(Vec<OpId>, u32, Timestamp)>> {
        self.mem_op_store
            .retrieve_op_ids_bounded(arc, start, limit_bytes)
    }

    fn earliest_timestamp_in_arc(
        &self,
        arc: DhtArc,
    ) -> BoxFut<'_, K2Result<Option<Timestamp>>> {
        self.mem_op_store.earliest_timestamp_in_arc(arc)
    }

    fn store_slice_hash(
        &self,
        arc: DhtArc,
        slice_index: u64,
        slice_hash: Bytes,
    ) -> BoxFut<'_, K2Result<()>> {
        self.mem_op_store
            .store_slice_hash(arc, slice_index, slice_hash)
    }

    fn slice_hash_count(&self, arc: DhtArc) -> BoxFut<'_, K2Result<u64>> {
        self.mem_op_store.slice_hash_count(arc)
    }

    fn retrieve_slice_hash(
        &self,
        arc: DhtArc,
        slice_index: u64,
    ) -> BoxFut<'_, K2Result<Option<Bytes>>> {
        self.mem_op_store.retrieve_slice_hash(arc, slice_index)
    }

    fn retrieve_slice_hashes(
        &self,
        arc: DhtArc,
    ) -> BoxFut<'_, K2Result<Vec<(u64, Bytes)>>> {
        self.mem_op_store.retrieve_slice_hashes(arc)
    }
}
