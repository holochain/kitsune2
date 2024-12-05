//! Kitsune2 arcs represent a range of hash locations on the DHT.
//!
//! An arc can exist in its own right and refer to a range of locations on the DHT.
//! It can also be used in context, such as an agent. Where it represents the range of locations
//! that agent is responsible for.

/// The definition of an arc for Kitsune.
///
/// The lower bound is inclusive, the upper bound is exclusive.
pub type ArcLiteral = (u32, u32);

/// A full arc is represented by `(0, u32::MAX)`.
pub const ARC_LITERAL_FULL: ArcLiteral = (0, u32::MAX);

/// Check if a location is contained within the arc.
pub fn arc_contains(arc_literal: ArcLiteral, location: u32) -> bool {
    if arc_literal.0 < arc_literal.1 {
        location >= arc_literal.0 && location < arc_literal.1
    } else {
        location >= arc_literal.0 || location < arc_literal.1
    }
}

/// A basic definition of a storage arc compatible with the concept of
/// storage and querying of items in a store that fall within that arc.
///
/// This is intentionally a type definition and NOT a struct to prevent
/// the accumulation of functionality attached to it. This is intended
/// to transmit the raw concept of the arc, and ensure that any complexity
/// of its usage are hidden in the modules that need to use this raw data,
/// e.g. any store or gossip modules.
///
/// - If None, this arc does not claim any coverage.
/// - If Some, this arc is an inclusive range from the first loc to the second.
/// - If the first bound is larger than the second, the claim wraps around
///   the end of u32::MAX to the other side.
/// - A full arc is represented by `Some((0, u32::MAX))`.
pub type BasicArc = Option<ArcLiteral>;

/// An empty basic arc (`None`) is used for tombstone entries and for
/// light-weight nodes that cannot afford the storage and bandwidth of being
/// an authority.
pub const BASIC_ARC_EMPTY: BasicArc = None;
/// A full basic arc (`Some((0, u32::MAX))`) is used by nodes that wish to
/// claim authority over the full DHT.
pub const BASIC_ARC_FULL: BasicArc = Some(ARC_LITERAL_FULL);
