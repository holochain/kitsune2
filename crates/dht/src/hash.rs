use crate::arc_set::ArcSet;
use crate::combine::combine_hashes;
use crate::{TimePartition, SECTOR_SIZE};
use kitsune2_api::{
    DhtArc, DynOpStore, K2Error, K2Result, StoredOp, Timestamp,
};
use std::collections::{HashMap, HashSet};

/// A partition of the hash space into sectors.
///
/// Partitions the hash structure into a fixed number of sectors. Each sector is
/// responsible for managing the time slices for that sector using a [TimePartition].
///
/// The set of possible hashes is mapped to a 32-bit location. See [Id::loc](::kitsune2_api::Id::loc)
/// for more information about this. Each agent is responsible for storing and serving some part of
/// the hash space. This module provides a structure that partitions the hash space into 512
/// equally-sized DHT sectors.
///
/// Each sector manages a [TimePartition] structure that is responsible for managing the time
/// slices for that sector. The interface of this module is largely responsible for delegating
/// the updating of time slices to the inner [TimePartition]s. This ensures all the
/// [TimePartition]s are updated in lockstep, which makes reasoning about the space-time state
/// easier.
///
/// This module must be informed about ops that have been stored. There is no active process here
/// that can look for newly stored ops. When a batch of ops is stored, the [HashPartition] must
/// be informed and will split the ops into the right sector based on the location of the op. Ops
/// are then pushed to the inner [TimePartition] for each sector. That determines which time slice
/// the ops belong to and update the combined hash values for affected slices.
///
/// That completes the high level information for this module, what follows is a more detailed
/// explanation of the design.
///
/// The bit-depth of the location roughly determines how evenly ops are distributed across the
/// sectors. For example, an 8-bit location can only map op hashes to 256 possible locations,
/// which would not use the top half of the sectors here. A 32-bit location retains enough
/// information to use the full range of sectors. A 16-bit or 64-bit location would work too, and
/// the choice between the options is not quantified here.
///
/// The choice of 512 sectors is a tradeoff between the minimum amount of data that a node must
/// store to participate in the network and the amount of data that must be sent over the network
/// to discover what op data needs to be fetched. Every node will store between 0 and 512
/// sectors. For nodes that are choosing to store data, the minimum they can store is 1
/// sector. Or in other words, 1/512th of the total data. To put this in context, an application
/// that has a complete data set of 5TB would require a minimum of 10GB of storage dedicated to
/// that app to be able to participate in the network.
/// It is important to note though, that each sector is managing a [TimePartition] structure
/// which requires some memory and processing to maintain. Nodes that take on more partitions will
/// have more work to do and will have to send more data to sync [Dht](crate::dht::Dht)s.
/// That happens when there are changes to data that has made it into a time slice and become part
/// of a combined hash. Generally, nodes will be able to just sync "what's new" but for catch-up,
/// this is true.
/// As a consequence, a new network that hasn't yet gained enough members to start reducing how
/// many sectors are covered by each node will have a higher overhead for each node.
/// It is the responsibility of the gossip module to work out how to be efficient about sending
/// the minimum amount of data required to keep all nodes up to date. However, the worst case is
/// that 512 combined hashes must be sent to find out which sector has a mismatch for a given
/// time slice. Over time, there will be more time slices to check.
/// Therefore, the choice of 512 should be thought of as a best-effort choice to ask a reasonable
/// amount of work and storage from each node in a large network but being no larger than that
/// so we don't add overhead to gossip.
///
/// It is also important to note that nodes may have to store the full 512 sectors if the
/// number of peers in the network is not high enough. The network must have enough peers that
/// data is still stored redundantly before the number of sectors per node can be reduced.
///
/// It should also be understood that as well as the minimum storage requirement and the
/// possibility of having to store more data on smaller networks, a user of a Kitsune2 app is
/// likely to want to use data that isn't stored on their own node. That means that although they
/// are free to delete that data again once they are done with it, the amount of data stored and
/// served by a node is not the only storage it may require. It is just a lower bound on the
/// free space that a node will need to have available.
pub struct HashPartition {
    /// This is just a convenience for internal function use.
    /// This should always be exactly `(u32::MAX / self.partitioned_hashes.len()) + 1`.
    size: u32,
    /// The sector count here (length of Vec) is always a power of 2.
    ///
    /// That is, (2**0, 2**1, etc). It is currently always 512 and is not configurable.
    sectors: Vec<TimePartition>,
}

impl std::fmt::Debug for HashPartition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Note that `sectors` are not included by default because it is a large amount of
        // information to see on in logs. If you need to debug the stored data, then consider
        // using the query methods to understand what is present.
        f.debug_struct("HashPartition")
            .field("size", &self.size)
            .finish()
    }
}

pub(crate) type PartialTimeSliceDetails =
    HashMap<u32, HashMap<u32, bytes::Bytes>>;

impl HashPartition {
    /// Create a new hash partition structure.
    ///
    /// The created structure has 512 sectors. This is currently not configurable. See the module
    /// documentation for more details.
    ///
    /// Each sector owns a [TimePartition] structure that is responsible for managing the time
    /// slices for that sector. The parameters to this function are used to create the
    /// [TimePartition] structure.
    #[tracing::instrument(level = "debug", skip(store))]
    pub async fn try_from_store(
        time_factor: u8,
        current_time: Timestamp,
        store: DynOpStore,
    ) -> K2Result<Self> {
        // We will always be one bucket short because u32::MAX is not a power of two. It is one less
        // than a power of two, so the last bucket is always one short.
        let num_partitions = (u32::MAX / SECTOR_SIZE) + 1;
        let mut sectors = Vec::with_capacity(num_partitions as usize);
        for i in 0..(num_partitions - 1) {
            sectors.push(
                TimePartition::try_from_store(
                    time_factor,
                    current_time,
                    DhtArc::Arc(
                        i * SECTOR_SIZE,
                        (i + 1).saturating_mul(SECTOR_SIZE) - 1,
                    ),
                    store.clone(),
                )
                .await?,
            );
        }

        // The last sector must be handled separately because it needs to include the
        // remainder of the hash space.
        sectors.push(
            TimePartition::try_from_store(
                time_factor,
                current_time,
                DhtArc::Arc((num_partitions - 1) * SECTOR_SIZE, u32::MAX),
                store.clone(),
            )
            .await?,
        );

        tracing::info!("Allocated [{}] sectors", sectors.len());

        Ok(Self {
            size: SECTOR_SIZE,
            sectors,
        })
    }

    /// Get the next update time of the inner time partitions.
    pub fn next_update_at(&self) -> Timestamp {
        // We know that a fixed number of partitions is always present,
        // so this is safe to unwrap here.
        self.sectors
            .first()
            .expect("Always at least one sector")
            .next_update_at()
    }

    /// Update the time partitions for each sector.
    pub async fn update(
        &mut self,
        store: DynOpStore,
        current_time: Timestamp,
    ) -> K2Result<()> {
        for partition in self.sectors.iter_mut() {
            partition.update(current_time, store.clone()).await?;
        }

        Ok(())
    }

    /// Inform the hash partition of ops that have been stored.
    ///
    /// The ops are placed into the right sector based on the location of each op. Then the updating
    /// of combined hashes is delegated to the inner [TimePartition]s.
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
            self.sectors[location as usize]
                .inform_ops_stored(ops, store.clone())
                .await?;
        }

        Ok(())
    }
}

// Query implementation
impl HashPartition {
    /// For a given sector index, return the DHT arc that the sector covers.
    ///
    /// This is actually stored on the [TimePartition] structure, so this function must find the
    /// relevant [TimePartition] and then return the arc constraint from that.
    pub(crate) fn dht_arc_for_sector_index(
        &self,
        sector_index: u32,
    ) -> K2Result<DhtArc> {
        let sector_index = sector_index as usize;
        if sector_index >= self.sectors.len() {
            return Err(K2Error::other("Sector index out of bounds"));
        }

        Ok(*self.sectors[sector_index].sector_constraint())
    }

    /// Get the time bounds for a full slice index.
    ///
    /// This is actually stored on the [TimePartition] structure, so this function must find the
    /// relevant [TimePartition] structure and then return the time bounds from that.
    ///
    /// Note that it doesn't matter which sector we choose because all sectors are updated in lock
    /// step by [HashPartition::update].
    pub(crate) fn time_bounds_for_full_slice_index(
        &self,
        slice_index: u64,
    ) -> K2Result<(Timestamp, Timestamp)> {
        self.sectors[0].time_bounds_for_full_slice_index(slice_index)
    }

    /// Get the time bounds for a partial slice index.
    ///
    /// This is actually stored on the [TimePartition] structure, so this function must find the
    /// relevant [TimePartition] and then return the time bounds from that.
    ///
    /// Note that it doesn't matter which sector we choose because all sectors are updated in lock
    /// step by [HashPartition::update].
    pub(crate) fn time_bounds_for_partial_slice_index(
        &self,
        slice_index: u32,
    ) -> K2Result<(Timestamp, Timestamp)> {
        self.sectors[0].time_bounds_for_partial_slice_index(slice_index)
    }

    /// Compute the disc top hash for the given arc set.
    ///
    /// Considering the hash space as a circle, with time represented outwards from the center, in
    /// each sector. This function requests the top hash of each sector, by asking the
    /// [TimePartition] to combine all its combined full time slices into a single hash. It works
    /// around the circle from 0, skipping any sectors that are not included in the `arc_set`,
    /// combining all the top hashes into a single hash.
    ///
    /// If there are no sectors included in the `arc_set`, then an empty hash is returned.
    ///
    /// Along with the disc top hash, the end timestamp of the last full time slice is returned.
    /// This should be used when comparing the disc top hash of one DHT model with that of another
    /// node, to ensure that both nodes are using a common reference point.
    pub(crate) async fn disc_top_hash(
        &self,
        arc_set: &ArcSet,
        store: DynOpStore,
    ) -> K2Result<(bytes::Bytes, Timestamp)> {
        let mut combined = bytes::BytesMut::new();
        for (sector_index, sector) in self.sectors.iter().enumerate() {
            if !arc_set.includes_sector_index(sector_index as u32) {
                continue;
            }

            let hash = sector.full_time_slice_top_hash(store.clone()).await?;
            if !hash.is_empty() {
                combine_hashes(&mut combined, hash);
            }
        }

        let timestamp = self.sectors[0].full_slice_end_timestamp();

        Ok((combined.freeze(), timestamp))
    }

    /// Computes a ring top hash for each ring.
    ///
    /// A ring is the collection of partial time slices at the same index, combined across each
    /// sector. This function retrieves the partial slice combined hashes for each sector in the
    /// arc set. It then combines the hashes for each partial time slice, working around the circle
    /// from 0 and skipping any sectors that are not included in the `arc_set`.
    ///
    /// Note that this function does not return a disc boundary. This means it MUST be used with
    /// [HashPartition::disc_top_hash] to ensure that the result from this function can be
    /// compared. No indexing or size information is returned with the ring hashes, so two sets of
    /// rings that are the same size may have different contents.
    pub(crate) fn ring_top_hashes(
        &self,
        arc_set: &ArcSet,
    ) -> Vec<bytes::Bytes> {
        let mut partials = Vec::with_capacity(arc_set.covered_sector_count());

        for (sector_index, sector) in self.sectors.iter().enumerate() {
            if !arc_set.includes_sector_index(sector_index as u32) {
                continue;
            }

            partials.push(sector.partial_slice_hashes().peekable());
        }

        let mut out = Vec::new();
        let mut combined = bytes::BytesMut::new();
        while partials.get_mut(0).and_then(|p| p.peek()).is_some() {
            combined.clear();
            for partial in &mut partials {
                if let Some(hash) = partial.next() {
                    if !hash.is_empty() {
                        combine_hashes(&mut combined, hash);

                        if combined.iter().all(|b| *b == 0) {
                            tracing::warn!("Blank combined hash, has the DHT model been informed about the same op(s) more than once?");
                        }
                    }
                }
            }
            out.push(combined.clone().freeze());
        }

        out
    }

    /// Compute the disc sector hashes for the given arc set.
    ///
    /// This function does a similar job to [HashPartition::disc_top_hash] but it does not
    /// combine the sector hashes. Instead, any sector that has a non-empty hash is returned in the
    /// hash set.
    ///
    /// Along with the sector hashes, the end timestamp of the last full time slice is returned.
    /// This should be used when comparing sector hashes of one DHT model with that of another node
    /// to ensure that both nodes are using a common reference point.
    pub(crate) async fn disc_sector_hashes(
        &self,
        arc_set: &ArcSet,
        store: DynOpStore,
    ) -> K2Result<(HashMap<u32, bytes::Bytes>, Timestamp)> {
        let mut out = HashMap::new();
        for (sector_index, sector) in self.sectors.iter().enumerate() {
            if !arc_set.includes_sector_index(sector_index as u32) {
                continue;
            }

            let hash = sector.full_time_slice_top_hash(store.clone()).await?;
            if !hash.is_empty() {
                out.insert(sector_index as u32, hash);
            }
        }

        let timestamp = self.sectors[0].full_slice_end_timestamp();

        Ok((out, timestamp))
    }

    /// Compute the disc sector details for the given arc set.
    ///
    /// Does a similar job to [HashPartition::disc_sector_hashes] but it returns the full time
    /// slice combined hashes for each sector that is both in the arc set and in the
    /// `sector_indices` input.
    ///
    /// Along with the sector detail hashes, the end timestamp of the last full time slice is
    /// returned. This should be used when comparing sector details hashes of one DHT model with
    /// that of another node to ensure that both nodes are using a common reference point.
    pub(crate) async fn disc_sector_sector_details(
        &self,
        arc_set: &ArcSet,
        sector_indices: Vec<u32>,
        store: DynOpStore,
    ) -> K2Result<(HashMap<u32, HashMap<u64, bytes::Bytes>>, Timestamp)> {
        let sectors_indices =
            sector_indices.into_iter().collect::<HashSet<_>>();

        let mut out = HashMap::new();

        for (sector_index, sector) in self.sectors.iter().enumerate() {
            if !arc_set.includes_sector_index(sector_index as u32)
                || !sectors_indices.contains(&(sector_index as u32))
            {
                continue;
            }

            out.insert(
                sector_index as u32,
                sector
                    .full_time_slice_hashes(store.clone())
                    .await?
                    .into_iter()
                    .collect(),
            );
        }

        let timestamp = self.sectors[0].full_slice_end_timestamp();

        Ok((out, timestamp))
    }

    /// Compute the ring details for the given arc set.
    ///
    /// Does a similar job to [HashPartition::ring_top_hashes] but it returns the partial time
    /// slice combined hashes for each ring sector that is both in the `arc_set` and in the
    /// `ring_indices`.
    ///
    /// Along with the ring details hashes, the end timestamp of the last full time slice is
    /// returned. This should be used when comparing ring details hashes of one DHT model with
    /// that of another node to ensure that both nodes are using a common reference point.
    pub(crate) fn ring_details(
        &self,
        arc_set: &ArcSet,
        ring_indices: Vec<u32>,
    ) -> K2Result<(PartialTimeSliceDetails, Timestamp)> {
        let mut out = HashMap::new();

        for (sector_index, sector) in self.sectors.iter().enumerate() {
            if !arc_set.includes_sector_index(sector_index as u32) {
                continue;
            }

            for ring_index in &ring_indices {
                let hash = sector.partial_slice_hash(*ring_index)?;

                // Important to capture that the ring didn't match even if the hash is empty, and
                // therefore we won't communicate this sector.
                let entry = out.entry(*ring_index).or_insert_with(HashMap::new);
                if !hash.is_empty() {
                    entry.insert(sector_index as u32, hash);
                }
            }
        }

        let timestamp = self.sectors[0].full_slice_end_timestamp();

        Ok((out, timestamp))
    }
}

#[cfg(test)]
impl HashPartition {
    /// Get the full slice end timestamp.
    pub fn full_slice_end_timestamp(&self) -> Timestamp {
        self.sectors[0].full_slice_end_timestamp()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test::test_store;
    use crate::UNIT_TIME;
    use kitsune2_api::{OpId, UNIX_TIMESTAMP};
    use kitsune2_core::factories::MemoryOp;
    use kitsune2_test_utils::enable_tracing;
    use std::time::Duration;

    #[tokio::test]
    async fn try_from_store() {
        enable_tracing();

        let store = test_store().await;
        let ph = HashPartition::try_from_store(14, Timestamp::now(), store)
            .await
            .unwrap();
        assert_eq!(512, ph.sectors.len());
        assert_eq!((u32::MAX / 512) + 1, ph.size);
        assert_eq!(
            &DhtArc::Arc(0, ph.size - 1),
            ph.sectors[0].sector_constraint()
        );
        assert_eq!(
            &DhtArc::Arc(511 * ph.size, u32::MAX),
            ph.sectors[511].sector_constraint()
        );
    }

    #[tokio::test]
    async fn covers_full_arc() {
        enable_tracing();

        let store = test_store().await;
        let ph = HashPartition::try_from_store(14, UNIX_TIMESTAMP, store)
            .await
            .unwrap();

        let mut start: u32 = 0;
        for i in 0..(ph.sectors.len() - 1) {
            let end = start.overflowing_add(ph.size).0;
            assert_eq!(
                DhtArc::Arc(start, end - 1),
                *ph.sectors[i].sector_constraint()
            );
            start = end;
        }

        assert_eq!(
            DhtArc::Arc(start, u32::MAX),
            *ph.sectors.last().unwrap().sector_constraint()
        );
    }

    #[tokio::test]
    async fn inform_ops_stored_in_full_slices() {
        enable_tracing();

        let store = test_store().await;
        let mut ph =
            HashPartition::try_from_store(14, Timestamp::now(), store.clone())
                .await
                .unwrap();

        let op_id_bytes_1 = bytes::Bytes::from_static(&[7, 0, 0, 0]);
        let op_id_bytes_2 = bytes::Bytes::from(ph.size.to_le_bytes().to_vec());
        ph.inform_ops_stored(
            store.clone(),
            vec![
                StoredOp {
                    op_id: OpId::from(op_id_bytes_1.clone()),
                    created_at: UNIX_TIMESTAMP,
                },
                StoredOp {
                    op_id: OpId::from(op_id_bytes_2.clone()),
                    created_at: UNIX_TIMESTAMP
                        + ph.sectors[0].full_slice_duration(),
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
        // Note that this is because we've stored at index 1, not that two hashes ended up in this
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

        let store = test_store().await;
        let mut ph =
            HashPartition::try_from_store(14, Timestamp::now(), store.clone())
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
                    created_at: ph.sectors[0].full_slice_end_timestamp(),
                },
                // Stored in the second time slice of the first space partition.
                StoredOp {
                    op_id: OpId::from(op_id_bytes_2.clone()),
                    created_at: ph.sectors[0].full_slice_end_timestamp()
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

        let partial_slice = &ph.sectors[0].partials()[0];
        assert_eq!(
            op_id_bytes_1,
            bytes::Bytes::from(partial_slice.hash().to_vec())
        );

        let partial_slice = &ph.sectors[1].partials()[1];
        assert_eq!(
            op_id_bytes_2,
            bytes::Bytes::from(partial_slice.hash().to_vec())
        );
    }

    #[tokio::test]
    async fn next_update_at_consistent() {
        enable_tracing();

        let store = test_store().await;
        let now = Timestamp::now();
        let ph = HashPartition::try_from_store(14, now, store.clone())
            .await
            .unwrap();

        let hashes_next_update_at = ph.next_update_at();
        assert!(hashes_next_update_at >= now);

        for h in ph.sectors {
            assert_eq!(hashes_next_update_at, h.next_update_at());
        }
    }

    #[tokio::test]
    async fn update_all() {
        enable_tracing();
        let store = test_store().await;
        let now = Timestamp::now();
        let mut ph = HashPartition::try_from_store(14, now, store.clone())
            .await
            .unwrap();

        assert_eq!(512, ph.sectors.len());

        for h in ph.sectors.iter() {
            let (start, _) = match h.sector_constraint() {
                DhtArc::Arc(s, e) => (s, e),
                _ => panic!("Expected an arc"),
            };
            store
                .process_incoming_ops(vec![MemoryOp::new(
                    now,
                    // Place the op within the current space partition
                    (start + 1).to_le_bytes().as_slice().to_vec(),
                )
                .into()])
                .await
                .unwrap();
        }

        // Check nothing is currently stored in the partials
        for h in &ph.sectors {
            for ps in h.partials() {
                assert!(ps.hash().is_empty())
            }
        }

        // Update with enough extra time to allocate a new partial over the current time
        ph.update(store, now + UNIT_TIME).await.unwrap();

        // Check that the partials have been updated
        for h in &ph.sectors {
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
