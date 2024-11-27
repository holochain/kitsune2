//! Partition of time into slices.
//!
//! Provides an encapsulation for calculating time slices that is consistent when computed
//! by different nodes. This is used to group Kitsune2 op data into time slices for combined hashes.
//! Giving multiple nodes the same time slice boundaries allows them to compute the same combined
//! hashes, and therefore effectively communicate which of their time slices match and which do not.
//!
//! A time slice is defined to be an interval of time that is closed (inclusive) at the start and
//! exclusive at the end. That is, the time slice includes the start timestamp but not the end.
//!
//! Time slices are partitioned into two types: full slices and partial slices. Full slices are
//! always of a fixed size, while partial slices are of varying sizes. Full slices occupy
//! historical time, while partial slices occupy recent time.
//!
//! The granularity of time slices is determined by the `factor` parameter. The chosen factor
//! determines the size of time slices. Where 2^X means "two raised to the power of X", the size of
//! a full time slice is 2^factor * [UNIT_TIME]. Partial time slices vary in size from
//! 2^(factor - 1) * [UNIT_TIME] down to 2^0 * [UNIT_TIME] (i.e. [UNIT_TIME]). There is some
//! amount of time left over that cannot be partitioned into smaller slices.
//!
//! > Note: The factor is used with durations and the code needs to be able to do arithmetic with
//!   it, so the factor has a maximum value of 53. The factor also has a minimum value of 1,
//!   though values lower than 4 are effectively meaningless. Consider choosing a factor that
//!   results in a meaningful split between recent time and historical, full slices.
//!
//! Because recent time is expected to change more frequently, combined hashes for partial slices
//! are stored in memory. Full slices are stored in the Kitsune2 op store.
//!
//! The algorithm for partitioning time is as follows:
//!   - Reserve a minimum amount of recent time as a sum of possible partial slice sizes.
//!   - Allocate as many full slices as possible.
//!   - Partition the remaining time into smaller slices. If there is space for two slices of a
//!     given size, then allocate two slices of that size. Otherwise, allocate one slice of that
//!     size. Larger slices are allocated before smaller slices.
//!
//! As time progresses, the partitioning needs to be updated. This is done by calling the
//! [PartitionedTime::update] method. The update method will:
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
//! A lower factor allocates less recent time and more slices to be stored but is more granular when
//! comparing time slices with another peer. A higher factor allocates more recent time and fewer
//! slices to be stored but is less granular when comparing time slices with another peer.

use crate::constant::UNIT_TIME;
use kitsune2_api::{DynOpStore, K2Error, K2Result, OpId, Timestamp};
use std::time::Duration;

#[derive(Debug)]
#[cfg_attr(test, derive(Clone, PartialEq))]
pub struct PartitionedTime {
    /// The timestamp at which the time partitioning starts.
    ///
    /// This must be a constant among peers who need to compute the same time slices.
    origin_timestamp: Timestamp,
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
    /// This is used regularly and is a constant based on [PartitionedTime::factor], so it is
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
    /// [PartitionedTime::update] is not called.
    /// It is idempotent to call [PartitionedTime::update] more often than required, but it is not
    /// efficient.
    next_update_at: Timestamp,
}

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
    hash: bytes::Bytes,
}

impl PartialSlice {
    /// The end timestamp of the time slice.
    ///
    /// This timestamp is not included in the time slice.
    fn end(&self) -> Timestamp {
        self.start
            + Duration::from_secs((1u64 << self.size) * UNIT_TIME.as_secs())
    }
}

// Public methods
impl PartitionedTime {
    /// Create a new instance of [PartitionedTime] from the given store.
    ///
    /// The store is needed to check how many time slices were created last time this
    /// [PartitionedTime] was updated, if any.
    ///
    /// The method will then update the state of the [PartitionedTime] to the current time.
    /// It does this by checking that the construction of this [PartitionedTime] is consistent
    /// and then calling [PartitionedTime::update].
    ///
    /// The resulting [PartitionedTime] will be consistent with the store and the current time.
    /// It should be updated again after [PartitionedTime::next_update_at].
    pub async fn try_from_store(
        origin_timestamp: Timestamp,
        factor: u8,
        store: DynOpStore,
    ) -> K2Result<Self> {
        let mut pt = Self::new(origin_timestamp, factor)?;

        pt.full_slices = store.slice_hash_count().await?;

        // The end timestamp of the last full slice
        let full_slice_end_timestamp = pt.full_slice_end_timestamp();

        // Given the time reserved by full slices, how much time is left to partition into smaller slices
        let recent_time = (Timestamp::now() - full_slice_end_timestamp).map_err(|_| {
            K2Error::other("Failed to calculate recent time, either the clock is wrong or this is a bug")
        })?;

        if pt.full_slices > 0 && recent_time < pt.min_recent_time {
            return Err(K2Error::other("Not enough recent time reserved, either the clock is wrong or this is a bug"));
        }

        // Update the state for the current time. The stored slices might be out of date.
        pt.update(store, Timestamp::now()).await?;

        Ok(pt)
    }

    /// The timestamp at which the next update is required.
    ///
    /// See the field [PartitionedTime::next_update_at] for more information.
    ///
    /// This value is updated by [PartitionedTime::update], so you can check the next update time
    /// after calling that method.
    pub fn next_update_at(&self) -> Timestamp {
        self.next_update_at
    }

    /// Update the state of the [PartitionedTime] to the current time.
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
        let mut full_slices_end_timestamp = self.full_slice_end_timestamp();

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

        for _ in 0..new_full_slices_count {
            // Store the hash of the full slice
            let op_hashes = store
                .retrieve_op_hashes_in_time_slice(
                    full_slices_end_timestamp,
                    full_slices_end_timestamp + self.full_slice_duration,
                )
                .await?;

            let hash = combine_op_hashes(op_hashes);

            store
                .store_slice_hash(self.full_slices, hash.to_vec())
                .await?;

            self.full_slices += 1;
            full_slices_end_timestamp += self.full_slice_duration;
        }

        let new_partials =
            self.layout_partials(current_time, full_slices_end_timestamp)?;
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
                    combine_op_hashes(
                        store
                            .retrieve_op_hashes_in_time_slice(start, end)
                            .await?,
                    )
                }
            };

            self.partial_slices.push(PartialSlice { start, size, hash })
        }

        // There will be a small amount of time left over, which we can't partition into smaller
        // slices. Some amount less than [UNIT_TIME] will be left over.
        // Once that has elapsed up to the next [UNIT_TIME] boundary, we can update again.
        if let Some(last_partial) = self.partial_slices.last() {
            self.next_update_at = last_partial.end() + UNIT_TIME;
        }

        Ok(())
    }
}

// Private methods
impl PartitionedTime {
    /// Private constructor, see [PartitionedTime::try_from_store].
    ///
    /// This constructor just creates an instance with initial values, but it doesn't update the
    /// state with full and partial slices for the current time.
    fn new(origin_timestamp: Timestamp, factor: u8) -> K2Result<Self> {
        Ok(Self {
            origin_timestamp,
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
        })
    }

    /// The timestamp at which the last full slice ends.
    ///
    /// If there are no full slices, then this is the origin timestamp.
    fn full_slice_end_timestamp(&self) -> Timestamp {
        let full_slices_duration = Duration::from_secs(
            self.full_slices * self.full_slice_duration.as_secs(),
        );
        self.origin_timestamp + full_slices_duration
    }

    /// Layout the partial slices for the given time range.
    ///
    /// Tries to allocate as many large slices as possible to fill the space.
    fn layout_partials(
        &self,
        current_time: Timestamp,
        mut start_at: Timestamp,
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

/// Combine a series of op hashes into a single hash.
///
/// Requires that the op hashes are already ordered.
/// If the input is empty, then the output is an empty byte array.
fn combine_op_hashes(hashes: Vec<OpId>) -> bytes::Bytes {
    let mut out = if let Some(first) = hashes.first() {
        bytes::BytesMut::zeroed(first.0.len())
    } else {
        // `Bytes::new` does not allocate, so if there was no input, then return an empty
        // byte array without allocating.
        return bytes::Bytes::new();
    };

    let iter = hashes.into_iter().map(|x| x.0 .0);
    for hash in iter {
        for (out_byte, hash_byte) in out.iter_mut().zip(hash.iter()) {
            *out_byte ^= hash_byte;
        }
    }

    out.freeze()
}

#[cfg(test)]
mod tests {
    use super::*;
    use kitsune2_api::{MetaOp, OpStore};
    use kitsune2_memory::Kitsune2MemoryOpStore;
    use std::sync::Arc;

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

    #[tokio::test]
    async fn new() {
        let origin_timestamp = Timestamp::now();
        let factor = 4;
        let pt = PartitionedTime::new(origin_timestamp, factor).unwrap();

        // Full slices would have size 2^4 = 16, so we should reserve space for at least one
        // of each smaller slice size
        assert_eq!((8 + 4 + 2 + 1) * UNIT_TIME, pt.min_recent_time);
    }

    #[tokio::test]
    async fn from_store() {
        let origin_timestamp = Timestamp::now();
        let factor = 4;
        let store = Arc::new(Kitsune2MemoryOpStore::default());
        let pt =
            PartitionedTime::try_from_store(origin_timestamp, factor, store)
                .await
                .unwrap();

        assert_eq!(0, pt.full_slices);
        assert!(pt.partial_slices.is_empty());
    }

    #[tokio::test]
    async fn one_partial_slice() {
        let now = Timestamp::now();
        let origin_timestamp =
            (now - Duration::from_secs(UNIT_TIME.as_secs() + 1)).unwrap();
        let factor = 4;
        let store = Arc::new(Kitsune2MemoryOpStore::default());
        let pt =
            PartitionedTime::try_from_store(origin_timestamp, factor, store)
                .await
                .unwrap();

        // Should allocate no full slices and one partial
        assert_eq!(0, pt.full_slices);
        assert_eq!(1, pt.partial_slices.len());

        // The partial should be:
        //   - The minimum factor size, and
        //   - start at the origin time
        //   - end at the origin time + UNIT_TIME
        assert_eq!(0, pt.partial_slices[0].size);
        assert_eq!(origin_timestamp, pt.partial_slices[0].start);
        assert_eq!(origin_timestamp + UNIT_TIME, pt.partial_slices[0].end());

        // The next required update should be at the end of the partial slice
        // plus another UNIT_TIME.
        assert_eq!(origin_timestamp + 2 * UNIT_TIME, pt.next_update_at);

        // Or to put it another way, UNIT_TIME - 1s from now
        assert_eq!(
            (now + UNIT_TIME - Duration::from_secs(1)).unwrap(),
            pt.next_update_at
        );
    }

    #[tokio::test]
    async fn all_single_partial_slices() {
        let factor = 7;
        let origin_timestamp = (Timestamp::now()
            - Duration::from_secs(
                // Enough time for all the partial slices
                PartitionedTime::new(Timestamp::now(), factor)
                    .unwrap()
                    .min_recent_time
                    .as_secs()
                    + 1,
            ))
        .unwrap();
        let store = Arc::new(Kitsune2MemoryOpStore::default());
        let pt =
            PartitionedTime::try_from_store(origin_timestamp, factor, store)
                .await
                .unwrap();

        assert_eq!(0, pt.full_slices);
        assert_eq!(factor as usize, pt.partial_slices.len());

        validate_partial_slices(&pt);
    }

    #[tokio::test]
    async fn one_double_others_single_partial_slices() {
        let factor = 7;
        let origin_timestamp = (Timestamp::now()
            - Duration::from_secs(
                PartitionedTime::new(Timestamp::now(), factor).unwrap().min_recent_time.as_secs() +
                // Add enough time to reserve a double slice in the first spot
            2u32.pow(factor as u32 - 1) as u64 * UNIT_TIME.as_secs()
                + 1,
            ))
        .unwrap();
        let store = Arc::new(Kitsune2MemoryOpStore::default());
        let pt =
            PartitionedTime::try_from_store(origin_timestamp, factor, store)
                .await
                .unwrap();

        assert_eq!(factor as usize + 1, pt.partial_slices.len());

        validate_partial_slices(&pt);
    }

    #[tokio::test]
    async fn all_double_slices() {
        let factor = 7;
        let origin_timestamp = (Timestamp::now()
            - Duration::from_secs(
                // Enough time for two of each of the partial slices
                2 * PartitionedTime::new(Timestamp::now(), factor)
                    .unwrap()
                    .min_recent_time
                    .as_secs()
                    + 1,
            ))
        .unwrap();
        let store = Arc::new(Kitsune2MemoryOpStore::default());
        let pt =
            PartitionedTime::try_from_store(origin_timestamp, factor, store)
                .await
                .unwrap();

        assert_eq!(factor as usize * 2, pt.partial_slices.len());

        validate_partial_slices(&pt);
    }

    #[tokio::test]
    async fn hashes_are_combined_for_partial_slices() {
        let factor = 7;
        let now = Timestamp::now();
        let origin_timestamp = (now
            - Duration::from_secs(
                // Enough time for all the partial slices
                PartitionedTime::new(Timestamp::now(), factor)
                    .unwrap()
                    .min_recent_time
                    .as_secs()
                    + 1,
            ))
        .unwrap();

        let store = Arc::new(Kitsune2MemoryOpStore::default());
        store
            .process_incoming_ops(vec![
                MetaOp {
                    op_id: OpId::from(bytes::Bytes::from(vec![7; 32])),
                    timestamp: (now - UNIT_TIME).unwrap(),
                    op_data: vec![],
                    op_flags: Default::default(),
                },
                MetaOp {
                    op_id: OpId::from(bytes::Bytes::from(vec![23; 32])),
                    timestamp: (now - UNIT_TIME).unwrap(),
                    op_data: vec![],
                    op_flags: Default::default(),
                },
            ])
            .await
            .unwrap();

        let pt =
            PartitionedTime::try_from_store(origin_timestamp, factor, store)
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
        let origin_timestamp = (Timestamp::now()
            - Duration::from_secs(
                // Two full slices
                2 * 2u32.pow(factor as u32) as u64 * UNIT_TIME.as_secs() +
                // Enough time remaining for recent time
            PartitionedTime::new(Timestamp::now(), factor)
                .unwrap()
                .min_recent_time
                .as_secs()
                + 1,
            ))
        .unwrap();
        let store = Arc::new(Kitsune2MemoryOpStore::default());
        store.store_slice_hash(0, vec![1; 64]).await.unwrap();
        store.store_slice_hash(1, vec![1; 64]).await.unwrap();

        let pt =
            PartitionedTime::try_from_store(origin_timestamp, factor, store)
                .await
                .unwrap();

        assert_eq!(2, pt.full_slices);
        assert_eq!(factor as usize, pt.partial_slices.len());

        validate_partial_slices(&pt);
    }

    #[tokio::test]
    async fn missing_full_slices_with_all_partials() {
        let factor = 7;
        let origin_timestamp = (Timestamp::now()
            - Duration::from_secs(
                // One full slice
                2u32.pow(factor as u32) as u64 * UNIT_TIME.as_secs() +
                // Enough time remaining for all the single partial slices
            PartitionedTime::new(Timestamp::now(), factor)
                .unwrap()
                .min_recent_time
                .as_secs()
                + 1,
            ))
        .unwrap();

        // Store with no full slices stored
        let store = Arc::new(Kitsune2MemoryOpStore::default());
        store
            .process_incoming_ops(vec![MetaOp {
                op_id: OpId::from(bytes::Bytes::from(vec![7; 32])),
                timestamp: origin_timestamp,
                op_data: vec![],
                op_flags: Default::default(),
            }])
            .await
            .unwrap();

        let pt = PartitionedTime::try_from_store(
            origin_timestamp,
            factor,
            store.clone(),
        )
        .await
        .unwrap();

        // One full slice and all the partial slices should be created
        assert_eq!(1, pt.full_slices);
        assert_eq!(factor as usize, pt.partial_slices.len());

        // The full slice should have been stored
        assert_eq!(1, store.slice_hash_count().await.unwrap());
        let full_slice_hash =
            store.retrieve_slice_hash(0).await.unwrap().unwrap();
        assert_eq!(vec![7; 32], full_slice_hash);

        validate_partial_slices(&pt);
    }

    #[tokio::test]
    async fn missing_full_slices_combines_hashes() {
        let factor = 7;
        let origin_timestamp = (Timestamp::now()
            - Duration::from_secs(
                // One full slice
                2u32.pow(factor as u32) as u64 * UNIT_TIME.as_secs() +
                // Enough time remaining for all the single partial slices
                PartitionedTime::new(Timestamp::now(), factor)
                    .unwrap()
                    .min_recent_time
                    .as_secs()
                + 1,
            ))
        .unwrap();

        // Store with no full slices stored
        let store = Arc::new(Kitsune2MemoryOpStore::default());
        store
            .process_incoming_ops(vec![
                MetaOp {
                    op_id: OpId::from(bytes::Bytes::from(vec![7; 32])),
                    timestamp: origin_timestamp,
                    op_data: vec![],
                    op_flags: Default::default(),
                },
                MetaOp {
                    op_id: OpId::from(bytes::Bytes::from(vec![23; 32])),
                    timestamp: origin_timestamp,
                    op_data: vec![],
                    op_flags: Default::default(),
                },
            ])
            .await
            .unwrap();

        let pt = PartitionedTime::try_from_store(
            origin_timestamp,
            factor,
            store.clone(),
        )
        .await
        .unwrap();

        // One full slice and all the partial slices should be created
        assert_eq!(1, pt.full_slices);
        assert_eq!(factor as usize, pt.partial_slices.len());

        // The full slice should have been stored
        assert_eq!(1, store.slice_hash_count().await.unwrap());
        let full_slice_hash =
            store.retrieve_slice_hash(0).await.unwrap().unwrap();
        // The hashes should be combined using XOR
        assert_eq!(vec![7 ^ 23; 32], full_slice_hash);

        validate_partial_slices(&pt);
    }

    #[tokio::test]
    async fn compute_hashes_for_full_slices_and_partials() {
        let factor = 7;
        let now = Timestamp::now();
        let origin_timestamp = (now
            - Duration::from_secs(
                // One full slice
                2u32.pow(factor as u32) as u64 * UNIT_TIME.as_secs() +
                // Enough time remaining for all the single partial slices
                PartitionedTime::new(Timestamp::now(), factor)
                    .unwrap()
                    .min_recent_time
                    .as_secs()
                + 1,
            ))
        .unwrap();

        // Store with no full slices stored
        let store = Arc::new(Kitsune2MemoryOpStore::default());
        store
            .process_incoming_ops(vec![
                MetaOp {
                    op_id: OpId::from(bytes::Bytes::from(vec![7; 32])),
                    timestamp: origin_timestamp,
                    op_data: vec![],
                    op_flags: Default::default(),
                },
                MetaOp {
                    op_id: OpId::from(bytes::Bytes::from(vec![23; 32])),
                    timestamp: origin_timestamp,
                    op_data: vec![],
                    op_flags: Default::default(),
                },
                MetaOp {
                    op_id: OpId::from(bytes::Bytes::from(vec![7; 32])),
                    timestamp: (now - UNIT_TIME).unwrap(),
                    op_data: vec![],
                    op_flags: Default::default(),
                },
                MetaOp {
                    op_id: OpId::from(bytes::Bytes::from(vec![29; 32])),
                    timestamp: (now - UNIT_TIME).unwrap(),
                    op_data: vec![],
                    op_flags: Default::default(),
                },
            ])
            .await
            .unwrap();

        let pt = PartitionedTime::try_from_store(
            origin_timestamp,
            factor,
            store.clone(),
        )
        .await
        .unwrap();

        // One full slice and all the partial slices should be created
        assert_eq!(1, pt.full_slices);
        assert_eq!(factor as usize, pt.partial_slices.len());

        // The full slice should have been stored
        assert_eq!(1, store.slice_hash_count().await.unwrap());
        let full_slice_hash =
            store.retrieve_slice_hash(0).await.unwrap().unwrap();
        // The hashes should be combined using XOR
        assert_eq!(vec![7 ^ 23; 32], full_slice_hash);

        // The last partial slice should have the combined hash of the two ops
        let last_partial = pt.partial_slices.last().unwrap();
        assert_eq!(vec![7 ^ 29; 32], last_partial.hash);

        validate_partial_slices(&pt);
    }

    #[tokio::test]
    async fn update_is_idempotent() {
        let factor = 7;
        let now = Timestamp::now();
        let origin_timestamp = (now
            - Duration::from_secs(
                // One full slice
                2u32.pow(factor as u32) as u64 * UNIT_TIME.as_secs() +
                // Enough time remaining for all the single partial slices
                PartitionedTime::new(Timestamp::now(), factor)
                    .unwrap()
                    .min_recent_time
                    .as_secs()
                + 1,
            ))
        .unwrap();

        // Store with no full slices stored
        let store = Arc::new(Kitsune2MemoryOpStore::default());
        store
            .process_incoming_ops(vec![
                MetaOp {
                    op_id: OpId::from(bytes::Bytes::from(vec![7; 32])),
                    timestamp: origin_timestamp,
                    op_data: vec![],
                    op_flags: Default::default(),
                },
                MetaOp {
                    op_id: OpId::from(bytes::Bytes::from(vec![23; 32])),
                    timestamp: origin_timestamp,
                    op_data: vec![],
                    op_flags: Default::default(),
                },
                MetaOp {
                    op_id: OpId::from(bytes::Bytes::from(vec![7; 32])),
                    timestamp: (now - UNIT_TIME).unwrap(),
                    op_data: vec![],
                    op_flags: Default::default(),
                },
                MetaOp {
                    op_id: OpId::from(bytes::Bytes::from(vec![29; 32])),
                    timestamp: (now - UNIT_TIME).unwrap(),
                    op_data: vec![],
                    op_flags: Default::default(),
                },
            ])
            .await
            .unwrap();

        let mut pt = PartitionedTime::try_from_store(
            origin_timestamp,
            factor,
            store.clone(),
        )
        .await
        .unwrap();

        // Capture the initially created state
        let pt_original = pt.clone();

        // Update repeatedly, the state should not change
        for i in 1..10 {
            let call_at = now + Duration::from_secs(i);
            assert!(call_at < pt.next_update_at());

            pt.update(store.clone(), call_at).await.unwrap();

            assert_eq!(pt_original, pt);
        }
    }

    #[tokio::test]
    async fn update_allocate_new_partial() {
        let factor = 7;
        let now = Timestamp::now();
        let origin_timestamp = (now
            - Duration::from_secs(
                // One full slice
                2u32.pow(factor as u32) as u64 * UNIT_TIME.as_secs() +
                // Enough time remaining for all the single partial slices
                PartitionedTime::new(Timestamp::now(), factor)
                    .unwrap()
                    .min_recent_time
                    .as_secs()
                + 1,
            ))
        .unwrap();

        let store = Arc::new(Kitsune2MemoryOpStore::default());
        store.store_slice_hash(0, vec![1; 64]).await.unwrap();
        store
            .process_incoming_ops(vec![
                MetaOp {
                    op_id: OpId::from(bytes::Bytes::from(vec![7; 32])),
                    timestamp: (now - UNIT_TIME).unwrap(),
                    op_data: vec![],
                    op_flags: Default::default(),
                },
                MetaOp {
                    op_id: OpId::from(bytes::Bytes::from(vec![29; 32])),
                    timestamp: (now - UNIT_TIME).unwrap(),
                    op_data: vec![],
                    op_flags: Default::default(),
                },
            ])
            .await
            .unwrap();

        let mut pt = PartitionedTime::try_from_store(
            origin_timestamp,
            factor,
            store.clone(),
        )
        .await
        .unwrap();

        assert_eq!(factor as usize, pt.partial_slices.len());
        let last_partial = pt.partial_slices.last().unwrap();
        assert_eq!(vec![7 ^ 29; 32], last_partial.hash);

        // Store a new op which will currently be outside the last partial slice
        store
            .process_incoming_ops(vec![MetaOp {
                op_id: OpId::from(bytes::Bytes::from(vec![13; 32])),
                timestamp: Timestamp::now(),
                op_data: vec![],
                op_flags: Default::default(),
            }])
            .await
            .unwrap();

        pt.update(store.clone(), Timestamp::now() + UNIT_TIME)
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
        let now = Timestamp::now();
        let origin_timestamp = (now
            - Duration::from_secs(
                // One full slice
                2u32.pow(factor as u32) as u64 * UNIT_TIME.as_secs() +
                // Enough time remaining for all the single partial slices
                PartitionedTime::new(Timestamp::now(), factor)
                    .unwrap()
                    .min_recent_time
                    .as_secs()
                + 1,
            ))
        .unwrap();

        let store = Arc::new(Kitsune2MemoryOpStore::default());
        store
            .process_incoming_ops(vec![
                MetaOp {
                    op_id: OpId::from(bytes::Bytes::from(vec![7; 32])),
                    timestamp: origin_timestamp,
                    op_data: vec![],
                    op_flags: Default::default(),
                },
                MetaOp {
                    op_id: OpId::from(bytes::Bytes::from(vec![23; 32])),
                    timestamp: origin_timestamp,
                    op_data: vec![],
                    op_flags: Default::default(),
                },
            ])
            .await
            .unwrap();

        let mut pt = PartitionedTime::try_from_store(
            origin_timestamp,
            factor,
            store.clone(),
        )
        .await
        .unwrap();

        assert_eq!(1, pt.full_slices);
        assert_eq!(
            vec![7 ^ 23; 32],
            store.retrieve_slice_hash(0).await.unwrap().unwrap()
        );

        // Store a new op, currently in the first partial slice, but will be in the next full slice.
        store
            .process_incoming_ops(vec![MetaOp {
                op_id: OpId::from(bytes::Bytes::from(vec![13; 32])),
                timestamp: pt.full_slice_end_timestamp(), // Start of the next full slice
                op_data: vec![],
                op_flags: Default::default(),
            }])
            .await
            .unwrap();

        pt.update(
            store.clone(),
            Timestamp::now()
                + Duration::from_secs(
                    2u32.pow(factor as u32) as u64 * UNIT_TIME.as_secs(),
                ),
        )
        .await
        .unwrap();

        assert_eq!(2, pt.full_slices);
        assert_eq!(factor as usize, pt.partial_slices.len());

        assert_eq!(2, store.slice_hash_count().await.unwrap());
        assert_eq!(
            vec![7 ^ 23; 32],
            store.retrieve_slice_hash(0).await.unwrap().unwrap()
        );
        assert_eq!(
            vec![13; 32],
            store.retrieve_slice_hash(1).await.unwrap().unwrap()
        );
    }

    #[tokio::test]
    async fn update_allocate_multiple_new_complete() {
        let factor = 7;
        let now = Timestamp::now();
        let origin_timestamp = (now
            - Duration::from_secs(
                PartitionedTime::new(Timestamp::now(), factor)
                    .unwrap()
                    .min_recent_time
                    .as_secs()
                    + 1,
            ))
        .unwrap();

        let store = Arc::new(Kitsune2MemoryOpStore::default());

        let mut pt = PartitionedTime::try_from_store(
            origin_timestamp,
            factor,
            store.clone(),
        )
        .await
        .unwrap();

        assert_eq!(0, pt.full_slices);

        store
            .process_incoming_ops(vec![
                // Store two new ops at the origin timestamp, to go into the first complete slice
                MetaOp {
                    op_id: OpId::from(bytes::Bytes::from(vec![7; 32])),
                    timestamp: origin_timestamp,
                    op_data: vec![],
                    op_flags: Default::default(),
                },
                MetaOp {
                    op_id: OpId::from(bytes::Bytes::from(vec![23; 32])),
                    timestamp: origin_timestamp,
                    op_data: vec![],
                    op_flags: Default::default(),
                },
                // Store two new ops at the origin timestamp plus one full time slice,
                // to go into the second complete slice
                MetaOp {
                    op_id: OpId::from(bytes::Bytes::from(vec![11; 32])),
                    timestamp: origin_timestamp + pt.full_slice_duration,
                    op_data: vec![],
                    op_flags: Default::default(),
                },
                MetaOp {
                    op_id: OpId::from(bytes::Bytes::from(vec![37; 32])),
                    timestamp: origin_timestamp + pt.full_slice_duration,
                    op_data: vec![],
                    op_flags: Default::default(),
                },
            ])
            .await
            .unwrap();

        pt.update(
            store.clone(),
            Timestamp::now() + (2 * pt.full_slice_duration),
        )
        .await
        .unwrap();

        assert_eq!(2, pt.full_slices);
        assert_eq!(factor as usize, pt.partial_slices.len());

        assert_eq!(2, store.slice_hash_count().await.unwrap());
        assert_eq!(
            vec![7 ^ 23; 32],
            store.retrieve_slice_hash(0).await.unwrap().unwrap()
        );
        assert_eq!(
            vec![11 ^ 37; 32],
            store.retrieve_slice_hash(1).await.unwrap().unwrap()
        );
    }

    fn validate_partial_slices(pt: &PartitionedTime) {
        let mut start_at = pt.origin_timestamp
            + Duration::from_secs(
                pt.full_slices
                    * 2u32.pow(pt.factor as u32) as u64
                    * UNIT_TIME.as_secs(),
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

            start_at += Duration::from_secs(
                2u32.pow(slice.size as u32) as u64 * UNIT_TIME.as_secs(),
            );
        }
    }
}
