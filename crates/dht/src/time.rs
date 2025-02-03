//! Partition of time into slices.
//!
//! Provides an encapsulation for calculating time slices that is consistent when computed
//! by different nodes. This is used to group Kitsune2 op data into time slices for combined hashes.
//! Giving multiple nodes the same time slice boundaries allows them to compute the same combined
//! hashes, and therefore effectively communicate which of their time slices match and which do not.
//!
//! A time slice is defined to be an interval of time that is closed at the start and open at the
//! end. That is, an interval `[start, end)` where:
//! - `start` is included in the time slice (inclusive bound)
//! - `end` is not included in the time slice (exclusive bound)
//!
//! Time slices are partitioned into two types: full slices and partial slices. Full slices are
//! always of a fixed size, while partial slices are of varying sizes. Full slices occupy
//! historical time, while partial slices occupy recent time.
//!
//! The granularity of time slices is determined by the `factor` parameter. The factor determines
//! the size of time slices. Where 2^X means "2 raised to the power of X", the size of a full time
//! slice is 2^factor * [UNIT_TIME]. Partial time slices vary in size from
//! 2^(factor - 1) * [UNIT_TIME] down to 2^0 * [UNIT_TIME] (i.e. [UNIT_TIME]). There is some
//! amount of time left over that cannot be partitioned into smaller slices. This time cannot be
//! included in comparisons but has an upper bound of [UNIT_TIME].
//!
//! > Note: The factor is used with durations and the code needs to be able to do arithmetic with
//! > it, so the factor has a maximum value of 53. The factor also has a minimum value of 1,
//! > though values lower than 4 are effectively meaningless. Consider choosing a factor that
//! > results in a meaningful split between recent time and historical, full slices.
//!
//! Because recent time is expected to change more frequently, combined hashes for partial slices
//! are stored in memory. Full slices are stored in the Kitsune2 op store. Storage of full slice
//! combined hashes is required to be sparse because time starts at 0, so there will usually be
//! many empty slices before the first one with data.
//!
//! The algorithm for partitioning time is as follows:
//!   - Reserve a minimum amount of recent time as a sum of possible partial slice sizes.
//!   - Allocate as many full slices as possible.
//!   - Partition the remaining time into smaller slices. If there is space for two slices of a
//!     given size, then allocate two slices of that size. Otherwise, allocate one slice of that
//!     size. Larger slices are allocated before smaller slices.
//!
//! As time progresses, the partitioning needs to be updated. This is done by calling the
//! [TimePartition::update] method. The update method will:
//!    - Run the partitioning algorithm to determine whether new full slices can be allocated and
//!      how to partition the remaining time into partial slices.
//!    - Store the combined hash of any new full slices in the Kitsune2 op store.
//!    - Store the combined hash of any new partial slices in memory.
//!    - Update the `next_update_at` field to the next time an update is required.
//!
//! As an example, consider a factor of 9. This means that full slices are 2^9 = 512 times the [UNIT_TIME].
//! With a unit time of 15 minutes, that means full slices are 512 * 15 minutes = 7680 minutes = 128 hours.
//! So every 5 days, a new full slice is created.
//! The partial slices reserve at least 2^8 + 2^7 ... 2^0 = 511 times the [UNIT_TIME], so 511 * 15 minutes
//! = 7685 minutes = 127.75 hours. That gives roughly 5 days of recent time.
//!
//! A lower factor allocates less recent time and requires more full slices to be stored but is
//! more granular when comparing time slices with another peer. A higher factor allocates more
//! recent time and requires fewer full slices to be stored but is less granular when comparing
//! time slices with another peer.

use crate::combine;
use crate::constant::UNIT_TIME;
use kitsune2_api::{
    DhtArc, DynOpStore, K2Error, K2Result, StoredOp, Timestamp, UNIX_TIMESTAMP,
};
use std::time::Duration;

/// The time partition structure.
#[derive(Debug)]
#[cfg_attr(test, derive(Clone, PartialEq))]
pub struct TimePartition {
    /// The factor used to determine the size of the time slices.
    ///
    /// The size of a time slice is 2^factor * [UNIT_TIME].
    /// Partial slices are created for "recent time" and are created in decreasing sizes from
    /// 2^factor * [UNIT_TIME] down to 2^0 * [UNIT_TIME].
    factor: u8,
    /// The number of full time slices that have been stored.
    ///
    /// These full slices are always of size 2^factor * [UNIT_TIME].
    full_slices: u64,
    /// The partial slices, with hashes stored in memory.
    ///
    /// Because these change every [UNIT_TIME], they are stored in memory.
    partial_slices: Vec<PartialSlice>,
    /// The duration of a full slice.
    ///
    /// This is used regularly and is a constant based on [TimePartition::factor], so it is
    /// calculated at construction and stored. It is computed as 2^factor * [UNIT_TIME].
    full_slice_duration: Duration,
    /// The minimum amount of time that must be reserved for recent time.
    ///
    /// The algorithm is free to allocate up to 2x this value as recent time. This allows for the
    /// algorithm to create more slices that fill more of the recent time window.
    min_recent_time: Duration,
    /// The timestamp at which the next update is required.
    ///
    /// After this time, there will be an excess amount of time that could be allocated into a
    /// partial slice. The data structure is still usable but will get behind if
    /// [TimePartition::update] is not called.
    ///
    /// It is idempotent to call [TimePartition::update] more often than required, but it is not
    /// efficient.
    next_update_at: Timestamp,
    /// The arc bounds for the sector that this time partition is associated with.
    ///
    /// Any queries to fetch ops in a time range must be constrained to this sector.
    sector_constraint: DhtArc,
}

/// A slice of recent time that has a combined hash of all the ops in that time slice.
///
/// This is used to represent a slice of recent time that is not yet ready to be stored as a full
/// time slice.
#[derive(Debug)]
#[cfg_attr(test, derive(Clone, PartialEq))]
pub struct PartialSlice {
    /// The start timestamp of the time slice.
    ///
    /// This timestamp is included in the time slice.
    start: Timestamp,
    /// Size is used in the formula 2^size * [UNIT_TIME].
    ///
    /// That means a 0 size translates to [UNIT_TIME], and a 1 size translates to 2*[UNIT_TIME].
    size: u8,
    /// The combined hash over all the ops in this time slice.
    hash: bytes::BytesMut,
}

impl PartialSlice {
    /// The end timestamp of the time slice.
    ///
    /// This timestamp is not included in the time slice.
    fn end(&self) -> Timestamp {
        self.start
            + Duration::from_secs((1u64 << self.size) * UNIT_TIME.as_secs())
    }

    #[cfg(test)]
    pub(crate) fn hash(&self) -> &[u8] {
        &self.hash
    }
}

// Public methods
impl TimePartition {
    /// Create a new instance of [TimePartition] from the given store.
    ///
    /// The store is needed to check how many time slices were created last time this
    /// [TimePartition] was updated, if any.
    ///
    /// The method will then update the state of the [TimePartition] to the current time.
    /// It does this by checking that the construction of this [TimePartition] is consistent
    /// and then calling [TimePartition::update].
    ///
    /// The resulting [TimePartition] will be consistent with the store at the current time.
    /// It should be updated again after [TimePartition::next_update_at].
    pub async fn try_from_store(
        factor: u8,
        current_time: Timestamp,
        sector_constraint: DhtArc,
        store: DynOpStore,
    ) -> K2Result<Self> {
        if sector_constraint == DhtArc::Empty {
            return Err(K2Error::other("Empty arc constraint is not valid"));
        }

        let mut pt = Self::new(factor, sector_constraint)?;

        pt.full_slices = store.slice_hash_count(sector_constraint).await?;

        // The end timestamp of the last full slice
        let full_slice_end_timestamp = pt.full_slice_end_timestamp();

        // Given the time reserved by full slices, how much time is left to partition into smaller slices
        let recent_time = (current_time - full_slice_end_timestamp).map_err(|_| {
            K2Error::other("Failed to calculate recent time, either the clock is wrong or this is a bug")
        })?;

        if pt.full_slices > 0 && recent_time < pt.min_recent_time {
            return Err(K2Error::other("Not enough recent time reserved, either the clock is wrong or this is a bug"));
        }

        // Update the state for the current time. The stored slices might be out of date.
        pt.update(store, current_time).await?;

        Ok(pt)
    }

    /// The timestamp at which the next update is required.
    ///
    /// See the field [TimePartition::next_update_at] for more information.
    ///
    /// This value is updated by [TimePartition::update], so you can check the next update time
    /// after calling that method.
    pub fn next_update_at(&self) -> Timestamp {
        self.next_update_at
    }

    /// Update the state of the [TimePartition] to the current time.
    ///
    /// This method will:
    ///   - Check if there is space for any new full slices
    ///   - Store the combined hash of any new full slices
    ///   - Recompute the layout of partial slices. Each partial slice will have its hash computed
    ///     or re-used.
    ///   - Update the `next_update_at` field to the next time an update is required.
    pub async fn update(
        &mut self,
        store: DynOpStore,
        current_time: Timestamp,
    ) -> K2Result<()> {
        // Check if there is enough time to allocate new full slices
        self.update_full_slice_hashes(store.clone(), current_time)
            .await?;

        // Check if there is enough time to allocate new partial slices
        self.update_partials(current_time, store.clone()).await?;

        // There will be a small amount of time left over, which we can't partition into smaller
        // slices. Some amount less than [UNIT_TIME] will be left over.
        // Once that has elapsed up to the next [UNIT_TIME] boundary, we can update again.
        if let Some(last_partial) = self.partial_slices.last() {
            self.next_update_at = last_partial.end() + UNIT_TIME;
        }

        Ok(())
    }

    /// Inform the [TimePartition] that ops have been stored.
    ///
    /// The caller is required to ensure that the ops belong to the hash range managed by this
    /// [TimePartition]. This method will update the hashes of the full and partial slices that the
    /// incoming ops belong in.
    ///
    /// If the op happens to be new enough to not belong in a slice, then it will be ignored. The
    /// op will be discovered later when a [TimePartition::update] adds a slice that includes the
    /// op.
    pub(crate) async fn inform_ops_stored(
        &mut self,
        stored_ops: Vec<StoredOp>,
        store: DynOpStore,
    ) -> K2Result<()> {
        let full_slice_end = self.full_slice_end_timestamp();

        for op in stored_ops {
            if op.created_at < full_slice_end {
                // This is a historical update. We don't really expect this to happen too often.
                // If we're syncing because we've been offline then it's okay and we should
                // try to detect that when it's happening but otherwise it'd be good to log a warning
                // here.
                tracing::info!("Historical update detected. Seeing many of these places load on our system, but it is expected if we've been offline or a network partition has been resolved.");

                let slice_index = op.created_at.as_micros()
                    / (self.full_slice_duration.as_micros() as i64);
                let current_hash = store
                    .retrieve_slice_hash(
                        self.sector_constraint,
                        slice_index as u64,
                    )
                    .await?;
                match current_hash {
                    Some(hash) => {
                        let mut hash = bytes::BytesMut::from(hash);
                        // Combine the stored hash with the new op hash
                        combine::combine_hashes(&mut hash, op.op_id.0 .0);

                        // and store the new value
                        store
                            .store_slice_hash(
                                self.sector_constraint,
                                slice_index as u64,
                                hash.freeze(),
                            )
                            .await?;
                    }
                    None => {
                        // If there was no hash stored, then store the new op hash
                        store
                            .store_slice_hash(
                                self.sector_constraint,
                                slice_index as u64,
                                op.op_id.0 .0,
                            )
                            .await?;
                    }
                }
            } else {
                let end_of_partials = match self
                    .partial_slices
                    .last()
                    .map(|last| last.end())
                {
                    Some(end) => end,
                    None => {
                        // If there are no partial slices yet, we can't update anything here.
                        // This would only happen if the current time is close to the UNIX_TIMESTAMP.
                        tracing::warn!("No partial slices yet, can't update partials. This is likely a configuration or clock issue.");
                        continue;
                    }
                };

                if op.created_at >= end_of_partials {
                    // This new op is not yet included in the partial slices. That's okay, there
                    // is expected to be a small amount of recent time that isn't covered by
                    // partial slices. This op will get included in a future update of the partials.
                    continue;
                }

                // At this point, we know we're between the end of the full slices and the end of
                // the partial slices. We can easily iterate backwards and check just the start
                // bound of the partials to find out which partial slice this op belongs to.
                for partial in self.partial_slices.iter_mut().rev() {
                    if op.created_at >= partial.start {
                        combine::combine_hashes(
                            &mut partial.hash,
                            op.op_id.0 .0,
                        );

                        // Belongs in exactly one partial, stop after finding the right one.
                        break;
                    }
                }
            }
        }

        Ok(())
    }

    /// Get the time bounds for a given full slice by slice index.
    pub(crate) fn time_bounds_for_full_slice_index(
        &self,
        slice_index: u64,
    ) -> K2Result<(Timestamp, Timestamp)> {
        if slice_index > self.full_slices {
            return Err(K2Error::other(
                "Requested slice index is beyond the current full slices",
            ));
        }

        let start = UNIX_TIMESTAMP
            + Duration::from_secs(
                slice_index * self.full_slice_duration.as_secs(),
            );
        let end = start + self.full_slice_duration;

        Ok((start, end))
    }

    /// Get the time bounds for a given partial slice by slice index.
    pub(crate) fn time_bounds_for_partial_slice_index(
        &self,
        slice_index: u32,
    ) -> K2Result<(Timestamp, Timestamp)> {
        let slice_index = slice_index as usize;
        if slice_index > self.partial_slices.len() {
            return Err(K2Error::other(
                "Requested slice index is beyond the current partial slices",
            ));
        }

        let partial = &self.partial_slices[slice_index];
        Ok((partial.start, partial.end()))
    }
}

// Public query methods
impl TimePartition {
    /// Compute a top hash over the full time slice combined hashes owned by this [TimePartition].
    ///
    /// This method will fetch the hashes of all the full time slices within the sector constraint
    /// for this [TimePartition]. Those are expected to be ordered by slice index, which implies
    /// that they are ordered by time. It will then combine those hashes into a single hash.
    pub async fn full_time_slice_top_hash(
        &self,
        store: DynOpStore,
    ) -> K2Result<bytes::Bytes> {
        let hashes =
            store.retrieve_slice_hashes(self.sector_constraint).await?;
        Ok(
            combine::combine_op_hashes(
                hashes.into_iter().map(|(_, hash)| hash),
            )
            .freeze(),
        )
    }

    /// Get the combined hashes for each of the partial slices owned by this [TimePartition].
    ///
    /// This method takes the current partial slices and returns their pre-computed hashes.
    /// These are combined hashes over all the ops in each partial slice, ordered by time.
    pub fn partial_slice_hashes(
        &self,
    ) -> impl Iterator<Item = bytes::Bytes> + use<'_> {
        self.partial_slices
            .iter()
            .map(|partial| partial.hash.clone().freeze())
    }

    /// Gets the combined hashes of all the full time slices owned by this [TimePartition].
    ///
    /// This is a pass-through to the provided store, using the sector constraint of this
    /// [TimePartition].
    pub async fn full_time_slice_hashes(
        &self,
        store: DynOpStore,
    ) -> K2Result<Vec<(u64, bytes::Bytes)>> {
        store.retrieve_slice_hashes(self.sector_constraint).await
    }

    /// Get the combined hash of a partial slice by its slice index.
    ///
    /// Note that the number of partial slices changes over time and the start point of the partial
    /// slices moves. It is important that the slice index is only used to refer to a specific slice
    /// at a specific point in time. This can be achieved by not calling [TimePartition::update]
    /// while expecting the slice index to refer to the same slice.
    ///
    /// # Errors
    ///
    /// This method will return an error if the requested slice index is beyond the current partial
    /// slices.
    pub fn partial_slice_hash(
        &self,
        slice_index: u32,
    ) -> K2Result<bytes::Bytes> {
        let slice_index = slice_index as usize;
        if slice_index > self.partial_slices.len() {
            return Err(K2Error::other(
                "Requested slice index is beyond the current partial slices",
            ));
        }

        Ok(self.partial_slices[slice_index].hash.clone().freeze())
    }
}

// Private methods
impl TimePartition {
    /// Private constructor, see [TimePartition::try_from_store].
    ///
    /// This constructor just creates an instance with initial values, but it doesn't update the
    /// state with full and partial slices for the current time.
    fn new(factor: u8, sector_constraint: DhtArc) -> K2Result<Self> {
        Ok(Self {
            factor,
            full_slices: 0,
            partial_slices: Vec::new(),
            full_slice_duration: Duration::from_secs(
                (1u64 << factor) * UNIT_TIME.as_secs(),
            ),
            // This only changes based on the factor, which can't change at runtime,
            // so compute it on construction.
            // Note that `- 1` is because the first partial slice is half the size of a full slice,
            // so we actually want to calculate to `factor - 1`.
            min_recent_time: residual_duration_for_factor(factor - 1)?,
            // Immediately requires an update, any time in the past will do
            next_update_at: Timestamp::from_micros(0),
            sector_constraint,
        })
    }

    /// The timestamp at which the last full slice ends.
    ///
    /// If there are no full slices, then this is the constant [UNIX_TIMESTAMP].
    pub(crate) fn full_slice_end_timestamp(&self) -> Timestamp {
        let full_slices_duration = Duration::from_secs(
            self.full_slices * self.full_slice_duration.as_secs(),
        );
        UNIX_TIMESTAMP + full_slices_duration
    }

    /// The [DhtArc] that describes the sector constraint for this [TimePartition].
    pub(crate) fn sector_constraint(&self) -> &DhtArc {
        &self.sector_constraint
    }

    /// Figure out how many new full slices need to be allocated.
    ///
    /// This is done by checking how many full slices fit between the current end of the last
    /// full slice and the current time. While also accounting for the minimum recent time.
    fn layout_full_slices(&self, current_time: Timestamp) -> K2Result<u64> {
        let full_slices_end_timestamp = self.full_slice_end_timestamp();

        let recent_time =
            (current_time - full_slices_end_timestamp).map_err(|_| {
                K2Error::other(
                    "Current time is before the complete time slice boundary",
                )
            })?;

        // Check if there is enough time to allocate new full slices
        let new_full_slices_count = if recent_time > self.min_recent_time {
            // Rely on integer rounding to get the correct number of full slices
            // that need adding.
            (recent_time - self.min_recent_time).as_secs()
                / (self.full_slice_duration.as_secs())
        } else {
            0
        };

        Ok(new_full_slices_count)
    }

    /// Update full slice hashes for the current time.
    ///
    /// This method will use [TimePartition::layout_full_slices] to determine how many new full
    /// slices should be allocated. It will then fetch the op hashes for each full slice and
    /// combine them into a single hash. That combined hash is then stored on the host.
    async fn update_full_slice_hashes(
        &mut self,
        store: DynOpStore,
        current_time: Timestamp,
    ) -> K2Result<()> {
        let new_full_slices_count = self.layout_full_slices(current_time)?;

        let mut full_slices_end_timestamp = self.full_slice_end_timestamp();
        for _ in 0..new_full_slices_count {
            // Store the hash of the full slice
            let op_hashes = store
                .retrieve_op_hashes_in_time_slice(
                    self.sector_constraint,
                    full_slices_end_timestamp,
                    full_slices_end_timestamp + self.full_slice_duration,
                    None,
                )
                .await?
                .0;

            let hash = combine::combine_op_hashes(op_hashes);

            if !hash.is_empty() {
                store
                    .store_slice_hash(
                        self.sector_constraint,
                        self.full_slices,
                        hash.freeze(),
                    )
                    .await?;
            }

            self.full_slices += 1;
            full_slices_end_timestamp += self.full_slice_duration;
        }

        Ok(())
    }

    /// Layout the partial slices for the given time range.
    ///
    /// Tries to allocate as many large slices as possible to fill the space.
    fn layout_partials(
        &self,
        mut start_at: Timestamp,
        current_time: Timestamp,
    ) -> K2Result<Vec<(Timestamp, u8)>> {
        let mut recent_time = (current_time - start_at).map_err(|_| {
            K2Error::other("Failed to calculate recent time for partials, either the clock is wrong or this is a bug")
        })?;

        let mut partials = Vec::new();

        // Now we want to partition the remaining time into smaller slices
        for i in (0..self.factor).rev() {
            // Starting from the largest slice size, if there's space for two of that slice size
            // then add two slices of that size, otherwise add one slice of that size
            let slice_size =
                Duration::from_secs((1u64 << i) * UNIT_TIME.as_secs());
            let slice_count = if recent_time
                > residual_duration_for_factor(i)? + slice_size
            {
                2
            } else if recent_time > slice_size {
                1
            } else {
                continue;
            };

            for _ in 0..slice_count {
                partials.push((start_at, i));
                start_at += slice_size;
                recent_time -= slice_size;
            }
        }

        Ok(partials)
    }

    /// Update the partial slices at the current time.
    ///
    /// This method will use [TimePartition::layout_partials] to determine how to partition
    /// recent time into partial slices. It will then fetch the op hashes for each partial slice
    /// and combine them into a single hash. That combined hash is then stored in memory.
    ///
    /// There is an optimization here to re-use the combined hash if the slice hasn't changed.
    /// That makes the function slightly cheaper to call, when we expect most partial slices to be
    /// stable, especially the larger ones that require more ops to be fetched.
    async fn update_partials(
        &mut self,
        current_time: Timestamp,
        store: DynOpStore,
    ) -> K2Result<()> {
        let full_slices_end_timestamp = self.full_slice_end_timestamp();
        let new_partials =
            self.layout_partials(full_slices_end_timestamp, current_time)?;
        let old_partials = std::mem::take(&mut self.partial_slices);

        for (start, size) in new_partials.into_iter() {
            // If this slice didn't change, then we can reuse the hash
            let maybe_old_hash = old_partials.iter().find_map(|b| {
                if b.start == start && b.size == size {
                    Some(b.hash.clone())
                } else {
                    None
                }
            });

            // Otherwise, we need to get the op hashes for this time slice and combine them
            let hash = match maybe_old_hash {
                Some(h) => h,
                None => {
                    let end = start
                        + Duration::from_secs(
                            (1u64 << size) * UNIT_TIME.as_secs(),
                        );
                    combine::combine_op_hashes(
                        store
                            .retrieve_op_hashes_in_time_slice(
                                self.sector_constraint,
                                start,
                                end,
                                None,
                            )
                            .await?
                            .0,
                    )
                }
            };

            self.partial_slices.push(PartialSlice { start, size, hash })
        }

        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn full_slice_duration(&self) -> Duration {
        self.full_slice_duration
    }

    #[cfg(test)]
    pub(crate) fn partials(&self) -> &[PartialSlice] {
        &self.partial_slices
    }
}

/// Computes what duration is required for a series of time slices.
///
/// The duration is computed as the sum of the powers of two from 0 to the factor, multiplied by
/// the [UNIT_TIME].
///
/// Returns an error if the factor is 54 or higher, as that would overflow the duration.
/// Note that the maximum factor changes if the [UNIT_TIME] is changed.
fn residual_duration_for_factor(factor: u8) -> K2Result<Duration> {
    if factor >= 54 {
        return Err(K2Error::other(
            "Time partitioning factor must be less than 54",
        ));
    }

    let mut sum = 1;
    for i in 1..=factor {
        sum |= 1u64 << i;
    }

    Ok(Duration::from_secs(sum * UNIT_TIME.as_secs()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test::test_store;
    use kitsune2_api::OpId;
    use kitsune2_core::factories::MemoryOp;
    use kitsune2_test_utils::enable_tracing;

    #[test]
    fn residual() {
        assert_eq!(UNIT_TIME, residual_duration_for_factor(0).unwrap());
        assert_eq!(
            (2 + 1) * UNIT_TIME,
            residual_duration_for_factor(1).unwrap()
        );
        assert_eq!(
            (4 + 2 + 1) * UNIT_TIME,
            residual_duration_for_factor(2).unwrap()
        );
        assert_eq!(
            (32 + 16 + 8 + 4 + 2 + 1) * UNIT_TIME,
            residual_duration_for_factor(5).unwrap()
        );

        let mask = 0b1111111111000000u16;
        assert_eq!(
            Duration::from_secs(!((mask as u64) << 48) * UNIT_TIME.as_secs()),
            residual_duration_for_factor(53).unwrap()
        );
    }

    #[should_panic(expected = "Time partitioning factor must be less than 54")]
    #[test]
    fn max_residual() {
        residual_duration_for_factor(54)
            .map_err(|e| e.to_string())
            .unwrap();
    }

    #[test]
    fn new() {
        let factor = 4;
        let pt = TimePartition::new(factor, DhtArc::FULL).unwrap();

        // Full slices would have size 2^4 = 16, so we should reserve space for at least one
        // of each smaller slice size
        assert_eq!((8 + 4 + 2 + 1) * UNIT_TIME, pt.min_recent_time);
    }

    #[tokio::test]
    async fn from_store() {
        let factor = 4;
        let store = test_store().await;
        let pt = TimePartition::try_from_store(
            factor,
            UNIX_TIMESTAMP,
            DhtArc::FULL,
            store,
        )
        .await
        .unwrap();

        assert_eq!(0, pt.full_slices);
        assert!(pt.partial_slices.is_empty());
    }

    #[tokio::test]
    async fn one_partial_slice() {
        let current_time =
            UNIX_TIMESTAMP + Duration::from_secs(UNIT_TIME.as_secs() + 1);
        let factor = 4;
        let store = test_store().await;
        let pt = TimePartition::try_from_store(
            factor,
            current_time,
            DhtArc::FULL,
            store,
        )
        .await
        .unwrap();

        // Should allocate no full slices and one partial
        assert_eq!(0, pt.full_slices);
        assert_eq!(1, pt.partial_slices.len());

        // The partial should be:
        //   - The minimum factor size, and
        //   - start at the UNIX_TIMESTAMP
        //   - end at the UNIX_TIMESTAMP + UNIT_TIME
        assert_eq!(0, pt.partial_slices[0].size);
        assert_eq!(UNIX_TIMESTAMP, pt.partial_slices[0].start);
        assert_eq!(UNIX_TIMESTAMP + UNIT_TIME, pt.partial_slices[0].end());

        // The next required update should be at the end of the partial slice
        // plus another UNIT_TIME.
        assert_eq!(UNIX_TIMESTAMP + 2 * UNIT_TIME, pt.next_update_at);
    }

    #[tokio::test]
    async fn all_single_partial_slices() {
        let factor = 7;
        let current_time = UNIX_TIMESTAMP
            + Duration::from_secs(
                // Enough time for all the partial slices
                min_recent_time(factor).as_secs() + 1,
            );
        let store = test_store().await;
        let pt = TimePartition::try_from_store(
            factor,
            current_time,
            DhtArc::FULL,
            store,
        )
        .await
        .unwrap();

        assert_eq!(0, pt.full_slices);
        assert_eq!(factor as usize, pt.partial_slices.len());

        validate_partial_slices(&pt);
    }

    #[tokio::test]
    async fn one_double_others_single_partial_slices() {
        let factor = 7;
        let current_time = UNIX_TIMESTAMP
            + Duration::from_secs(
                min_recent_time(factor).as_secs() +
                // Add enough time to reserve a double slice in the first spot
            (1u64 << (factor - 1)) * UNIT_TIME.as_secs()
                + 1,
            );
        let store = test_store().await;
        let pt = TimePartition::try_from_store(
            factor,
            current_time,
            DhtArc::FULL,
            store,
        )
        .await
        .unwrap();

        assert_eq!(factor as usize + 1, pt.partial_slices.len());
        assert_eq!(pt.partial_slices[0].size, factor - 1);
        assert_eq!(pt.partial_slices[0].size, pt.partial_slices[1].size);

        validate_partial_slices(&pt);
    }

    #[tokio::test]
    async fn all_double_slices() {
        let factor = 7;
        let current_time = UNIX_TIMESTAMP
            + Duration::from_secs(
                // Enough time for two of each of the partial slices
                2 * min_recent_time(factor).as_secs() + 1,
            );
        let store = test_store().await;
        let pt = TimePartition::try_from_store(
            factor,
            current_time,
            DhtArc::FULL,
            store,
        )
        .await
        .unwrap();

        assert_eq!(factor as usize * 2, pt.partial_slices.len());

        validate_partial_slices(&pt);
    }

    #[tokio::test]
    async fn hashes_are_combined_for_partial_slices() {
        let factor = 7;
        let current_time = UNIX_TIMESTAMP
            + Duration::from_secs(
                // Enough time for all the partial slices
                min_recent_time(factor).as_secs() + 1,
            );

        let store = test_store().await;
        store
            .process_incoming_ops(vec![
                MemoryOp::new((current_time - UNIT_TIME).unwrap(), vec![7; 32])
                    .into(),
                MemoryOp::new(
                    (current_time - UNIT_TIME).unwrap(),
                    vec![23; 32],
                )
                .into(),
            ])
            .await
            .unwrap();

        let pt = TimePartition::try_from_store(
            factor,
            current_time,
            DhtArc::FULL,
            store,
        )
        .await
        .unwrap();

        assert_eq!(factor as usize, pt.partial_slices.len());
        let last_partial = pt.partial_slices.last().unwrap();
        assert_eq!(vec![7 ^ 23; 32], last_partial.hash);

        validate_partial_slices(&pt);
    }

    #[tokio::test]
    async fn full_slices_with_single_slice_partials() {
        let factor = 7;
        let current_time = UNIX_TIMESTAMP
            + Duration::from_secs(
                // Two full slices
                2 * full_slice_duration(factor).as_secs() +
                // Enough time remaining for recent time
                min_recent_time(factor)
                .as_secs()
                + 1,
            );
        let store = test_store().await;
        let arc_constraint = DhtArc::Arc(0, 2);
        store
            .store_slice_hash(arc_constraint, 0, vec![1; 64].into())
            .await
            .unwrap();
        store
            .store_slice_hash(arc_constraint, 1, vec![1; 64].into())
            .await
            .unwrap();

        let pt = TimePartition::try_from_store(
            factor,
            current_time,
            arc_constraint,
            store,
        )
        .await
        .unwrap();

        assert_eq!(2, pt.full_slices);
        assert_eq!(factor as usize, pt.partial_slices.len());

        validate_partial_slices(&pt);
    }

    #[tokio::test]
    async fn missing_full_slices_combines_hashes() {
        let factor = 7;
        let current_time = UNIX_TIMESTAMP
            + Duration::from_secs(
                // One full slice
                full_slice_duration(factor).as_secs() +
                // Enough time remaining for all the single partial slices
                min_recent_time(factor)
                    .as_secs()
                + 1,
            );

        // Store with no full slices stored
        let store = test_store().await;
        store
            .process_incoming_ops(vec![
                MemoryOp::new(UNIX_TIMESTAMP, vec![7; 32]).into(),
                MemoryOp::new(UNIX_TIMESTAMP, vec![23; 32]).into(),
            ])
            .await
            .unwrap();

        let arc_constraint = DhtArc::Arc(0, 2);
        let pt = TimePartition::try_from_store(
            factor,
            current_time,
            arc_constraint,
            store.clone(),
        )
        .await
        .unwrap();

        // One full slice and all the partial slices should be created
        assert_eq!(1, pt.full_slices);
        assert_eq!(factor as usize, pt.partial_slices.len());

        // The full slice should have been stored
        assert_eq!(1, store.slice_hash_count(arc_constraint).await.unwrap());
        let full_slice_hash = store
            .retrieve_slice_hash(arc_constraint, 0)
            .await
            .unwrap()
            .unwrap();
        // The hashes should be combined using XOR
        assert_eq!(vec![7 ^ 23; 32], full_slice_hash);

        validate_partial_slices(&pt);
    }

    #[tokio::test]
    async fn compute_hashes_for_full_slices_and_partials() {
        let factor = 7;
        let current_time = UNIX_TIMESTAMP
            + Duration::from_secs(
                // One full slice
                full_slice_duration(factor).as_secs() +
                // Enough time remaining for all the single partial slices
                min_recent_time(factor)
                    .as_secs()
                + 1,
            );

        // Store with no full slices stored
        let store = test_store().await;
        store
            .process_incoming_ops(vec![
                MemoryOp::new(UNIX_TIMESTAMP, vec![7; 32]).into(),
                MemoryOp::new(UNIX_TIMESTAMP, vec![23; 32]).into(),
                MemoryOp::new(
                    (current_time - UNIT_TIME).unwrap(),
                    vec![11; 32],
                )
                .into(),
                MemoryOp::new(
                    (current_time - UNIT_TIME).unwrap(),
                    vec![29; 32],
                )
                .into(),
            ])
            .await
            .unwrap();

        let arc_constraint = DhtArc::Arc(0, 2);
        let pt = TimePartition::try_from_store(
            factor,
            current_time,
            arc_constraint,
            store.clone(),
        )
        .await
        .unwrap();

        // One full slice and all the partial slices should be created
        assert_eq!(1, pt.full_slices);
        assert_eq!(factor as usize, pt.partial_slices.len());

        // The full slice should have been stored
        assert_eq!(1, store.slice_hash_count(arc_constraint).await.unwrap());
        let full_slice_hash = store
            .retrieve_slice_hash(arc_constraint, 0)
            .await
            .unwrap()
            .unwrap();
        // The hashes should be combined using XOR
        assert_eq!(vec![7 ^ 23; 32], full_slice_hash);

        // The last partial slice should have the combined hash of the two ops
        let last_partial = pt.partial_slices.last().unwrap();
        assert_eq!(vec![11 ^ 29; 32], last_partial.hash);

        validate_partial_slices(&pt);
    }

    #[tokio::test]
    async fn update_is_idempotent() {
        let factor = 7;
        let current_time = UNIX_TIMESTAMP
            + Duration::from_secs(
                // One full slice
                full_slice_duration(factor).as_secs() +
                // Enough time remaining for all the single partial slices
                min_recent_time(factor)
                    .as_secs()
                + 1,
            );

        let store = test_store().await;
        store
            .process_incoming_ops(vec![
                MemoryOp::new(UNIX_TIMESTAMP, vec![7; 32]).into(),
                MemoryOp::new(
                    (current_time - UNIT_TIME).unwrap(),
                    vec![11; 32],
                )
                .into(),
            ])
            .await
            .unwrap();

        let mut pt = TimePartition::try_from_store(
            factor,
            current_time,
            DhtArc::FULL,
            store.clone(),
        )
        .await
        .unwrap();

        // Capture the initially created state
        let pt_original = pt.clone();

        // Update repeatedly, the state should not change
        for _ in 1..10 {
            let call_at =
                current_time + Duration::from_secs(UNIT_TIME.as_secs() / 100);
            assert!(call_at < pt.next_update_at());

            pt.update(store.clone(), call_at).await.unwrap();

            assert_eq!(pt_original, pt);
        }
    }

    #[tokio::test]
    async fn update_allocate_new_partial() {
        let factor = 7;
        let current_time = UNIX_TIMESTAMP
            + Duration::from_secs(
                // One full slice
                full_slice_duration(factor).as_secs() +
                // Enough time remaining for all the single partial slices
                min_recent_time(factor)
                .as_secs()
                + 1,
            );

        let store = test_store().await;
        let arc_constraint = DhtArc::Arc(0, 2);
        store
            .store_slice_hash(arc_constraint, 0, vec![1; 64].into())
            .await
            .unwrap();
        store
            .process_incoming_ops(vec![
                MemoryOp::new((current_time - UNIT_TIME).unwrap(), vec![7; 32])
                    .into(),
                MemoryOp::new(
                    (current_time - UNIT_TIME).unwrap(),
                    vec![29; 32],
                )
                .into(),
            ])
            .await
            .unwrap();

        let mut pt = TimePartition::try_from_store(
            factor,
            current_time,
            arc_constraint,
            store.clone(),
        )
        .await
        .unwrap();

        assert_eq!(factor as usize, pt.partial_slices.len());
        let last_partial = pt.partial_slices.last().unwrap();
        assert_eq!(vec![7 ^ 29; 32], last_partial.hash);

        // Store a new op which will currently be outside the last partial slice
        store
            .process_incoming_ops(vec![MemoryOp::new(
                current_time,
                vec![13; 32],
            )
            .into()])
            .await
            .unwrap();

        pt.update(store.clone(), current_time + UNIT_TIME)
            .await
            .unwrap();

        assert_eq!(factor as usize + 1, pt.partial_slices.len());

        let second_last_partial = pt.partial_slices.iter().nth_back(1).unwrap();
        assert_eq!(vec![7 ^ 29; 32], second_last_partial.hash);

        let last_partial = pt.partial_slices.last().unwrap();
        assert_eq!(vec![13; 32], last_partial.hash);
    }

    #[tokio::test]
    async fn update_allocate_new_complete() {
        let factor = 7;
        let current_times = UNIX_TIMESTAMP
            + Duration::from_secs(
                // One full slice
                full_slice_duration(factor).as_secs() +
                // Enough time remaining for all the single partial slices
                min_recent_time(factor)
                    .as_secs()
                + 1,
            );

        let store = test_store().await;
        store
            .process_incoming_ops(vec![
                MemoryOp::new(UNIX_TIMESTAMP, vec![7; 32]).into(),
                MemoryOp::new(UNIX_TIMESTAMP, vec![23; 32]).into(),
            ])
            .await
            .unwrap();

        let arc_constraint = DhtArc::Arc(0, 2);
        let mut pt = TimePartition::try_from_store(
            factor,
            current_times,
            arc_constraint,
            store.clone(),
        )
        .await
        .unwrap();

        assert_eq!(1, pt.full_slices);
        assert_eq!(
            vec![7 ^ 23; 32],
            store
                .retrieve_slice_hash(arc_constraint, 0)
                .await
                .unwrap()
                .unwrap()
        );

        // Store a new op, currently in the first partial slice, but will be in the next full slice.
        store
            .process_incoming_ops(vec![MemoryOp::new(
                pt.full_slice_end_timestamp(), // Start of the next full slice
                vec![13; 32],
            )
            .into()])
            .await
            .unwrap();

        pt.update(
            store.clone(),
            UNIX_TIMESTAMP
                + 2 * full_slice_duration(factor)
                + min_recent_time(factor)
                + Duration::from_secs(1),
        )
        .await
        .unwrap();

        assert_eq!(2, pt.full_slices);
        assert_eq!(factor as usize, pt.partial_slices.len());

        assert_eq!(2, store.slice_hash_count(arc_constraint).await.unwrap());
        assert_eq!(
            vec![7 ^ 23; 32],
            store
                .retrieve_slice_hash(arc_constraint, 0)
                .await
                .unwrap()
                .unwrap()
        );
        assert_eq!(
            vec![13; 32],
            store
                .retrieve_slice_hash(arc_constraint, 1)
                .await
                .unwrap()
                .unwrap()
        );
    }

    #[tokio::test]
    async fn update_allocate_multiple_new_complete() {
        let factor = 7;
        let current_time = UNIX_TIMESTAMP
            + Duration::from_secs(min_recent_time(factor).as_secs() + 1);

        let store = test_store().await;

        let arc_constraint = DhtArc::Arc(0, 2);
        let mut pt = TimePartition::try_from_store(
            factor,
            current_time,
            arc_constraint,
            store.clone(),
        )
        .await
        .unwrap();

        assert_eq!(0, pt.full_slices);

        store
            .process_incoming_ops(vec![
                // Store two new ops at the unix timestamp, to go into the first complete slice
                MemoryOp::new(UNIX_TIMESTAMP, vec![7; 32]).into(),
                MemoryOp::new(UNIX_TIMESTAMP, vec![23; 32]).into(),
                // Store two new ops at the unix timestamp plus one full time slice,
                // to go into the second complete slice
                MemoryOp::new(
                    UNIX_TIMESTAMP + pt.full_slice_duration,
                    vec![11; 32],
                )
                .into(),
                MemoryOp::new(
                    UNIX_TIMESTAMP + pt.full_slice_duration,
                    vec![37; 32],
                )
                .into(),
            ])
            .await
            .unwrap();

        pt.update(store.clone(), current_time + (2 * pt.full_slice_duration))
            .await
            .unwrap();

        assert_eq!(2, pt.full_slices);
        assert_eq!(factor as usize, pt.partial_slices.len());

        assert_eq!(2, store.slice_hash_count(arc_constraint).await.unwrap());
        assert_eq!(
            vec![7 ^ 23; 32],
            store
                .retrieve_slice_hash(arc_constraint, 0)
                .await
                .unwrap()
                .unwrap()
        );
        assert_eq!(
            vec![11 ^ 37; 32],
            store
                .retrieve_slice_hash(arc_constraint, 1)
                .await
                .unwrap()
                .unwrap()
        );
    }

    #[tokio::test]
    async fn compression_all_empty_slices_at_current_time() {
        enable_tracing();

        let factor = 7;
        let current_time = Timestamp::now();
        let store = test_store().await;

        let arc_constraint = DhtArc::FULL;
        let mut pt = TimePartition::try_from_store(
            factor,
            current_time,
            arc_constraint,
            store.clone(),
        )
        .await
        .unwrap();
        let initial_full_slices_count = pt.full_slices;

        // Quick calculation of how many full slices we can fit into the current time, leaving space
        // for the minimum recent time.
        // Something like 15k full slices at the time of writing.
        assert_eq!(
            (current_time.as_micros() as u128 - pt.min_recent_time.as_micros())
                / (pt.full_slice_duration.as_micros()),
            pt.full_slices as u128
        );

        let slice_hash_count =
            store.slice_hash_count(arc_constraint).await.unwrap();
        // Should be nothing stored, because we haven't ever created any data.
        assert_eq!(0, slice_hash_count);

        // Getting some partial slice that doesn't have any data stored will just return `None`
        let some_slice_hash = store
            .retrieve_slice_hash(arc_constraint, 5203984823)
            .await
            .unwrap();
        assert!(some_slice_hash.is_none());

        // Now insert an op at the current time
        store
            .process_incoming_ops(vec![MemoryOp::new(
                pt.full_slice_end_timestamp(),
                vec![7; 32],
            )
            .into()])
            .await
            .unwrap();
        // and compute the new state in the future
        pt.update(store.clone(), Timestamp::now() + pt.full_slice_duration)
            .await
            .unwrap();

        // Then a single full slice should have been created at the current time
        assert!(store.slice_hash_count(arc_constraint).await.unwrap() > 15_000);
        // and the count should match the number of full slices that the time partition claims to
        // have created.
        assert_eq!(
            store.slice_hash_count(arc_constraint).await.unwrap(),
            initial_full_slices_count + 1
        );
    }

    #[tokio::test]
    async fn inform_ops_stored_for_empty_full_slice() {
        enable_tracing();

        let factor = 14;
        let current_time = Timestamp::now();
        let store = test_store().await;

        let arc_constraint = DhtArc::Arc(0, 32);
        let mut pt = TimePartition::try_from_store(
            factor,
            current_time,
            arc_constraint,
            store.clone(),
        )
        .await
        .unwrap();

        let initial_hash =
            store.retrieve_slice_hash(arc_constraint, 0).await.unwrap();
        assert!(initial_hash.is_none());

        // Receive an op into the first time slice
        pt.inform_ops_stored(
            vec![StoredOp {
                op_id: OpId::from(bytes::Bytes::copy_from_slice(&[
                    11, 0, 0, 0,
                ])),
                created_at: UNIX_TIMESTAMP,
            }],
            store.clone(),
        )
        .await
        .unwrap();

        // The hash should be updated to include the new op
        let updated_hash = store
            .retrieve_slice_hash(arc_constraint, 0)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(vec![11, 0, 0, 0], updated_hash);
    }

    #[tokio::test]
    async fn inform_ops_stored_for_existing_full_slice() {
        enable_tracing();

        let factor = 14;
        let current_time = Timestamp::now();
        let store = test_store().await;
        // Insert a single op in the first time slice
        store
            .process_incoming_ops(vec![MemoryOp::new(
                UNIX_TIMESTAMP,
                vec![7, 0, 0, 0],
            )
            .into()])
            .await
            .unwrap();

        let arc_constraint = DhtArc::Arc(0, 32);
        let mut pt = TimePartition::try_from_store(
            factor,
            current_time,
            arc_constraint,
            store.clone(),
        )
        .await
        .unwrap();

        let initial_hash = store
            .retrieve_slice_hash(arc_constraint, 0)
            .await
            .unwrap()
            .unwrap();
        let mut expected = vec![7];
        expected.resize(32, 0);
        assert_eq!(expected, initial_hash);

        // Receive a new op into the same time slice
        let mut inner_op_id = vec![23];
        inner_op_id.resize(32, 0);
        pt.inform_ops_stored(
            vec![StoredOp {
                op_id: OpId::from(bytes::Bytes::from(inner_op_id)),
                created_at: UNIX_TIMESTAMP,
            }],
            store.clone(),
        )
        .await
        .unwrap();

        // The hash should be updated to include the new op
        let updated_hash = store
            .retrieve_slice_hash(arc_constraint, 0)
            .await
            .unwrap()
            .unwrap();
        let mut expected = vec![7 ^ 23];
        expected.resize(32, 0);
        assert_eq!(expected, updated_hash);
    }

    #[tokio::test]
    async fn inform_ops_stored_for_empty_partial_slice() {
        enable_tracing();

        let factor = 14;
        let current_time =
            UNIX_TIMESTAMP + min_recent_time(factor) + Duration::from_secs(3);
        let store = test_store().await;

        let arc_constraint = DhtArc::Arc(0, 32);
        let mut pt = TimePartition::try_from_store(
            factor,
            current_time,
            arc_constraint,
            store.clone(),
        )
        .await
        .unwrap();

        // Ensure that all the partial slices are empty
        assert!(pt.partial_slices.iter().all(|slice| slice.hash.is_empty()));

        // Receive an op into the first and last time slices
        pt.inform_ops_stored(
            vec![
                StoredOp {
                    op_id: OpId::from(bytes::Bytes::copy_from_slice(&[
                        11, 0, 0, 0,
                    ])),
                    created_at: UNIX_TIMESTAMP,
                },
                StoredOp {
                    op_id: OpId::from(bytes::Bytes::copy_from_slice(&[
                        29, 0, 0, 0,
                    ])),
                    created_at: (current_time - Duration::from_secs(5))
                        .unwrap(),
                },
            ],
            store.clone(),
        )
        .await
        .unwrap();

        assert_eq!(vec![11, 0, 0, 0], pt.partial_slices.first().unwrap().hash);
        assert_eq!(vec![29, 0, 0, 0], pt.partial_slices.last().unwrap().hash);
    }

    #[tokio::test]
    async fn inform_ops_stored_for_existing_partial_slice() {
        enable_tracing();

        let factor = 14;
        let current_time =
            UNIX_TIMESTAMP + min_recent_time(factor) + Duration::from_secs(3);
        let store = test_store().await;
        store
            .process_incoming_ops(vec![
                MemoryOp::new(
                    UNIX_TIMESTAMP + Duration::from_secs(10),
                    vec![7, 0, 0, 0],
                )
                .into(),
                MemoryOp::new(
                    (current_time - Duration::from_secs(30)).unwrap(),
                    vec![31, 0, 0, 0],
                )
                .into(),
            ])
            .await
            .unwrap();

        let arc_constraint = DhtArc::Arc(0, 32);
        let mut pt = TimePartition::try_from_store(
            factor,
            current_time,
            arc_constraint,
            store.clone(),
        )
        .await
        .unwrap();

        let mut expected = vec![7];
        expected.resize(32, 0);
        assert_eq!(expected, pt.partial_slices.first().unwrap().hash);
        let mut expected = vec![31];
        expected.resize(32, 0);
        assert_eq!(expected, pt.partial_slices.last().unwrap().hash);

        // Receive an op into the first and last time slices
        let mut inner_op_id_1 = vec![11];
        inner_op_id_1.resize(32, 0);
        let mut inner_op_id_2 = vec![29];
        inner_op_id_2.resize(32, 0);
        pt.inform_ops_stored(
            vec![
                StoredOp {
                    op_id: OpId::from(bytes::Bytes::from(inner_op_id_1)),
                    created_at: UNIX_TIMESTAMP,
                },
                StoredOp {
                    op_id: OpId::from(bytes::Bytes::from(inner_op_id_2)),
                    created_at: (current_time - Duration::from_secs(5))
                        .unwrap(),
                },
            ],
            store.clone(),
        )
        .await
        .unwrap();

        let mut expected = vec![7 ^ 11];
        expected.resize(32, 0);
        assert_eq!(expected, pt.partial_slices.first().unwrap().hash);
        let mut expected = vec![31 ^ 29];
        expected.resize(32, 0);
        assert_eq!(expected, pt.partial_slices.last().unwrap().hash);
    }

    fn validate_partial_slices(pt: &TimePartition) {
        let mut start_at = UNIX_TIMESTAMP
            + Duration::from_secs(
                pt.full_slices * full_slice_duration(pt.factor).as_secs(),
            );
        for (i, slice) in pt.partial_slices.iter().enumerate() {
            if i > 0 {
                // Require that the slice sizes are decreasing
                // Not that the decrease is not strictly monotonic, as up to two slices of the same
                // size are permitted
                assert!(
                    pt.partial_slices[i - 1].size >= slice.size,
                    "factor must decrease"
                );
            }
            if i > 1 {
                // Two is fine, if there are 3 of one size then they should have been collapsed before
                // the partial slices can be considered in a valid state.
                assert!(
                    pt.partial_slices[i - 2].size
                        != pt.partial_slices[i - 1].size
                        || pt.partial_slices[i - 1].size != slice.size,
                    "no more than two slices of the same size are permitted"
                );
            }

            // Check that the slices start at the correct time.
            assert_eq!(start_at, slice.start);

            start_at +=
                Duration::from_secs((1u64 << slice.size) * UNIT_TIME.as_secs());
        }
    }

    fn min_recent_time(factor: u8) -> Duration {
        TimePartition::new(factor, DhtArc::FULL)
            .unwrap()
            .min_recent_time
    }

    fn full_slice_duration(factor: u8) -> Duration {
        TimePartition::new(factor, DhtArc::FULL)
            .unwrap()
            .full_slice_duration
    }
}
