use crate::SECTOR_SIZE;
use kitsune2_api::{DhtArc, K2Error, K2Result};
use std::collections::HashSet;

/// Represents a set of [DhtArc]s.
///
/// A set of [DhtArc]s is combined as a set union into an [ArcSet].
///
/// To restrict [`crate::dht::Dht`] operations to a specific set of sectors, the [`ArcSet`]s of two
/// DHTs can be intersected to find the common sectors, using [ArcSet::intersection].
#[derive(Clone, PartialEq)]
pub struct ArcSet {
    inner: HashSet<u32>,
}

impl std::fmt::Debug for ArcSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut dbg = f.debug_struct("ArcSet");
        match self.inner.len() {
            0 => dbg.field("sectors", &"Empty").finish(),
            len if len == (u32::MAX / SECTOR_SIZE + 1) as usize => {
                dbg.field("sectors", &"Full").finish()
            }
            _ => dbg.field("sectors", &self.inner.len()).finish(),
        }
    }
}

impl ArcSet {
    /// Create a new arc set from a list of arcs.
    ///
    /// The resulting arc set represents the union of the input arcs.
    pub fn new(arcs: Vec<DhtArc>) -> K2Result<Self> {
        let factor = u32::MAX / SECTOR_SIZE + 1;

        // The factor that was used to define the size should have been a power of 2
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
                        length / SECTOR_SIZE + 1
                    } else {
                        (end - start) / SECTOR_SIZE + 1
                    };

                    let mut start = start;
                    for _ in 0..num_sectors_covered {
                        inner.insert(start / SECTOR_SIZE);
                        start = start.overflowing_add(SECTOR_SIZE).0;
                    }

                    if start != end.overflowing_add(1).0
                        && !(end == u32::MAX && start == 0)
                    {
                        return Err(K2Error::other(format!(
                            "Invalid arc, expected end at {start} but arc specifies {end}"
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
    /// use kitsune2_dht::{ArcSet, SECTOR_SIZE};
    ///
    /// # fn main() -> kitsune2_api::K2Result<()> {
    /// let arc_1 = DhtArc::Arc(0, 2 * SECTOR_SIZE - 1);
    /// let arc_set_1 = ArcSet::new(vec![arc_1])?;
    ///
    /// let arc_2 = DhtArc::Arc(SECTOR_SIZE, 4 * SECTOR_SIZE - 1);
    /// let arc_set_2 = ArcSet::new(vec![arc_2])?;
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
    /// use kitsune2_dht::{ArcSet, SECTOR_SIZE};
    ///
    /// # fn main() -> kitsune2_api::K2Result<()> {
    /// let arc_1 = DhtArc::Arc(0, 2 * SECTOR_SIZE - 1);
    /// let arc_2 = DhtArc::Arc(2 * SECTOR_SIZE, 4 * SECTOR_SIZE - 1);
    /// let arc_set = ArcSet::new(vec![arc_1, arc_2])?;
    ///
    /// assert_eq!(4, arc_set.covered_sector_count());
    /// # Ok(())
    /// # }
    /// ```
    pub fn covered_sector_count(&self) -> usize {
        self.inner.len()
    }

    /// Encode this arc set into a compressed format for transmission.
    ///
    /// The encoding is a simple bitset where each bit represents a sector.
    pub fn encode(&self) -> Vec<u32> {
        let mut out = vec![0; 512 / 32];

        for sector in &self.inner {
            let index = sector / 32;
            let bit = sector % 32;
            out[index as usize] |= 1 << bit;
        }

        out
    }

    /// Decode an arc set from a compressed format.
    ///
    /// See the [ArcSet::encode] method for details on the encoding format.
    pub fn decode(input: &[u32]) -> K2Result<Self> {
        if input.len() != 512 / 32 {
            return Err(K2Error::other("Invalid size for encoded arc set"));
        }

        let mut inner = HashSet::new();
        for (index, value) in input.iter().enumerate() {
            for bit in 0..32 {
                if value & (1 << bit) != 0 {
                    inner.insert(index as u32 * 32 + bit);
                }
            }
        }

        Ok(ArcSet { inner })
    }

    /// Check whether a given sector index is included in this arc set.
    pub fn includes_sector_index(&self, value: u32) -> bool {
        self.inner.contains(&value)
    }

    /// Remove the given sector indices from this arc set.
    ///
    /// Note that this is a mutable operation, which is normally not needed for arc sets. This
    /// function therefore consumes the provided arc set to make it harder to accidentally modify
    /// an arc set that wasn't intended to be updated.
    pub fn without_sector_indices(
        mut self,
        remove: impl IntoIterator<Item = u32>,
    ) -> Self {
        for sector in remove {
            self.inner.remove(&sector);
        }

        self
    }

    /// Convert an arc set to a list of arcs.
    ///
    /// Note that an [ArcSet] is created from a `Vec<DhtArc>`, so if you have just created an
    /// [ArcSet] then this should return an equivalent list of arcs to what you provided, though
    /// not necessarily the same list. E.g. `ArcSet::new(vec![DhtArc::FULL, DhtArc::FULL])` will
    /// return `vec![DhtArc::FULL]`.
    ///
    /// This method is intended to be used after taking an intersection with [ArcSet::intersection].
    /// In that case, the list of arcs is not known and converting to a list of arcs is useful.
    pub fn as_arcs(&self) -> Vec<DhtArc> {
        if self.inner.is_empty() {
            return vec![];
        } else if self.inner.len() == 512 {
            return vec![DhtArc::FULL];
        }

        let mut arcs = vec![];

        let mut sectors = self.inner.iter().copied().collect::<Vec<_>>();
        sectors.sort();

        let mut start = 0;
        while start < sectors.len() {
            // Determine consecutive sectors.
            let mut end = start;
            while end + 1 < sectors.len()
                && sectors[end] + 1 == sectors[end + 1]
            {
                end += 1;
            }

            let start_sector = sectors[start];
            let end_sector = sectors[end];

            let top = (end_sector + 1).saturating_mul(SECTOR_SIZE);
            arcs.push(DhtArc::Arc(
                start_sector * SECTOR_SIZE,
                if top == u32::MAX { u32::MAX } else { top - 1 },
            ));

            start = end + 1;
        }

        arcs
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn new_with_no_arcs() {
        let set = ArcSet::new(vec![]).unwrap();

        assert!(set.inner.is_empty());
    }

    #[test]
    fn new_with_full_arc() {
        let set = ArcSet::new(vec![DhtArc::FULL]).unwrap();

        // Sufficient to check that all the right values are included
        assert_eq!(512, set.inner.len());
        assert_eq!(511, *set.inner.iter().max().unwrap());
    }

    #[test]
    fn new_with_two_arcs() {
        let set = ArcSet::new(vec![
            DhtArc::Arc(0, 255 * SECTOR_SIZE - 1),
            DhtArc::Arc(255 * SECTOR_SIZE, u32::MAX),
        ])
        .unwrap();

        // Should become a full arc
        assert_eq!(512, set.inner.len());
        assert_eq!(511, *set.inner.iter().max().unwrap());
    }

    #[test]
    fn overlapping_arcs() {
        let set = ArcSet::new(vec![
            DhtArc::Arc(0, 3 * SECTOR_SIZE - 1),
            DhtArc::Arc(2 * SECTOR_SIZE, 4 * SECTOR_SIZE - 1),
        ])
        .unwrap();

        assert_eq!(4, set.inner.len());
        assert_eq!(3, *set.inner.iter().max().unwrap());
    }

    #[test]
    fn wrapping_arc() {
        let set = ArcSet::new(vec![DhtArc::Arc(
            510 * SECTOR_SIZE,
            3 * SECTOR_SIZE - 1,
        )])
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
        let set = ArcSet::new(vec![
            DhtArc::Arc(510 * SECTOR_SIZE, 3 * SECTOR_SIZE - 1),
            DhtArc::Arc(2 * SECTOR_SIZE, 4 * SECTOR_SIZE - 1),
        ])
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
        let set = ArcSet::new(vec![DhtArc::Arc(0, 50)]);

        assert!(set.is_err());
        assert_eq!(
            "Invalid arc, expected end at 8388608 but arc specifies 50 (src: None)",
            set.unwrap_err().to_string()
        );
    }

    #[test]
    fn valid_and_invalid_arcs() {
        let set = ArcSet::new(vec![
            DhtArc::Arc(0, SECTOR_SIZE - 1),
            DhtArc::Arc(u32::MAX, u32::MAX),
        ]);

        assert!(set.is_err());
        assert_eq!(
            "Invalid arc, expected end at 8388607 but arc specifies 4294967295 (src: None)",
            set.unwrap_err().to_string()
        );
    }

    #[test]
    fn intersect_non_overlapping_sets() {
        let set1 = ArcSet::new(vec![DhtArc::Arc(0, SECTOR_SIZE - 1)]).unwrap();
        let set2 = ArcSet::new(vec![DhtArc::Arc(
            2 * SECTOR_SIZE,
            3 * SECTOR_SIZE - 1,
        )])
        .unwrap();

        let intersection = set1.intersection(&set2);

        assert!(intersection.inner.is_empty());
    }

    #[test]
    fn intersect_overlapping_by_one() {
        let set1 =
            ArcSet::new(vec![DhtArc::Arc(0, 2 * SECTOR_SIZE - 1)]).unwrap();
        let set2 =
            ArcSet::new(vec![DhtArc::Arc(SECTOR_SIZE, 3 * SECTOR_SIZE - 1)])
                .unwrap();

        let intersection = set1.intersection(&set2);

        assert_eq!(1, intersection.inner.len());
        assert!(intersection.inner.contains(&1));
    }

    #[test]
    fn intersect_overlapping_by_multiple() {
        let set1 =
            ArcSet::new(vec![DhtArc::Arc(0, 10 * SECTOR_SIZE - 1)]).unwrap();
        let set2 =
            ArcSet::new(vec![DhtArc::Arc(SECTOR_SIZE, 3 * SECTOR_SIZE - 1)])
                .unwrap();

        let intersection = set1.intersection(&set2);

        assert_eq!(2, intersection.inner.len());
        assert!(intersection.inner.contains(&1));
        assert!(intersection.inner.contains(&2));
    }

    #[test]
    fn preserves_full_arc() {
        let full_set = ArcSet::new(vec![DhtArc::FULL]).unwrap();
        assert_eq!(
            full_set,
            full_set.intersection(&ArcSet::new(vec![DhtArc::FULL]).unwrap())
        );
    }

    #[test]
    fn preserves_empty() {
        let empty_set = ArcSet::new(vec![DhtArc::Empty]).unwrap();
        assert_eq!(
            empty_set,
            empty_set.intersection(&ArcSet::new(vec![DhtArc::FULL]).unwrap())
        );
    }

    #[test]
    fn encode_decode_empty() {
        let set = ArcSet::new(vec![DhtArc::Empty]).unwrap();
        let encoded = set.encode();
        let decoded = ArcSet::decode(&encoded).unwrap();

        assert_eq!(set, decoded);
    }

    #[test]
    fn encode_decode_full() {
        let set = ArcSet::new(vec![DhtArc::FULL]).unwrap();
        let encoded = set.encode();
        let decoded = ArcSet::decode(&encoded).unwrap();

        assert_eq!(set, decoded);
    }

    #[test]
    fn encode_decode_sparse() {
        let set = ArcSet::new(vec![
            DhtArc::Arc(0, SECTOR_SIZE - 1),
            DhtArc::Arc(20 * SECTOR_SIZE, 21 * SECTOR_SIZE - 1),
        ])
        .unwrap();

        let encoded = set.encode();
        let decoded = ArcSet::decode(&encoded).unwrap();

        assert_eq!(set, decoded);
    }

    #[test]
    fn as_arcs_empty() {
        let set = ArcSet::new(vec![DhtArc::Empty]).unwrap();
        assert_eq!(set.as_arcs(), vec![]);
    }

    #[test]
    fn as_arcs_full() {
        let set = ArcSet::new(vec![DhtArc::FULL]).unwrap();
        assert_eq!(set.as_arcs(), vec![DhtArc::FULL]);
    }

    #[test]
    fn as_arcs_full_from_overlap() {
        let set = ArcSet::new(vec![
            DhtArc::Arc(0, 2 * SECTOR_SIZE - 1),
            DhtArc::Arc(SECTOR_SIZE, u32::MAX),
        ])
        .unwrap();
        assert_eq!(set.as_arcs(), vec![DhtArc::FULL]);
    }

    #[test]
    fn as_arcs_multiple_disjoint() {
        let arc_1 = DhtArc::Arc(0, 2 * SECTOR_SIZE - 1);
        let arc_2 = DhtArc::Arc(10 * SECTOR_SIZE, 40 * SECTOR_SIZE - 1);
        let set = ArcSet::new(vec![arc_1, arc_2]).unwrap();

        assert_eq!(set.as_arcs(), vec![arc_1, arc_2]);
    }

    #[test]
    fn as_arcs_full_on_wrap() {
        let set = ArcSet::new(vec![
            DhtArc::Arc(SECTOR_SIZE, 2 * SECTOR_SIZE - 1),
            DhtArc::Arc(2 * SECTOR_SIZE, SECTOR_SIZE - 1),
        ])
        .unwrap();

        assert_eq!(set.as_arcs(), vec![DhtArc::FULL]);
    }

    #[test]
    fn as_arcs_split_on_wrap() {
        let set = ArcSet::new(vec![DhtArc::Arc(
            510 * SECTOR_SIZE,
            2 * SECTOR_SIZE - 1,
        )])
        .unwrap();

        assert_eq!(
            set.as_arcs(),
            vec![
                DhtArc::Arc(0, 2 * SECTOR_SIZE - 1),
                DhtArc::Arc(510 * SECTOR_SIZE, u32::MAX)
            ]
        );
    }

    #[test]
    fn debug_arc_set() {
        assert_eq!(
            "ArcSet { sectors: \"Empty\" }",
            format!("{:?}", ArcSet::new(vec![DhtArc::Empty]).unwrap())
        );

        let set =
            ArcSet::new(vec![DhtArc::Arc(0, 2 * SECTOR_SIZE - 1)]).unwrap();
        assert_eq!("ArcSet { sectors: 2 }", format!("{set:?}"));

        let set = ArcSet::new(vec![
            DhtArc::Arc(0, 2 * SECTOR_SIZE - 1),
            DhtArc::Arc(SECTOR_SIZE, 3 * SECTOR_SIZE - 1),
        ])
        .unwrap();
        assert_eq!("ArcSet { sectors: 3 }", format!("{set:?}"));

        assert_eq!(
            "ArcSet { sectors: \"Full\" }",
            format!("{:?}", ArcSet::new(vec![DhtArc::FULL]).unwrap())
        );
    }
}
