//! Partition of the hash space.
//!
//! The space of possible hashes is mapped to a 32-bit location. See [Id::loc](::kitsune2_api::id::Id::loc)
//! for more information about this. Each agent is responsible for storing and serving some part of
//! the hash space. This module provides a structure that partitions the hash space into 512
//! equally-sized DHT arcs.
//!
//! Each space partition manages a [PartitionedTime] structure that is responsible for managing the
//! time slices for that space partition. The interface of this module is largely responsible for
//! delegating the updating of time slices to the inner time partitions. This ensures all the time
//! partitions are updated in lockstep, which makes reasoning about the space-time partitioning
//! easier.
//!
//! This module must be informed about ops that have been stored. There is no active process here
//! that can look for newly stored ops. When a batch of ops is stored, the [PartitionedHashes] must
//! be informed and will split the ops into the right space partition based on the location of the op.
//! Ops are then pushed to the inner time partition for each space partition. It is the time
//! partitions that are responsible for updating the combined hash values.
//!
//! That completes the high level information for this module, what follows is a more detailed
//! explanation of the design.
//!
//! The bit-depth of the location roughly determines how evenly ops are distributed across the
//! partitions. For example, an 8-bit location can only map op hashes to 256 possible locations,
//! which would not use the top half of the partitions here. A 32-bit location retains enough
//! information to use the full range of partitions and can be calculated inside a 32-bit integer.
//! A 16-bit or 64-bit location would work too, and the choice between the options is not
//! quantified here.
//!
//! The choice of 512 partitions is a tradeoff between the minimum amount of data that a node must
//! store to participate in the network and the amount of data that must be sent over the network
//! to discover what op data needs to be fetched. Every node will store between 0 and 512
//! partitions. For nodes that are choosing to store data, the minimum they can store is 1
//! partition. Or in other words, 1/512th of the total data. To put this in context, an application
//! that has a complete data set of 5TB would require a minimum of 10GB of storage dedicated to
//! that app to be able to participate in the network.
//! It is important to note though, that each partition is managing a [PartitionedTime] structure
//! which requires some memory and processing to maintain. Nodes that take on more partitions will
//! have more work to do and will have to send more data during gossip rounds. That happens when
//! there are changes to data that has made it into a time slice and become part of a combined
//! hash. Generally, nodes will be able to just sync "what's new" but for catch-up, this is true.
//! As a consequence, a new network that hasn't yet gained enough members to start reducing how
//! many partitions are covered by each node will have a higher overhead for each node.
//! It is the responsibility of the gossip module to work out how to be efficient about sending
//! the minimum amount of data required to keep all nodes up to date. However, the worst case is
//! that 512 combined hashes must be sent to find out which partition has a mismatch for a given
//! time slice. Over time, there will be more time slices to check.
//! Therefore, the choice of 512 should be thought of as a best-effort choice to ask a reasonable
//! amount of work and storage from each node in a large network but being no larger than that
//! so we don't add overhead to gossip.
//!
//! It is also important to note that nodes may have to store the full 512 partitions if the
//! number of peers in the network is not high enough. The network must have enough peers that
//! data is still stored redundantly before the number of partitions per node can be reduced.
//!
//! It should also be understood that as well as the minimum storage requirement and the
//! possibility of having to store more data on smaller networks, a user of a Kitsune2 app is
//! likely to want to use data that isn't stored on their own node. That means that although they
//! are free to delete that data again once they are done with it, the amount of data stored and
//! served by a node is not the only storage it may require. It is just a lower bound on the
//! free space that a node will need to have available.

use crate::arc_set::ArcSet;
use crate::combine::combine_hashes;
use crate::PartitionedTime;
use kitsune2_api::{
    DhtArc, DynOpStore, K2Error, K2Result, StoredOp, Timestamp,
};
use std::collections::HashMap;

/// A partitioned hash structure.
///
/// Partitions the hash structure into a fixed number of partitions. Each partition is
/// responsible for managing the time slices for that partition using a [PartitionedTime].
#[derive(Debug)]
pub struct PartitionedHashes {
    /// This is just a convenience for internal function use.
    /// This should always be exactly `(u32::MAX / self.partitioned_hashes.len()) + 1`.
    size: u32,
    /// The partition count here (length of Vec) is always a power of 2.
    ///
    /// That is, (2**0, 2**1, etc). It is currently always 512 and is not configurable.
    partitioned_hashes: Vec<PartitionedTime>,
}

pub type PartialTimeSliceDetails = HashMap<u32, HashMap<u32, bytes::Bytes>>;

impl PartitionedHashes {
    /// Create a new partitioned hash structure.
    ///
    /// This creates a new partitioned hash structure which has 512 partitions. This is currently
    /// not configurable. See the module documentation for more details.
    ///
    /// Each space partition owns a [PartitionedTime] structure that is responsible for managing
    /// the time slices for that space partition. Other parameters to this function are used to
    /// create the [PartitionedTime] structure.
    pub async fn try_from_store(
        time_factor: u8,
        current_time: Timestamp,
        store: DynOpStore,
    ) -> K2Result<Self> {
        // Creates 512 partitions, because 32 - 23 = 9, and 2^9 = 512.
        const SIZE: u32 = 1u32 << 23;

        // We will always be one bucket short because u32::MAX is not a power of two. It is one less
        // than a power of two, so the last bucket is always one short.
        let num_partitions = (u32::MAX / SIZE) + 1;
        let mut partitioned_hashes =
            Vec::with_capacity(num_partitions as usize);
        for i in 0..(num_partitions - 1) {
            partitioned_hashes.push(
                PartitionedTime::try_from_store(
                    time_factor,
                    current_time,
                    DhtArc::Arc(i * SIZE, (i + 1).saturating_mul(SIZE) - 1),
                    store.clone(),
                )
                .await?,
            );
        }

        // The last partition must be handled separately because it needs to include the
        // remainder of the hash space.
        partitioned_hashes.push(
            PartitionedTime::try_from_store(
                time_factor,
                current_time,
                DhtArc::Arc((num_partitions - 1) * SIZE, u32::MAX),
                store.clone(),
            )
            .await?,
        );

        tracing::info!(
            "Allocated [{}] space partitions",
            partitioned_hashes.len()
        );

        Ok(Self {
            size: SIZE,
            partitioned_hashes,
        })
    }

    /// Get the next update time of the inner time partitions.
    pub fn next_update_at(&self) -> Timestamp {
        // We know that a fixed number of partitions is always present,
        // so this is safe to unwrap here.
        self.partitioned_hashes
            .first()
            .expect("Always at least one space partition")
            .next_update_at()
    }

    /// Update the time partitions for each space partition.
    pub async fn update(
        &mut self,
        store: DynOpStore,
        current_time: Timestamp,
    ) -> K2Result<()> {
        for partition in self.partitioned_hashes.iter_mut() {
            partition.update(store.clone(), current_time).await?;
        }

        Ok(())
    }

    /// Inform the time partitions of ops that have been stored.
    ///
    /// The ops are placed into the right space partition based on the location of the op. Then the
    /// updating of hashes is delegated to the inner time partition for each space partition.
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
                .inform_ops_stored(store.clone(), ops)
                .await?;
        }

        Ok(())
    }

    pub fn dht_arc_for_sector_id(&self, sector_id: u32) -> K2Result<DhtArc> {
        let sector_id = sector_id as usize;
        if sector_id >= self.partitioned_hashes.len() {
            return Err(K2Error::other("Sector ID out of bounds"));
        }

        Ok(*self.partitioned_hashes[sector_id].arc_constraint())
    }

    pub fn time_bounds_for_full_slice_id(
        &self,
        slice_id: u64,
    ) -> K2Result<(Timestamp, Timestamp)> {
        self.partitioned_hashes[0].time_bounds_for_full_slice_id(slice_id)
    }

    pub fn time_bounds_for_partial_slice_id(
        &self,
        slice_id: u32,
    ) -> K2Result<(Timestamp, Timestamp)> {
        self.partitioned_hashes[0].time_bounds_for_partial_slice_id(slice_id)
    }
}

// Query implementation
impl PartitionedHashes {
    pub async fn full_time_slice_disc_top_hash(
        &self,
        arc_set: &ArcSet,
        store: DynOpStore,
    ) -> K2Result<(bytes::Bytes, Timestamp)> {
        let mut combined = bytes::BytesMut::new();
        for (sector_id, sector) in self.partitioned_hashes.iter().enumerate() {
            if !arc_set.includes_sector_id(sector_id as u32) {
                continue;
            }

            let hash =
                sector.combined_full_time_slice_hash(store.clone()).await?;
            if !hash.is_empty() {
                combine_hashes(&mut combined, hash);
            }
        }

        let timestamp = self.partitioned_hashes[0].full_slice_end_timestamp();

        Ok((combined.freeze(), timestamp))
    }

    pub fn partial_time_rings(&self, arc_set: &ArcSet) -> Vec<bytes::Bytes> {
        let mut partials = Vec::with_capacity(arc_set.covered_sector_count());

        for (sector_id, sector) in self.partitioned_hashes.iter().enumerate() {
            if !arc_set.includes_sector_id(sector_id as u32) {
                continue;
            }

            partials.push(sector.combined_partial_slice_hashes().peekable());
        }

        let mut out = Vec::new();
        let mut combined = bytes::BytesMut::new();
        while partials[0].peek().is_some() {
            combined.clear();
            for partial in &mut partials {
                if let Some(hash) = partial.next() {
                    if !hash.is_empty() {
                        combine_hashes(&mut combined, hash);
                    }
                }
            }
            out.push(combined.clone().freeze());
        }

        out
    }

    pub async fn full_time_slice_sector_hashes(
        &self,
        arc_set: &ArcSet,
        store: DynOpStore,
    ) -> K2Result<(HashMap<u32, bytes::Bytes>, Timestamp)> {
        let mut out = HashMap::new();
        for (sector_id, sector) in self.partitioned_hashes.iter().enumerate() {
            if !arc_set.includes_sector_id(sector_id as u32) {
                continue;
            }

            let hash =
                sector.combined_full_time_slice_hash(store.clone()).await?;
            if !hash.is_empty() {
                out.insert(sector_id as u32, hash);
            }
        }

        let timestamp = self.partitioned_hashes[0].full_slice_end_timestamp();

        Ok((out, timestamp))
    }

    pub async fn full_time_slice_sector_details(
        &self,
        sector_ids: Vec<u32>,
        arc_set: &ArcSet,
        store: DynOpStore,
    ) -> K2Result<(HashMap<u32, HashMap<u64, bytes::Bytes>>, Timestamp)> {
        let sectors_ids = sector_ids
            .into_iter()
            .collect::<std::collections::HashSet<_>>();

        let mut out = HashMap::new();

        for (sector_id, sector) in self.partitioned_hashes.iter().enumerate() {
            if !arc_set.includes_sector_id(sector_id as u32)
                || !sectors_ids.contains(&(sector_id as u32))
            {
                continue;
            }

            out.insert(
                sector_id as u32,
                sector.full_time_slice_hashes(store.clone()).await?,
            );
        }

        let timestamp = self.partitioned_hashes[0].full_slice_end_timestamp();

        Ok((out, timestamp))
    }

    pub fn partial_time_slice_details(
        &self,
        ring_ids: Vec<u32>,
        arc_set: &ArcSet,
    ) -> K2Result<(PartialTimeSliceDetails, Timestamp)> {
        let mut out = HashMap::new();

        for (sector_id, sector) in self.partitioned_hashes.iter().enumerate() {
            if !arc_set.includes_sector_id(sector_id as u32) {
                continue;
            }

            for ring_id in &ring_ids {
                let hash = sector.partial_slice_hash(*ring_id)?;

                // Important to capture that the ring didn't match even if the hash is empty and
                // therefore we won't communicate this sector.
                let entry = out.entry(*ring_id).or_insert_with(HashMap::new);
                if !hash.is_empty() {
                    entry.insert(sector_id as u32, hash);
                }
            }
        }

        let timestamp = self.partitioned_hashes[0].full_slice_end_timestamp();

        Ok((out, timestamp))
    }
}

#[cfg(test)]
impl PartitionedHashes {
    pub fn full_slice_end_timestamp(&self) -> Timestamp {
        self.partitioned_hashes[0].full_slice_end_timestamp()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::UNIT_TIME;
    use kitsune2_api::{OpId, OpStore, UNIX_TIMESTAMP};
    use kitsune2_memory::{Kitsune2MemoryOp, Kitsune2MemoryOpStore};
    use kitsune2_test_utils::enable_tracing;
    use std::sync::Arc;
    use std::time::Duration;

    #[tokio::test]
    async fn try_from_store() {
        enable_tracing();

        let store = Arc::new(Kitsune2MemoryOpStore::default());
        let ph = PartitionedHashes::try_from_store(14, Timestamp::now(), store)
            .await
            .unwrap();
        assert_eq!(512, ph.partitioned_hashes.len());
        assert_eq!((u32::MAX / 512) + 1, ph.size);
        assert_eq!(
            &DhtArc::Arc(0, ph.size - 1),
            ph.partitioned_hashes[0].arc_constraint()
        );
        assert_eq!(
            &DhtArc::Arc(511 * ph.size, u32::MAX),
            ph.partitioned_hashes[511].arc_constraint()
        );
    }

    #[tokio::test]
    async fn covers_full_arc() {
        enable_tracing();

        let store = Arc::new(Kitsune2MemoryOpStore::default());
        let ph = PartitionedHashes::try_from_store(14, UNIX_TIMESTAMP, store)
            .await
            .unwrap();

        let mut start: u32 = 0;
        for i in 0..(ph.partitioned_hashes.len() - 1) {
            let end = start.overflowing_add(ph.size).0;
            assert_eq!(
                DhtArc::Arc(start, end - 1),
                *ph.partitioned_hashes[i].arc_constraint()
            );
            start = end;
        }

        assert_eq!(
            DhtArc::Arc(start, u32::MAX),
            *ph.partitioned_hashes.last().unwrap().arc_constraint()
        );
    }

    #[tokio::test]
    async fn inform_ops_stored_in_full_slices() {
        enable_tracing();

        let store = Arc::new(Kitsune2MemoryOpStore::default());
        let mut ph = PartitionedHashes::try_from_store(
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
                        + ph.partitioned_hashes[0].full_slice_duration(),
                },
            ],
        )
        .await
        .unwrap();

        let count = store
            .slice_hash_count(DhtArc::Arc(0, ph.size - 1))
            .await
            .unwrap();
        assert_eq!(1, count);

        let hash = store
            .retrieve_slice_hash(DhtArc::Arc(0, ph.size - 1), 0)
            .await
            .unwrap();
        assert!(hash.is_some());
        assert_eq!(op_id_bytes_1, hash.unwrap());

        let count = store
            .slice_hash_count(DhtArc::Arc(ph.size, 2 * ph.size - 1))
            .await
            .unwrap();
        // Note that this is because we've stored at id 1, not that two hashes ended up in this
        // partition.
        assert_eq!(2, count);

        let hash = store
            .retrieve_slice_hash(DhtArc::Arc(ph.size, 2 * ph.size - 1), 1)
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
                // Stored in the first time slice of the first space partition.
                StoredOp {
                    op_id: OpId::from(op_id_bytes_1.clone()),
                    timestamp: ph.partitioned_hashes[0]
                        .full_slice_end_timestamp(),
                },
                // Stored in the second time slice of the first space partition.
                StoredOp {
                    op_id: OpId::from(op_id_bytes_2.clone()),
                    timestamp: ph.partitioned_hashes[0]
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
                .slice_hash_count(DhtArc::Arc(i * ph.size, (i + 1) * ph.size))
                .await
                .unwrap();
            assert_eq!(0, count);
        }

        let partial_slice = &ph.partitioned_hashes[0].partials()[0];
        assert_eq!(
            op_id_bytes_1,
            bytes::Bytes::from(partial_slice.hash().to_vec())
        );

        let partial_slice = &ph.partitioned_hashes[1].partials()[1];
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
        let ph = PartitionedHashes::try_from_store(14, now, store.clone())
            .await
            .unwrap();

        let hashes_next_update_at = ph.next_update_at();
        assert!(hashes_next_update_at >= now);

        for h in ph.partitioned_hashes {
            assert_eq!(hashes_next_update_at, h.next_update_at());
        }
    }

    #[tokio::test]
    async fn update_all() {
        enable_tracing();
        let store = Arc::new(Kitsune2MemoryOpStore::default());
        let now = Timestamp::now();
        let mut ph = PartitionedHashes::try_from_store(14, now, store.clone())
            .await
            .unwrap();

        assert_eq!(512, ph.partitioned_hashes.len());

        for h in ph.partitioned_hashes.iter() {
            let (start, end) = match h.arc_constraint() {
                DhtArc::Arc(s, e) => (s, e),
                _ => panic!("Expected an arc"),
            };
            store
                .process_incoming_ops(vec![Kitsune2MemoryOp::new(
                    // Place the op within the current space partition
                    OpId::from(bytes::Bytes::copy_from_slice(
                        (start + 1).to_le_bytes().as_slice(),
                    )),
                    now,
                    end.to_be_bytes().to_vec(),
                )
                .try_into()
                .unwrap()])
                .await
                .unwrap();
        }

        // Check nothing is currently stored in the partials
        for h in &ph.partitioned_hashes {
            for ps in h.partials() {
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
                h.partials()
                    .iter()
                    .filter(|ps| !ps.hash().is_empty())
                    .count()
            );
        }
    }
}
