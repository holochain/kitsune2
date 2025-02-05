//! Kitsune2 arcs represent a range of hash locations on the DHT.
//!
//! When used in the context of an agent, it represents the range of hash locations
//! that agent is responsible for.

use serde::{Deserialize, Serialize};

/// The definition of a storage arc compatible with the concept of
/// storage and querying of items in a store that fall within that arc.
#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, Hash, Eq, PartialEq, Default,
)]
#[serde(untagged)]
pub enum DhtArc {
    /// No DHT locations are contained within this arc.
    #[default]
    Empty,
    /// A specific range of DHT locations are contained within this arc.
    ///
    /// The lower and upper bounds are inclusive.
    Arc(u32, u32),
}

impl DhtArc {
    /// A full arc that contains all DHT locations.
    pub const FULL: DhtArc = DhtArc::Arc(0, u32::MAX);

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
            DhtArc::Empty => u32::MAX,
            DhtArc::Arc(arc_start, arc_end) => {
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
    pub fn overlaps(&self, other: &DhtArc) -> bool {
        match (&self, &other) {
            (DhtArc::Empty, _) | (_, DhtArc::Empty) => false,
            (
                this @ DhtArc::Arc(a_beg, a_end),
                other @ DhtArc::Arc(b_beg, b_end),
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

    /// Get the length of the arc.
    pub fn len(&self) -> u32 {
        match self {
            DhtArc::Empty => 0,
            DhtArc::Arc(start, end) => {
                if start > end {
                    u32::MAX - start + end + 1
                } else {
                    end - start + 1
                }
            }
        }
    }

    /// Determine if the arc is empty.
    pub fn is_empty(&self) -> bool {
        matches!(self, DhtArc::Empty)
    }
}

#[cfg(test)]
mod tests {
    use crate::DhtArc;

    #[test]
    fn contains_full_arc_all_values() {
        let arc = DhtArc::FULL;

        // Contains bounds
        assert!(arc.contains(0));
        assert!(arc.contains(u32::MAX));

        // and a value in the middle somewhere
        assert!(arc.contains(u32::MAX / 2));
    }

    #[test]
    fn contains_includes_bounds() {
        let arc = DhtArc::Arc(32, 64);

        assert!(arc.contains(32));
        assert!(arc.contains(64));
    }

    #[test]
    fn contains_wraps_around() {
        let arc = DhtArc::Arc(u32::MAX - 32, 32);

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
    fn contains_empty_arc_no_locations() {
        let arc = DhtArc::Empty;

        assert!(!arc.contains(0));
        assert!(!arc.contains(u32::MAX));
        assert!(!arc.contains(u32::MAX / 2));
    }

    #[test]
    fn arc_dist_edge_cases() {
        type Dist = u32;
        type Loc = u32;
        const F: &[(Dist, Loc, DhtArc)] = &[
            // Empty arcs contain no values, distance is always u32::MAX
            (u32::MAX, 0, DhtArc::Empty),
            (u32::MAX, u32::MAX / 2, DhtArc::Empty),
            (u32::MAX, u32::MAX, DhtArc::Empty),
            // Unit length arcs at max value
            (1, 0, DhtArc::Arc(u32::MAX, u32::MAX)),
            (
                u32::MAX / 2 + 1,
                u32::MAX / 2,
                DhtArc::Arc(u32::MAX, u32::MAX),
            ),
            // Unit length arcs at min value
            (1, u32::MAX, DhtArc::Arc(0, 0)),
            (u32::MAX / 2, u32::MAX / 2, DhtArc::Arc(0, 0)),
            // Lower bound is inclusive
            (0, 0, DhtArc::Arc(0, 1)),
            (0, u32::MAX - 1, DhtArc::Arc(u32::MAX - 1, u32::MAX)),
            // Distance from lower bound, non-wrapping
            (1, 0, DhtArc::Arc(1, 2)),
            (1, u32::MAX, DhtArc::Arc(0, 1)),
            // Distance from upper bound, non-wrapping
            (1, 0, DhtArc::Arc(u32::MAX - 1, u32::MAX)),
            // Distance from upper bound, wrapping
            (0, 0, DhtArc::Arc(u32::MAX, 0)),
            (1, 1, DhtArc::Arc(u32::MAX, 0)),
            // Distance from lower bound, wrapping
            (1, u32::MAX - 1, DhtArc::Arc(u32::MAX, 0)),
            (1, u32::MAX - 1, DhtArc::Arc(u32::MAX, 1)),
            // Contains, wrapping
            (0, 0, DhtArc::Arc(u32::MAX, 1)),
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
        const F: &[(DoOverlap, DhtArc, DhtArc)] = &[
            (false, DhtArc::Arc(0, 0), DhtArc::Arc(1, 1)),
            (false, DhtArc::Arc(0, 0), DhtArc::Arc(u32::MAX, u32::MAX)),
            (true, DhtArc::Arc(0, 0), DhtArc::Arc(0, 0)),
            (
                true,
                DhtArc::Arc(u32::MAX, u32::MAX),
                DhtArc::Arc(u32::MAX, u32::MAX),
            ),
            (true, DhtArc::Arc(u32::MAX, 0), DhtArc::Arc(0, 0)),
            (
                true,
                DhtArc::Arc(u32::MAX, 0),
                DhtArc::Arc(u32::MAX, u32::MAX),
            ),
            (
                true,
                DhtArc::Arc(u32::MAX, 0),
                DhtArc::Arc(u32::MAX, u32::MAX),
            ),
            (true, DhtArc::Arc(0, 3), DhtArc::Arc(1, 2)),
            (true, DhtArc::Arc(1, 2), DhtArc::Arc(0, 3)),
            (true, DhtArc::Arc(1, 3), DhtArc::Arc(2, 4)),
            (true, DhtArc::Arc(2, 4), DhtArc::Arc(1, 3)),
            (true, DhtArc::Arc(u32::MAX - 1, 1), DhtArc::Arc(u32::MAX, 0)),
            (true, DhtArc::Arc(u32::MAX, 0), DhtArc::Arc(u32::MAX - 1, 1)),
            (true, DhtArc::Arc(u32::MAX - 1, 0), DhtArc::Arc(u32::MAX, 1)),
            (true, DhtArc::Arc(u32::MAX, 1), DhtArc::Arc(u32::MAX - 1, 0)),
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
