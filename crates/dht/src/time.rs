//! Partition of time into intervals.
//!
//! Provides an encapsulation for calculating time intervals which is consistent when computed
//! by different nodes.
//!
//! TODO: Design docs

use crate::constant::UNIT_TIME;
use kitsune2_api::{DynOpStore, K2Error, K2Result, OpId, Timestamp};
use std::time::Duration;

#[derive(Debug)]
pub struct PartitionedTime {
    origin_timestamp: Timestamp,
    factor: u8,
    full_buckets: u64,
    partial_buckets: Vec<PartialBucket>,
    min_recent_time: Duration,
    next_update_at: Timestamp,
}

#[derive(Debug)]
pub struct PartialBucket {
    /// The start timestamp of the time slice.
    ///
    /// This timestamp is included in the time slice.
    start: Timestamp,
    /// Size is used in the formula 2**size * [UNIT_TIME].
    ///
    /// That means a 0 size translates to [UNIT_TIME], and a 1 size translates to 2*[UNIT_TIME].
    size: u8,
    /// The combined hash over all the ops in this time slice.
    hash: bytes::Bytes,
}

impl PartitionedTime {
    /// Private constructor, see [PartitionedTime::from_store].
    ///
    /// This constructor just creates an instance with initial values, but it doesn't update the
    /// state with full and partial buckets for the current time.
    fn new(origin_timestamp: Timestamp, factor: u8) -> Self {
        Self {
            origin_timestamp,
            factor,
            full_buckets: 0,
            partial_buckets: Vec::new(),
            // This only changes based on the factor, which can't change at runtime,
            // so compute it on construction
            min_recent_time: residual_duration_for_factor(factor - 1),
            // Immediately requires an update, any time in the past will do
            next_update_at: Timestamp::from_micros(0),
        }
    }

    pub async fn from_store(
        origin_timestamp: Timestamp,
        factor: u8,
        store: DynOpStore,
    ) -> K2Result<Self> {
        let mut pt = Self::new(origin_timestamp, factor);

        pt.full_buckets = store
            .slice_hash_count()
            .await?;

        // The end timestamp of the last full bucket
        let full_bucket_end_timestamp = pt.full_bucket_end_timestamp();

        // Given the time reserved by full buckets, how much time is left to partition into smaller buckets
        let mut recent_time = (Timestamp::now() - full_bucket_end_timestamp).map_err(|_| {
            K2Error::other("Failed to calculate recent time, either the clock is is wrong or this is a bug")
        })?;

        if pt.full_buckets > 0 && recent_time < pt.min_recent_time {
            return Err(K2Error::other("Not enough recent time reserved, other the clock is is wrong or this is a bug"));
        }

        // Update the state for the current time. The stored buckets might be out of date.
        pt.update(store, Timestamp::now()).await?;

        Ok(pt)
    }

    pub async fn update(
        &mut self,
        store: DynOpStore,
        current_time: Timestamp,
    ) -> K2Result<()> {
        let mut full_bucket_end_timestamp = self.full_bucket_end_timestamp();

        let mut recent_time = (current_time - full_bucket_end_timestamp)
            .map_err(|_| K2Error::other("Clock invalid"))?;

        let new_full_buckets_count = if recent_time > self.min_recent_time {
            // Rely on integer rounding to get the correct number of full buckets
            // that need adding.
            (recent_time - self.min_recent_time).as_secs()
                / (2u32.pow(self.factor as u32) as u64
                * UNIT_TIME.as_secs())
        } else {
            0
        };

        full_bucket_end_timestamp += Duration::from_secs(
            new_full_buckets_count
                * 2u32.pow(self.factor as u32) as u64
                * UNIT_TIME.as_secs(),
        );

        let new_partials =
            self.layout_partials(current_time, full_bucket_end_timestamp)?;
        let old_partials = std::mem::take(&mut self.partial_buckets);

        for (start, size) in new_partials.into_iter() {
            // If this bucket didn't change, then we can reuse the hash
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
                None => store
                    .retrieve_op_hashes_in_time_slice(
                        start,
                        start
                            + Duration::from_secs(
                                2u32.pow(size as u32) as u64
                                    * UNIT_TIME.as_secs(),
                            ),
                    )
                    .await?
                    .into_iter()
                    .map(|x| x.0 .0)
                    .reduce(|a, b| {
                        a.iter().zip(b.iter()).map(|(a, b)| a ^ b).collect()
                    })
                    .unwrap_or_else(|| bytes::Bytes::new()),
            };

            self.partial_buckets.push(PartialBucket {
                start,
                size,
                hash,
            })
        }

        Ok(())
    }

    /// The timestamp at which the last full bucket ends.
    ///
    /// If there are no full buckets, then this is the origin timestamp.
    fn full_bucket_end_timestamp(&self) -> Timestamp {
        let full_buckets_duration = Duration::from_secs(
            self.full_buckets
                * 2u32.pow(self.factor as u32) as u64
                * UNIT_TIME.as_secs(),
        );
        self.origin_timestamp + full_buckets_duration
    }

    /// Layout the partial buckets for the given time range.
    ///
    /// Tries to allocate as many large buckets as possible to fill the space.
    fn layout_partials(
        &self,
        current_time: Timestamp,
        mut start_at: Timestamp,
    ) -> K2Result<Vec<(Timestamp, u8)>> {
        let mut recent_time = (current_time - start_at).map_err(|_| {
            K2Error::other("Failed to calculate recent time for partials, either the clock is is wrong or this is a bug")
        })?;

        let mut partials = Vec::new();

        // Now we want to partition the remaining time into smaller buckets
        for i in (0..self.factor).rev() {
            // Starting from the largest bucket size, if there's space for two of that bucket size
            // then add two buckets of that size, otherwise add one bucket of that size
            let bucket_size = Duration::from_secs(
                2u32.pow(i as u32) as u64 * UNIT_TIME.as_secs(),
            );
            let bucket_count = if recent_time
                > residual_duration_for_factor(i) + bucket_size
            {
                2
            } else if recent_time > bucket_size {
                1
            } else {
                continue;
            };

            for _ in 0..bucket_count {
                partials.push((start_at, i));
                start_at += bucket_size;
                recent_time -= bucket_size;
            }
        }

        Ok(partials)
    }
}

/// Computes what duration is required for a series of time buckets.
///
/// The duration is computed as the sum of the powers of two from 0 to the factor, multiplied by
/// the [UNIT_TIME].
///
/// Panics if the factor is greater than 53.
/// Note that the maximum factor changes if the [UNIT_TIME] is changed.
fn residual_duration_for_factor(factor: u8) -> Duration {
    if factor > 63 {
        panic!("Factor must be less than 64");
    }

    let mut sum = 1;
    for i in 1..=factor {
        sum |= 1u64 << i;
    }

    Duration::from_secs(sum * UNIT_TIME.as_secs())
}

/// Combine a series of op hashes into a single hash.
///
/// Requires that the op hashes are already ordered.
/// If the input is empty, then the output is an empty byte array.
fn combine_op_hashes(hashes: Vec<OpId>) -> bytes::Bytes {
    hashes
        .into_iter()
        .map(|x| x.0.0)
        .reduce(|a, b| a.iter().zip(b.iter()).map(|(a, b)| a ^ b).collect())
        .unwrap_or_else(|| bytes::Bytes::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use kitsune2_api::OpStore;
    use kitsune2_memory::Kitsune2MemoryOpStore;
    use std::sync::Arc;

    #[test]
    fn residual() {
        assert_eq!(UNIT_TIME, residual_duration_for_factor(0));
        assert_eq!((2 + 1) * UNIT_TIME, residual_duration_for_factor(1));
        assert_eq!((4 + 2 + 1) * UNIT_TIME, residual_duration_for_factor(2));
        assert_eq!(
            (32 + 16 + 8 + 4 + 2 + 1) * UNIT_TIME,
            residual_duration_for_factor(5)
        );

        let mask = 0b1111111111000000u16;
        assert_eq!(
            Duration::from_secs(!((mask as u64) << 48) * UNIT_TIME.as_secs()),
            residual_duration_for_factor(53)
        );
    }

    #[should_panic]
    #[test]
    fn max_residual() {
        residual_duration_for_factor(54);
    }

    #[tokio::test]
    async fn new() {
        let origin_timestamp = Timestamp::now();
        let factor = 4;
        let pt = PartitionedTime::new(origin_timestamp, factor);

        // Full buckets would have size 2^4 = 16, so we should reserve space for at last one
        // of each smaller bucket size
        assert_eq!((8 + 4 + 2 + 1) * UNIT_TIME, pt.min_recent_time);
    }

    #[tokio::test]
    async fn from_store() {
        let origin_timestamp = Timestamp::now();
        let factor = 4;
        let store = Arc::new(Kitsune2MemoryOpStore::default());
        let pt = PartitionedTime::from_store(origin_timestamp, factor, store)
            .await
            .unwrap();

        assert_eq!(0, pt.full_buckets);
        assert!(pt.partial_buckets.is_empty());
    }

    #[tokio::test]
    async fn one_partial_bucket() {
        let origin_timestamp = (Timestamp::now()
            - Duration::from_secs(UNIT_TIME.as_secs() + 1))
        .unwrap();
        let factor = 4;
        let store = Arc::new(Kitsune2MemoryOpStore::default());
        let pt = PartitionedTime::from_store(origin_timestamp, factor, store)
            .await
            .unwrap();

        assert_eq!(0, pt.full_buckets);
        assert_eq!(1, pt.partial_buckets.len());
        assert_eq!(0, pt.partial_buckets[0].size);
        assert_eq!(origin_timestamp, pt.partial_buckets[0].start);
    }

    #[tokio::test]
    async fn all_single_partial_buckets() {
        let factor = 7;
        let origin_timestamp = (Timestamp::now()
            - Duration::from_secs(
                PartitionedTime::new(Timestamp::now(), factor)
                    .min_recent_time
                    .as_secs()
                    + 1,
            ))
        .unwrap();
        let store = Arc::new(Kitsune2MemoryOpStore::default());
        let pt = PartitionedTime::from_store(origin_timestamp, factor, store)
            .await
            .unwrap();

        assert_eq!(0, pt.full_buckets);
        assert_eq!(factor as usize, pt.partial_buckets.len());

        validate_partial_buckets(&pt);
    }

    #[tokio::test]
    async fn one_double_others_single_partial_bucket() {
        let factor = 7;
        let origin_timestamp = (Timestamp::now()
            - Duration::from_secs(
                PartitionedTime::new(Timestamp::now(), factor).min_recent_time.as_secs() +
                // Add enough time to reserve a double bucket in the first spot
            2u32.pow(factor as u32 - 1) as u64 * UNIT_TIME.as_secs()
                + 1,
            ))
        .unwrap();
        let store = Arc::new(Kitsune2MemoryOpStore::default());
        let mut pt =
            PartitionedTime::from_store(origin_timestamp, factor, store)
                .await
                .unwrap();

        assert_eq!(factor as usize + 1, pt.partial_buckets.len());

        validate_partial_buckets(&pt);
    }

    #[tokio::test]
    async fn all_double_buckets() {
        let factor = 7;
        let origin_timestamp = (Timestamp::now()
            - Duration::from_secs(
                2 * PartitionedTime::new(Timestamp::now(), factor)
                    .min_recent_time
                    .as_secs()
                    + 1,
            ))
        .unwrap();
        let store = Arc::new(Kitsune2MemoryOpStore::default());
        let mut pt =
            PartitionedTime::from_store(origin_timestamp, factor, store)
                .await
                .unwrap();

        println!("{:?}", pt);

        assert_eq!(factor as usize * 2, pt.partial_buckets.len());

        validate_partial_buckets(&pt);
    }

    #[tokio::test]
    async fn full_buckets_with_single_bucket_partials() {
        let factor = 7;
        let origin_timestamp = (Timestamp::now()
            - Duration::from_secs(
                // Two full buckets
                2 * 2u32.pow(factor as u32) as u64 * UNIT_TIME.as_secs() +
                // Enough time remaining for recent time
            PartitionedTime::new(Timestamp::now(), factor)
                .min_recent_time
                .as_secs()
                + 1,
            ))
        .unwrap();
        let store = Arc::new(Kitsune2MemoryOpStore::default());
        store.store_slice_hash(0, vec![1; 64]).await.unwrap();
        store.store_slice_hash(1, vec![1; 64]).await.unwrap();

        let mut pt =
            PartitionedTime::from_store(origin_timestamp, factor, store)
                .await
                .unwrap();

        assert_eq!(2, pt.full_buckets);
        assert_eq!(factor as usize, pt.partial_buckets.len());

        validate_partial_buckets(&pt);
    }

    fn validate_partial_buckets(pt: &PartitionedTime) {
        let mut start_at = pt.origin_timestamp
            + Duration::from_secs(
                pt.full_buckets
                    * 2u32.pow(pt.factor as u32) as u64
                    * UNIT_TIME.as_secs(),
            );
        for (i, bucket) in pt.partial_buckets.iter().enumerate() {
            if i > 0 {
                // Require that the bucket sizes are decreasing
                // Not that the decrease is not strictly monotonic, as up to two buckets of the same
                // size are permitted
                assert!(
                    pt.partial_buckets[i - 1].size >= pt.partial_buckets[i].size,
                    "factor must decrease"
                );
            }
            if i > 1 {
                // Two is fine, if there are 3 of one size then they should have been collapsed before
                // the partial buckets can be considered in a valid state.
                assert!(
                    pt.partial_buckets[i - 2].size != pt.partial_buckets[i - 1].size
                        || pt.partial_buckets[i - 1].size
                            != pt.partial_buckets[i].size,
                    "no more than two buckets of the same size are permitted"
                );
            }

            // Check that the buckets are
            assert_eq!(start_at, pt.partial_buckets[i].start);

            start_at += Duration::from_secs(
                2u32.pow(pt.partial_buckets[i].size as u32) as u64
                    * UNIT_TIME.as_secs(),
            );
        }
    }
}
