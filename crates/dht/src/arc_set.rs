//! Represents a set of [DhtArc]s.
//!
//! A set of [DhtArc]s is combined as a set union into an [ArcSet].
//!
//! To restrict [crate::dht::Dht] operations to a specific set of sectors, the [ArcSet]s of two
//! DHTs can be intersected to find the common sectors, using [ArcSet::intersection].

use kitsune2_api::{DhtArc, K2Error, K2Result};
use std::collections::HashSet;

/// Represents a set of [DhtArc]s.
#[derive(Debug)]
#[cfg_attr(test, derive(PartialEq))]
pub struct ArcSet {
    inner: HashSet<u32>,
}

impl ArcSet {
    /// Create a new arc set from a list of arcs.
    ///
    /// The resulting arc set represents the union of the input arcs.
    pub fn new(size: u32, arcs: Vec<DhtArc>) -> K2Result<Self> {
        let factor = u32::MAX / size + 1;

        // The original factor should have been a power of 2
        if factor == 0 || factor & (factor - 1) != 0 {
            return Err(K2Error::other("Invalid size"));
        }

        let mut inner = HashSet::new();
        for arc in arcs {
            // If we have reached full arc then there's no need to keep going
            if inner.len() == factor as usize {
                break;
            }

            match arc {
                DhtArc::Empty => {
                    continue;
                }
                DhtArc::Arc(start, end) => {
                    let num_sectors_covered = if start > end {
                        let length = u32::MAX - start + end + 1;
                        length / size + 1
                    } else {
                        (end - start) / size + 1
                    };

                    let mut start = start;
                    for _ in 0..num_sectors_covered {
                        inner.insert(start / size);
                        start = start.overflowing_add(size).0;
                    }

                    if start != end.overflowing_add(1).0
                        && !(end == u32::MAX && start == 0)
                    {
                        return Err(K2Error::other(format!(
                            "Invalid arc, expected end at {} but arc specifies {}",
                            start, end
                        )));
                    }
                }
            }
        }

        Ok(ArcSet { inner })
    }

    /// Get the intersection of two arc sets as a new [ArcSet].
    ///
    /// # Example
    ///
    /// ```rust
    /// use kitsune2_api::DhtArc;
    /// use kitsune2_dht::ArcSet;
    ///
    /// # fn main() -> kitsune2_api::K2Result<()> {
    /// use tracing::Instrument;
    /// let arc_size = 1 << 23;
    /// let arc_1 = DhtArc::Arc(0, 2 * arc_size - 1);
    /// let arc_set_1 = ArcSet::new(arc_size, vec![arc_1])?;
    ///
    /// let arc_2 = DhtArc::Arc(arc_size, 4 * arc_size - 1);
    /// let arc_set_2 = ArcSet::new(arc_size, vec![arc_2])?;
    ///
    /// assert_eq!(1, arc_set_1.intersection(&arc_set_2).covered_sector_count());
    /// # Ok(())
    /// # }
    /// ```
    pub fn intersection(&self, other: &Self) -> Self {
        ArcSet {
            inner: self.inner.intersection(&other.inner).copied().collect(),
        }
    }

    /// The number of sectors covered by this arc set.
    ///
    /// # Example
    ///
    /// ```rust
    /// use kitsune2_api::DhtArc;
    /// use kitsune2_dht::ArcSet;
    ///
    /// # fn main() -> kitsune2_api::K2Result<()> {
    /// let arc_size = 1 << 23;
    /// let arc_1 = DhtArc::Arc(0, 2 * arc_size - 1);
    /// let arc_2 = DhtArc::Arc(2 * arc_size, 4 * arc_size - 1);
    /// let arc_set = ArcSet::new(arc_size, vec![arc_1, arc_2])?;
    ///
    /// assert_eq!(4, arc_set.covered_sector_count());
    /// # Ok(())
    /// # }
    /// ```
    pub fn covered_sector_count(&self) -> usize {
        self.inner.len()
    }

    pub(crate) fn includes_sector_index(&self, value: u32) -> bool {
        self.inner.contains(&value)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    const SECTOR_SIZE: u32 = 1u32 << 23;

    #[test]
    fn new_with_no_arcs() {
        let set = ArcSet::new(SECTOR_SIZE, vec![]).unwrap();

        assert!(set.inner.is_empty());
    }

    #[test]
    fn new_with_full_arc() {
        let set = ArcSet::new(SECTOR_SIZE, vec![DhtArc::FULL]).unwrap();

        // Sufficient to check that all the right values are included
        assert_eq!(512, set.inner.len());
        assert_eq!(511, *set.inner.iter().max().unwrap());
    }

    #[test]
    fn new_with_two_arcs() {
        let set = ArcSet::new(
            SECTOR_SIZE,
            vec![
                DhtArc::Arc(0, 255 * SECTOR_SIZE - 1),
                DhtArc::Arc(255 * SECTOR_SIZE, u32::MAX),
            ],
        )
        .unwrap();

        // Should become a full arc
        assert_eq!(512, set.inner.len());
        assert_eq!(511, *set.inner.iter().max().unwrap());
    }

    #[test]
    fn overlapping_arcs() {
        let set = ArcSet::new(
            SECTOR_SIZE,
            vec![
                DhtArc::Arc(0, 3 * SECTOR_SIZE - 1),
                DhtArc::Arc(2 * SECTOR_SIZE, 4 * SECTOR_SIZE - 1),
            ],
        )
        .unwrap();

        assert_eq!(4, set.inner.len());
        assert_eq!(3, *set.inner.iter().max().unwrap());
    }

    #[test]
    fn wrapping_arc() {
        let set = ArcSet::new(
            SECTOR_SIZE,
            vec![DhtArc::Arc(510 * SECTOR_SIZE, 3 * SECTOR_SIZE - 1)],
        )
        .unwrap();

        assert_eq!(5, set.inner.len(), "Set is {:?}", set.inner);
        assert_eq!(
            set.inner.len(),
            set.inner
                .intersection(&vec![510, 511, 0, 1, 2].into_iter().collect())
                .count()
        );
    }

    #[test]
    fn overlapping_wrapping_arcs() {
        let set = ArcSet::new(
            SECTOR_SIZE,
            vec![
                DhtArc::Arc(510 * SECTOR_SIZE, 3 * SECTOR_SIZE - 1),
                DhtArc::Arc(2 * SECTOR_SIZE, 4 * SECTOR_SIZE - 1),
            ],
        )
        .unwrap();

        assert_eq!(6, set.inner.len(), "Set is {:?}", set.inner);
        assert_eq!(
            set.inner.len(),
            set.inner
                .intersection(&vec![510, 511, 0, 1, 2, 3].into_iter().collect())
                .count()
        );
    }

    #[test]
    fn arc_not_on_boundaries() {
        let set = ArcSet::new(SECTOR_SIZE, vec![DhtArc::Arc(0, 50)]);

        assert!(set.is_err());
        assert_eq!(
            "Invalid arc, expected end at 8388608 but arc specifies 50 (src: None)",
            set.unwrap_err().to_string()
        );
    }

    #[test]
    fn valid_and_invalid_arcs() {
        let set = ArcSet::new(
            SECTOR_SIZE,
            vec![
                DhtArc::Arc(0, SECTOR_SIZE - 1),
                DhtArc::Arc(u32::MAX, u32::MAX),
            ],
        );

        assert!(set.is_err());
        assert_eq!(
            "Invalid arc, expected end at 8388607 but arc specifies 4294967295 (src: None)",
            set.unwrap_err().to_string()
        );
    }

    #[test]
    fn intersect_non_overlapping_sets() {
        let set1 =
            ArcSet::new(SECTOR_SIZE, vec![DhtArc::Arc(0, SECTOR_SIZE - 1)])
                .unwrap();
        let set2 = ArcSet::new(
            SECTOR_SIZE,
            vec![DhtArc::Arc(2 * SECTOR_SIZE, 3 * SECTOR_SIZE - 1)],
        )
        .unwrap();

        let intersection = set1.intersection(&set2);

        assert!(intersection.inner.is_empty());
    }

    #[test]
    fn intersect_overlapping_by_one() {
        let set1 =
            ArcSet::new(SECTOR_SIZE, vec![DhtArc::Arc(0, 2 * SECTOR_SIZE - 1)])
                .unwrap();
        let set2 = ArcSet::new(
            SECTOR_SIZE,
            vec![DhtArc::Arc(SECTOR_SIZE, 3 * SECTOR_SIZE - 1)],
        )
        .unwrap();

        let intersection = set1.intersection(&set2);

        assert_eq!(1, intersection.inner.len());
        assert!(intersection.inner.contains(&1));
    }

    #[test]
    fn intersect_overlapping_by_multiple() {
        let set1 = ArcSet::new(
            SECTOR_SIZE,
            vec![DhtArc::Arc(0, 10 * SECTOR_SIZE - 1)],
        )
        .unwrap();
        let set2 = ArcSet::new(
            SECTOR_SIZE,
            vec![DhtArc::Arc(SECTOR_SIZE, 3 * SECTOR_SIZE - 1)],
        )
        .unwrap();

        let intersection = set1.intersection(&set2);

        assert_eq!(2, intersection.inner.len());
        assert!(intersection.inner.contains(&1));
        assert!(intersection.inner.contains(&2));
    }

    #[test]
    fn preserves_full_arc() {
        let full_set = ArcSet::new(SECTOR_SIZE, vec![DhtArc::FULL]).unwrap();
        assert_eq!(
            full_set,
            full_set.intersection(
                &ArcSet::new(SECTOR_SIZE, vec![DhtArc::FULL]).unwrap()
            )
        );
    }

    #[test]
    fn preserves_empty() {
        let empty_set = ArcSet::new(SECTOR_SIZE, vec![DhtArc::Empty]).unwrap();
        assert_eq!(
            empty_set,
            empty_set.intersection(
                &ArcSet::new(SECTOR_SIZE, vec![DhtArc::FULL]).unwrap()
            )
        );
    }
}
