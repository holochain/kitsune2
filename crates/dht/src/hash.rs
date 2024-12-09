//! Partition of the hash space.
//!
//! The space of possible hashes is mapped to a 32-bit location. See [::kitsune2_api::id::Id::loc] for
//! more information about this. Each agent is responsible for storing and serving some part of
//! the hash space. This module provides a structure that partitions the hash space by some factor.
//!
//! The factor must be chosen up front and cannot be changed. The factor determines the number of
//! partitions to create. A factor of 32 will create a single partition. Each unit decrease from 32
//! will double the number of partitions. A good default for a network that is expected to grow to
//! a few thousand nodes is 21. This will create 2^11 partitions, which is 2048. Given that data is
//! also stored redundantly, say 10 times, this means that 20,000 nodes are needed for the data
//! to be maximally distributed. With higher replication, say 50 times, and a hash factor of 20,
//! over 200,000 nodes are needed for maximal distribution.
//!
//! Each hash partition manages a [PartitionedTime] structure that is responsible for managing the
//! time slices for that hash partition. The interface of this module is largely responsible for
//! delegating the updating of time slices to the inner time partitions. This ensures that all the
//! time partitions are updated in lockstep. This makes reasoning about the space-time partitioning
//! easier.
//!
//! This module must be informed about ops that have been stored. There is no active process here
//! that can look for newly stored ops. When a batch of ops is stored, the [PartitionedHashes] must
//! be informed and will split the ops into the right hash partition based on the location of the op.
//! Ops are then pushed to the inner time partition for each hash partition. It is the time
//! partitions that are responsible for updating the combined hash values.

use crate::PartitionedTime;
use kitsune2_api::{
    ArcLiteral, DynOpStore, K2Error, K2Result, StoredOp, Timestamp,
};
use std::collections::HashMap;

/// A partitioned hash structure.
///
/// Partitions the hash structure into a configurable number of partitions. Each partition is
/// responsible for managing the time slices for that partition.
#[derive(Debug)]
pub struct PartitionedHashes {
    /// This is just a convenience for internal function use.
    /// This should always be exactly `((u32::MAX + 1) / self.partitioned_hashes.len()`.
    size: u32,
    /// The partition count here (length of Vec) should always be a power of 2.
    /// (2**0, 2**1, etc).
    partitioned_hashes: Vec<HashPartition>,
}

#[derive(Debug)]
struct HashPartition {
    _arc: ArcLiteral,
    partitioned_time: PartitionedTime,
}

impl PartitionedHashes {
    /// Create a new partitioned hash structure.
    ///
    /// The `hash_factor` determines the number of partitions to create. A value of 32 will create
    /// a single partition. Each unit decrease from 32 will double the number of partitions.
    ///
    /// Each hash partition owns a [PartitionedTime] structure that is responsible for managing
    /// the time slices for that hash partition. Other parameters to this function are used to
    /// create the [PartitionedTime] structure.
    pub async fn try_from_store(
        hash_factor: u8,
        time_factor: u8,
        current_time: Timestamp,
        store: DynOpStore,
    ) -> K2Result<Self> {
        if hash_factor > 32 {
            return Err(K2Error::other("Hash factor must be 32 or lower"));
        }

        let (size, num_buckets) = if hash_factor == 32 {
            (u32::MAX, 1)
        } else {
            let size = 1u32 << hash_factor;

            // We will always be one bucket short because u32::MAX is not a power of two. It is one less
            // than a power of two, so the last bucket is always one short.
            (size, (u32::MAX / size) + 1)
        };

        let mut partitioned_hashes = Vec::with_capacity(num_buckets as usize);
        for i in 0..num_buckets {
            partitioned_hashes.push(
                HashPartition::try_from_store(
                    (i * size, (i + 1).overflowing_mul(size).0),
                    time_factor,
                    current_time,
                    store.clone(),
                )
                .await?,
            );
        }

        tracing::info!(
            "Allocated {} hash partitions",
            partitioned_hashes.len()
        );

        Ok(Self {
            size,
            partitioned_hashes,
        })
    }

    /// Get the next update time of the inner time partitions.
    pub fn next_update_at(&self) -> Timestamp {
        // Because the minimum `hash_factor` is 0, and we compute 2^hash_factor, there will always be at least one partition.
        self.partitioned_hashes
            .first()
            .expect("Always at least one hash partition")
            .partitioned_time
            .next_update_at()
    }

    /// Update the time partitions for each hash partition.
    pub async fn update(
        &mut self,
        store: DynOpStore,
        current_time: Timestamp,
    ) -> K2Result<()> {
        for partition in self.partitioned_hashes.iter_mut() {
            partition
                .partitioned_time
                .update(store.clone(), current_time)
                .await?;
        }

        Ok(())
    }

    /// Inform the time partitions of ops that have been stored.
    ///
    /// The ops are split into the right hash partition based on the location of the op. Then the
    /// updating of hashes is delegated to the inner time partition for each hash partition.
    pub async fn inform_ops_stored(
        &mut self,
        store: DynOpStore,
        stored_ops: Vec<StoredOp>,
    ) -> K2Result<()> {
        let by_location = stored_ops
            .into_iter()
            .map(|op| {
                let location = op.op_id.loc();
                (location / self.size, op)
            })
            .fold(
                HashMap::<u32, Vec<StoredOp>>::new(),
                |mut acc, (location, op)| {
                    acc.entry(location).or_default().push(op);
                    acc
                },
            );

        for (location, ops) in by_location {
            self.partitioned_hashes[location as usize]
                .partitioned_time
                .inform_ops_stored(store.clone(), ops)
                .await?;
        }

        Ok(())
    }
}

impl HashPartition {
    pub async fn try_from_store(
        arc: ArcLiteral,
        time_factor: u8,
        current_time: Timestamp,
        store: DynOpStore,
    ) -> K2Result<Self> {
        let partitioned_time = PartitionedTime::try_from_store(
            time_factor,
            current_time,
            arc,
            store,
        )
        .await?;
        Ok(Self {
            _arc: arc,
            partitioned_time,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::UNIT_TIME;
    use kitsune2_api::{OpId, OpStore, ARC_LITERAL_FULL, UNIX_TIMESTAMP};
    use kitsune2_memory::{Kitsune2MemoryOp, Kitsune2MemoryOpStore};
    use kitsune2_test_utils::enable_tracing;
    use std::sync::Arc;
    use std::time::Duration;

    #[tokio::test]
    async fn create_with_no_partitioning() {
        enable_tracing();

        let store = Arc::new(Kitsune2MemoryOpStore::default());
        let ph =
            PartitionedHashes::try_from_store(32, 14, Timestamp::now(), store)
                .await
                .unwrap();
        assert_eq!(1, ph.partitioned_hashes.len());
        assert_eq!(u32::MAX, ph.size);
        assert_eq!(ARC_LITERAL_FULL, ph.partitioned_hashes[0]._arc);
    }

    #[tokio::test]
    async fn create_with_default_partition_factor() {
        enable_tracing();

        let store = Arc::new(Kitsune2MemoryOpStore::default());
        let ph =
            PartitionedHashes::try_from_store(20, 14, Timestamp::now(), store)
                .await
                .unwrap();
        assert_eq!(1 << 20, ph.size);
        assert_eq!((0, true), (1u32 << 20).overflowing_mul(ph.size));
    }

    #[tokio::test]
    async fn covers_full_arc() {
        enable_tracing();

        // Check that the behaviour is consistent for a reasonable range of expected values
        for hash_factor in 20u8..31 {
            let store = Arc::new(Kitsune2MemoryOpStore::default());
            let ph = PartitionedHashes::try_from_store(
                hash_factor,
                14,
                Timestamp::now(),
                store,
            )
            .await
            .unwrap();

            let mut start: u32 = 0;
            for i in 0..ph.partitioned_hashes.len() {
                let end = start.overflowing_add(ph.size).0;
                assert_eq!(start, ph.partitioned_hashes[i]._arc.0);
                assert_eq!(end, ph.partitioned_hashes[i]._arc.1);
                start = end;
            }

            assert_eq!(0, start, "While checking factor {}", hash_factor);
        }
    }

    #[tokio::test]
    async fn inform_ops_stored_in_full_slices() {
        enable_tracing();

        let store = Arc::new(Kitsune2MemoryOpStore::default());
        let mut ph = PartitionedHashes::try_from_store(
            20,
            14,
            Timestamp::now(),
            store.clone(),
        )
        .await
        .unwrap();

        let op_id_bytes_1 = bytes::Bytes::from_static(&[7, 0, 0, 0]);
        let op_id_bytes_2 = bytes::Bytes::from(ph.size.to_le_bytes().to_vec());
        ph.inform_ops_stored(
            store.clone(),
            vec![
                StoredOp {
                    op_id: OpId::from(op_id_bytes_1.clone()),
                    timestamp: UNIX_TIMESTAMP,
                },
                StoredOp {
                    op_id: OpId::from(op_id_bytes_2.clone()),
                    timestamp: UNIX_TIMESTAMP
                        + ph.partitioned_hashes[0]
                            .partitioned_time
                            .full_slice_duration(),
                },
            ],
        )
        .await
        .unwrap();

        let count = store.slice_hash_count((0, ph.size)).await.unwrap();
        assert_eq!(1, count);

        let hash = store.retrieve_slice_hash((0, ph.size), 0).await.unwrap();
        assert!(hash.is_some());
        assert_eq!(op_id_bytes_1, hash.unwrap());

        let count = store
            .slice_hash_count((ph.size, 2 * ph.size))
            .await
            .unwrap();
        // Note that this is because we've stored at id 1, not that two hashes ended up in this
        // partition.
        assert_eq!(2, count);

        let hash = store
            .retrieve_slice_hash((ph.size, 2 * ph.size), 1)
            .await
            .unwrap();
        assert!(hash.is_some());
        assert_eq!(op_id_bytes_2, hash.unwrap());
    }

    #[tokio::test]
    async fn inform_ops_stored_in_partial_slices() {
        enable_tracing();

        let store = Arc::new(Kitsune2MemoryOpStore::default());
        let mut ph = PartitionedHashes::try_from_store(
            20,
            14,
            Timestamp::now(),
            store.clone(),
        )
        .await
        .unwrap();

        let op_id_bytes_1 = bytes::Bytes::from_static(&[100, 0, 0, 0]);
        let op_id_bytes_2 = bytes::Bytes::from(ph.size.to_le_bytes().to_vec());
        ph.inform_ops_stored(
            store.clone(),
            vec![
                // Stored in the first partial
                StoredOp {
                    op_id: OpId::from(op_id_bytes_1.clone()),
                    timestamp: ph.partitioned_hashes[0]
                        .partitioned_time
                        .full_slice_end_timestamp(),
                },
                // Stored in the second time slice of the first space partition.
                StoredOp {
                    op_id: OpId::from(op_id_bytes_2.clone()),
                    timestamp: ph.partitioned_hashes[0]
                        .partitioned_time
                        .full_slice_end_timestamp()
                        + Duration::from_secs((1 << 13) * UNIT_TIME.as_secs()),
                },
            ],
        )
        .await
        .unwrap();

        // No full slices should get stored
        for i in 0..(u32::MAX / ph.size) {
            let count = store
                .slice_hash_count((i * ph.size, (i + 1) * ph.size))
                .await
                .unwrap();
            assert_eq!(0, count);
        }

        let partial_slice =
            &ph.partitioned_hashes[0].partitioned_time.partials()[0];
        assert_eq!(
            op_id_bytes_1,
            bytes::Bytes::from(partial_slice.hash().to_vec())
        );

        let partial_slice =
            &ph.partitioned_hashes[1].partitioned_time.partials()[1];
        assert_eq!(
            op_id_bytes_2,
            bytes::Bytes::from(partial_slice.hash().to_vec())
        );
    }

    #[tokio::test]
    async fn next_update_at_consistent() {
        enable_tracing();

        let store = Arc::new(Kitsune2MemoryOpStore::default());
        let now = Timestamp::now();
        let ph = PartitionedHashes::try_from_store(30, 14, now, store.clone())
            .await
            .unwrap();

        assert_eq!(4, ph.partitioned_hashes.len());

        let hashes_next_update_at = ph.next_update_at();
        assert!(hashes_next_update_at >= now);

        for h in ph.partitioned_hashes {
            assert_eq!(
                hashes_next_update_at,
                h.partitioned_time.next_update_at()
            );
        }
    }

    #[tokio::test]
    async fn update_all() {
        enable_tracing();
        let store = Arc::new(Kitsune2MemoryOpStore::default());
        let now = Timestamp::now();
        let mut ph =
            PartitionedHashes::try_from_store(30, 14, now, store.clone())
                .await
                .unwrap();

        assert_eq!(4, ph.partitioned_hashes.len());

        for h in ph.partitioned_hashes.iter() {
            store
                .process_incoming_ops(vec![Kitsune2MemoryOp::new(
                    // Place the op within the current hash partition
                    OpId::from(bytes::Bytes::copy_from_slice(
                        (h._arc.0 + 1).to_le_bytes().as_slice(),
                    )),
                    now,
                    h._arc.1.to_be_bytes().to_vec(),
                )
                .try_into()
                .unwrap()])
                .await
                .unwrap();
        }

        // Check nothing is currently stored in the partials
        for h in &ph.partitioned_hashes {
            for ps in h.partitioned_time.partials() {
                assert!(ps.hash().is_empty())
            }
        }

        // Update with enough extra time to allocate a new partial over the current time
        ph.update(store, now + UNIT_TIME).await.unwrap();

        // Check that the partials have been updated
        for h in &ph.partitioned_hashes {
            // Exactly one partial should now have a hash
            assert_eq!(
                1,
                h.partitioned_time
                    .partials()
                    .iter()
                    .filter(|ps| !ps.hash().is_empty())
                    .count()
            );
        }
    }
}
