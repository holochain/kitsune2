//! Kitsune2 arcs represent a range of hash locations on the DHT.
//!
//! An arc can exist in its own right and refer to a range of locations on the DHT.
//! It can also be used in context, such as an agent. Where it represents the range of locations
//! that agent is responsible for.

use serde::{Deserialize, Serialize};

/// A basic definition of a storage arc compatible with the concept of
/// storage and querying of items in a store that fall within that arc.
#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq, Default)]
pub enum BasicArc {
    /// No DHT locations are contained within this arc.
    #[default]
    Empty,
    /// All DHT locations are contained within this arc.
    Full,
    /// A specific range of DHT locations are contained within this arc.
    ///
    /// The lower bound is inclusive, the upper bound is exclusive.
    Literal(u32, u32),
}

impl BasicArc {
    /// Create a new arc.
    ///
    /// If both values are 0 then a full arc is returned.
    /// Otherwise, equal values for start and end will result in an empty arc.
    /// Otherwise, a literal arc is returned.
    ///
    /// It is valid to create a [BasicArc] directly when using the Full and Empty variants.
    /// However, when creating an arc from bounds, it is recommended to use this function for
    /// consistency.
    pub fn new(start: u32, end: u32) -> Self {
        if start == end {
            if start == 0 {
                BasicArc::Full
            } else {
                BasicArc::Empty
            }
        } else {
            BasicArc::Literal(start, end)
        }
    }

    /// Convert to a canonical form.
    ///
    /// This function will return a new arc that is equivalent to the original arc,
    /// See the documentation for [BasicArc::new].
    pub fn canonicalize(&self) -> Self {
        match self {
            BasicArc::Empty => BasicArc::Empty,
            BasicArc::Full => BasicArc::Full,
            BasicArc::Literal(start, end) => Self::new(*start, *end),
        }
    }

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
    /// Arc wraps around, loc < arc_end
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
    /// Arc does not wrap around, loc >= arc_end
    /// |---------s------e--l--|
    /// 0                      u32::MAX
    /// ```
    pub fn dist(&self, loc: u32) -> u32 {
        match self.canonicalize() {
            BasicArc::Empty => u32::MAX,
            BasicArc::Full => 0,
            BasicArc::Literal(arc_start, arc_end) => {
                let (d1, d2) = if arc_start > arc_end {
                    // this arc wraps around the end of u32::MAX

                    if loc >= arc_start || loc < arc_end {
                        return 0;
                    } else {
                        (
                            // Here we know that location is less than arc_start
                            arc_start - loc,
                            // Here we know that location is greater than or equal to arc_end, but
                            // because arc_start > arc_end, we know that we actually know that
                            // location is greater than arc_end. Therefore, this cannot overflow.
                            // Add one to account for the exclusive upper bound.
                            loc - arc_end + 1,
                        )
                    }
                } else {
                    // this arc does not wrap, arc_start <= arc_end

                    if loc >= arc_start && loc < arc_end {
                        return 0;
                    } else if loc < arc_start {
                        (
                            // Here we know that location is less than arc_start
                            arc_start - loc,
                            // Add one to account for the wrap and add one to account for the
                            // distance to exclusive upper bound.
                            // Here we know that location is less than arc_end, but we need to
                            // compute the wrapping distance between them. Adding 1 would be safe
                            // but adding 2 could overflow if arc_start == arc_end and
                            // loc = arc_start - 1.
                            (u32::MAX - arc_end + loc).saturating_add(2),
                        )
                    } else {
                        (
                            u32::MAX - loc + arc_start + 1,
                            // Add one to account for the exclusive upper bound.
                            // Here we know that location is greater than or equal to arc_end.
                            // Therefore, this could overflow if loc == u32::MAX and arc_end == 0.
                            (loc - arc_end).saturating_add(1),
                        )
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
    pub fn overlaps(&self, other: &BasicArc) -> bool {
        match (&self.canonicalize(), &other.canonicalize()) {
            (BasicArc::Empty, _) | (_, BasicArc::Empty) => false,
            (BasicArc::Full, _) | (_, BasicArc::Full) => true,
            (
                this @ BasicArc::Literal(a_beg, a_end),
                other @ BasicArc::Literal(b_beg, b_end),
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
    use crate::BasicArc;

    #[test]
    fn canonicalize_already_correct() {
        let arc = BasicArc::Empty;
        assert_eq!(arc, arc.canonicalize());

        let arc = BasicArc::Full;
        assert_eq!(arc, arc.canonicalize());

        let arc = BasicArc::Literal(32, 64);
        assert_eq!(arc, arc.canonicalize());

        let arc = BasicArc::Literal(64, 32);
        assert_eq!(BasicArc::Literal(64, 32), arc.canonicalize());
    }

    #[test]
    fn canonicalize_empty() {
        let arc = BasicArc::Literal(32, 32);
        assert_eq!(BasicArc::Empty, arc.canonicalize());

        let arc = BasicArc::Literal(1, 1);
        assert_eq!(BasicArc::Empty, arc.canonicalize());

        let arc = BasicArc::Literal(u32::MAX, u32::MAX);
        assert_eq!(BasicArc::Empty, arc.canonicalize());
    }

    #[test]
    fn canonicalize_full() {
        let arc = BasicArc::Literal(0, 0);
        assert_eq!(BasicArc::Full, arc.canonicalize());
    }

    #[test]
    fn full_arc_includes_all_values() {
        let arc = BasicArc::Full;

        // Contains bounds
        assert!(arc.contains(0));
        assert!(arc.contains(u32::MAX));

        // and a value in the middle somewhere
        assert!(arc.contains(u32::MAX / 2));
    }

    #[test]
    fn arc_includes_start_but_not_end() {
        let arc = BasicArc::Literal(32, 64);

        assert!(arc.contains(32));
        assert!(arc.contains(40));

        assert!(!arc.contains(64));
    }

    #[test]
    fn arc_wraps_around() {
        let arc = BasicArc::Literal(u32::MAX - 32, 32);

        assert!(arc.contains(u32::MAX - 1));
        assert!(arc.contains(u32::MAX));
        assert!(arc.contains(20));

        assert!(!arc.contains(32));
    }

    #[test]
    fn arc_dist_edge_cases() {
        type Dist = u32;
        type Loc = u32;
        const F: &[(Dist, Loc, BasicArc)] = &[
            // Empty arcs contain no values, distance is always u32::MAX
            (u32::MAX, 0, BasicArc::Empty),
            (u32::MAX, u32::MAX / 2, BasicArc::Empty),
            (u32::MAX, u32::MAX, BasicArc::Empty),
            // Empty arc represented as literal
            (u32::MAX, 0, BasicArc::Literal(u32::MAX, u32::MAX)),
            (
                u32::MAX,
                u32::MAX / 2,
                BasicArc::Literal(u32::MAX, u32::MAX),
            ),
            // Full arc, represented as literal
            (0, u32::MAX, BasicArc::Literal(0, 0)),
            (0, u32::MAX / 2, BasicArc::Literal(0, 0)),
            // Lower bound is inclusive
            (0, 0, BasicArc::Literal(0, 1)),
            (0, u32::MAX - 1, BasicArc::Literal(u32::MAX - 1, u32::MAX)),
            // Distance from lower bound, non-wrapping
            (1, 0, BasicArc::Literal(1, 2)),
            (1, u32::MAX, BasicArc::Literal(0, 1)),
            // Distance from upper bound, non-wrapping
            (2, 0, BasicArc::Literal(u32::MAX - 1, u32::MAX)),
            // Distance from upper bound, wrapping
            (1, 0, BasicArc::Literal(u32::MAX, 0)),
            (2, 1, BasicArc::Literal(u32::MAX, 0)),
            // Distance from lower bound, wrapping
            (1, u32::MAX - 1, BasicArc::Literal(u32::MAX, 0)),
            (1, u32::MAX - 1, BasicArc::Literal(u32::MAX, 1)),
            // Contains, wrapping
            (0, 0, BasicArc::Literal(u32::MAX, 1)),
        ];

        for (dist, loc, arc) in F.iter() {
            assert_eq!(
                *dist,
                arc.dist(*loc),
                "While checking that {:?} contains {}",
                arc,
                loc
            );
        }
    }

    #[test]
    fn arcs_overlap_edge_cases() {
        type DoOverlap = bool;
        const F: &[(DoOverlap, BasicArc, BasicArc)] = &[
            (false, BasicArc::Literal(0, 0), BasicArc::Literal(1, 1)),
            (
                false,
                BasicArc::Literal(0, 0),
                BasicArc::Literal(u32::MAX, u32::MAX),
            ),
            (true, BasicArc::Literal(0, 0), BasicArc::Literal(0, 0)),
            (
                false,
                BasicArc::Literal(u32::MAX, u32::MAX),
                BasicArc::Literal(u32::MAX, u32::MAX),
            ),
            (
                true,
                BasicArc::Literal(u32::MAX, 0),
                BasicArc::Literal(0, 0),
            ),
            (
                false,
                BasicArc::Literal(u32::MAX, 0),
                BasicArc::Literal(u32::MAX, u32::MAX),
            ),
            (
                false,
                BasicArc::Literal(u32::MAX, 0),
                BasicArc::Literal(u32::MAX, u32::MAX),
            ),
            (true, BasicArc::Literal(0, 3), BasicArc::Literal(1, 2)),
            (true, BasicArc::Literal(1, 2), BasicArc::Literal(0, 3)),
            (true, BasicArc::Literal(1, 3), BasicArc::Literal(2, 4)),
            (true, BasicArc::Literal(2, 4), BasicArc::Literal(1, 3)),
            (
                true,
                BasicArc::Literal(u32::MAX - 1, 1),
                BasicArc::Literal(u32::MAX, 0),
            ),
            (
                true,
                BasicArc::Literal(u32::MAX, 0),
                BasicArc::Literal(u32::MAX - 1, 1),
            ),
            (
                true,
                BasicArc::Literal(u32::MAX - 1, 0),
                BasicArc::Literal(u32::MAX, 1),
            ),
            (
                true,
                BasicArc::Literal(u32::MAX, 1),
                BasicArc::Literal(u32::MAX - 1, 0),
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
