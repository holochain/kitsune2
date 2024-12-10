//! Kitsune2 arcs represent a range of hash locations on the DHT.
//!
//! An arc can exist in its own right and refer to a range of locations on the DHT.
//! It can also be used in context, such as an agent. Where it represents the range of locations
//! that agent is responsible for.

use serde::{Deserialize, Serialize};

/// The definition of a storage arc compatible with the concept of
/// storage and querying of items in a store that fall within that arc.
#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq, Default)]
#[serde(untagged)]
pub enum StorageArc {
    /// No DHT locations are contained within this arc.
    #[default]
    Empty,
    /// A specific range of DHT locations are contained within this arc.
    ///
    /// The lower and upper bounds are inclusive.
    Arc(u32, u32),
}

impl StorageArc {
    /// A full arc that contains all DHT locations.
    pub const FULL: StorageArc = StorageArc::Arc(0, u32::MAX);

    /// Get the min distance from a location to an arc in a wrapping u32 space.
    /// This function will only return 0 if the location is covered by the arc.
    /// This function will return u32::MAX if the arc is empty.
    ///
    /// All possible cases:
    ///
    /// ```text
    /// s = arc_start
    /// e = arc_end
    /// l = location
    ///
    /// Arc wraps around, loc >= arc_start
    ///
    /// |----e-----------s--l--|
    /// 0                      u32::MAX
    ///
    /// Arc wraps around, loc <= arc_end
    /// |-l--e-----------s-----|
    /// 0                      u32::MAX
    ///
    /// Arc wraps around, loc outside of arc
    /// |----e----l------s-----|
    /// 0                      u32::MAX
    ///
    /// Arc does not wrap around, loc inside of arc
    /// |---------s--l---e-----|
    /// 0                      u32::MAX
    ///
    /// Arc does not wrap around, loc < arc_start
    /// |-----l---s------e-----|
    /// 0                      u32::MAX
    ///
    /// Arc does not wrap around, loc > arc_end
    /// |---------s------e--l--|
    /// 0                      u32::MAX
    /// ```
    pub fn dist(&self, loc: u32) -> u32 {
        match self {
            StorageArc::Empty => u32::MAX,
            StorageArc::Arc(arc_start, arc_end) => {
                let (d1, d2) = if arc_start > arc_end {
                    // this arc wraps around the end of u32::MAX

                    if loc >= *arc_start || loc <= *arc_end {
                        return 0;
                    } else {
                        (
                            // Here we know that location is less than arc_start
                            arc_start - loc,
                            loc - arc_end,
                        )
                    }
                } else {
                    // this arc does not wrap, arc_start <= arc_end

                    if loc >= *arc_start && loc <= *arc_end {
                        return 0;
                    } else if loc < *arc_start {
                        (
                            // Here we know that location is less than arc_start
                            arc_start - loc,
                            // Add one to account for the wrap.
                            // Here we know that location is less than arc_end, but we need to
                            // compute the wrapping distance between them. Adding 1 is safe and
                            // cannot overflow.
                            u32::MAX - arc_end + loc + 1,
                        )
                    } else {
                        (u32::MAX - loc + arc_start + 1, loc - arc_end)
                    }
                };

                std::cmp::min(d1, d2)
            }
        }
    }

    /// Convenience function to determine if a location is contained within the arc.
    ///
    /// Simply checks whether the distance from the location to the arc is 0.
    pub fn contains(&self, loc: u32) -> bool {
        self.dist(loc) == 0
    }

    /// Determine if any part of two arcs overlap.
    ///
    /// All possible cases (though note the arcs can also wrap around u32::MAX):
    ///
    /// ```text
    /// a = a_start
    /// A = a_end
    /// b = b_start
    /// B = b_end
    ///
    /// The tail of a..A overlaps the head of b..B
    ///
    /// |---a--b-A--B---|
    ///
    /// The tail of b..B overlaps the head of a..A
    ///
    /// |---b--a-B--A---|
    ///
    /// b..B is fully contained by a..A
    ///
    /// |---a--b-B--A---|
    ///
    /// a..A is fully contained by b..B
    ///
    /// |---b--a-A--B---|
    /// ```
    pub fn overlaps(&self, other: &StorageArc) -> bool {
        match (&self, &other) {
            (StorageArc::Empty, _) | (_, StorageArc::Empty) => false,
            (
                this @ StorageArc::Arc(a_beg, a_end),
                other @ StorageArc::Arc(b_beg, b_end),
            ) => {
                // The only way for there to be overlap is if
                // either of a's start or end points are within b
                // or either of b's start or end points are within a
                this.dist(*b_beg) == 0
                    || this.dist(*b_end) == 0
                    || other.dist(*a_beg) == 0
                    || other.dist(*a_end) == 0
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::StorageArc;

    #[test]
    fn contains_full_arc_all_values() {
        let arc = StorageArc::FULL;

        // Contains bounds
        assert!(arc.contains(0));
        assert!(arc.contains(u32::MAX));

        // and a value in the middle somewhere
        assert!(arc.contains(u32::MAX / 2));
    }

    #[test]
    fn contains_includes_bounds() {
        let arc = StorageArc::Arc(32, 64);

        assert!(arc.contains(32));
        assert!(arc.contains(64));
    }

    #[test]
    fn contains_wraps_around() {
        let arc = StorageArc::Arc(u32::MAX - 32, 32);

        assert!(!arc.contains(u32::MAX - 33));
        assert!(arc.contains(u32::MAX - 32));
        assert!(arc.contains(u32::MAX - 1));
        assert!(arc.contains(u32::MAX));
        assert!(arc.contains(0));
        assert!(arc.contains(20));
        assert!(arc.contains(32));
        assert!(!arc.contains(33));
    }

    #[test]
    fn arc_dist_edge_cases() {
        type Dist = u32;
        type Loc = u32;
        const F: &[(Dist, Loc, StorageArc)] = &[
            // Empty arcs contain no values, distance is always u32::MAX
            (u32::MAX, 0, StorageArc::Empty),
            (u32::MAX, u32::MAX / 2, StorageArc::Empty),
            (u32::MAX, u32::MAX, StorageArc::Empty),
            // Unit length arcs at max value
            (1, 0, StorageArc::Arc(u32::MAX, u32::MAX)),
            (
                u32::MAX / 2 + 1,
                u32::MAX / 2,
                StorageArc::Arc(u32::MAX, u32::MAX),
            ),
            // Unit length arcs at min value
            (1, u32::MAX, StorageArc::Arc(0, 0)),
            (u32::MAX / 2, u32::MAX / 2, StorageArc::Arc(0, 0)),
            // Lower bound is inclusive
            (0, 0, StorageArc::Arc(0, 1)),
            (0, u32::MAX - 1, StorageArc::Arc(u32::MAX - 1, u32::MAX)),
            // Distance from lower bound, non-wrapping
            (1, 0, StorageArc::Arc(1, 2)),
            (1, u32::MAX, StorageArc::Arc(0, 1)),
            // Distance from upper bound, non-wrapping
            (1, 0, StorageArc::Arc(u32::MAX - 1, u32::MAX)),
            // Distance from upper bound, wrapping
            (0, 0, StorageArc::Arc(u32::MAX, 0)),
            (1, 1, StorageArc::Arc(u32::MAX, 0)),
            // Distance from lower bound, wrapping
            (1, u32::MAX - 1, StorageArc::Arc(u32::MAX, 0)),
            (1, u32::MAX - 1, StorageArc::Arc(u32::MAX, 1)),
            // Contains, wrapping
            (0, 0, StorageArc::Arc(u32::MAX, 1)),
        ];

        for (dist, loc, arc) in F.iter() {
            assert_eq!(
                *dist,
                arc.dist(*loc),
                "While checking the distance from {} to arc {:?}",
                loc,
                arc
            );
        }
    }

    #[test]
    fn arcs_overlap_edge_cases() {
        type DoOverlap = bool;
        const F: &[(DoOverlap, StorageArc, StorageArc)] = &[
            (false, StorageArc::Arc(0, 0), StorageArc::Arc(1, 1)),
            (
                false,
                StorageArc::Arc(0, 0),
                StorageArc::Arc(u32::MAX, u32::MAX),
            ),
            (true, StorageArc::Arc(0, 0), StorageArc::Arc(0, 0)),
            (
                true,
                StorageArc::Arc(u32::MAX, u32::MAX),
                StorageArc::Arc(u32::MAX, u32::MAX),
            ),
            (true, StorageArc::Arc(u32::MAX, 0), StorageArc::Arc(0, 0)),
            (
                true,
                StorageArc::Arc(u32::MAX, 0),
                StorageArc::Arc(u32::MAX, u32::MAX),
            ),
            (
                true,
                StorageArc::Arc(u32::MAX, 0),
                StorageArc::Arc(u32::MAX, u32::MAX),
            ),
            (true, StorageArc::Arc(0, 3), StorageArc::Arc(1, 2)),
            (true, StorageArc::Arc(1, 2), StorageArc::Arc(0, 3)),
            (true, StorageArc::Arc(1, 3), StorageArc::Arc(2, 4)),
            (true, StorageArc::Arc(2, 4), StorageArc::Arc(1, 3)),
            (
                true,
                StorageArc::Arc(u32::MAX - 1, 1),
                StorageArc::Arc(u32::MAX, 0),
            ),
            (
                true,
                StorageArc::Arc(u32::MAX, 0),
                StorageArc::Arc(u32::MAX - 1, 1),
            ),
            (
                true,
                StorageArc::Arc(u32::MAX - 1, 0),
                StorageArc::Arc(u32::MAX, 1),
            ),
            (
                true,
                StorageArc::Arc(u32::MAX, 1),
                StorageArc::Arc(u32::MAX - 1, 0),
            ),
        ];

        for (do_overlap, a, b) in F.iter() {
            assert_eq!(
                *do_overlap,
                a.overlaps(b),
                "While checking that {:?} overlaps {:?}",
                a,
                b
            );
        }
    }
}
