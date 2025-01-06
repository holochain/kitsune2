//! A glossary of terms used in the Kitsune2 project.
//!
//! ## Time slice
//! A time slice is a period of time. It is defined to be an interval from a start time,
//! inclusive, up to some end time, exclusive.
//!
//! Time slices come in two flavours: full and partial. A full time slice is a time slice that
//! covers a time interval that is maximally large, according to configuration. This represents an
//! amount of historical time. A partial time slice represents a time interval that is smaller than
//! a full time slice and is used to represent a time interval that is more recent.
//!
//! ## DHT
//! A Distributed Hash Table (DHT) is a data structure that is keyed by a hash value and distributed
//! across a network of nodes. The DHT is used to store and retrieve data based on the hash key.
//!
//! ## DHT: Sector
//! In the context of the DHT, a sector is a range of hash locations. It is equivalent to a DhtArc
//! but the sectors are equally-sized and cover the entire hash space without overlapping.
//!
//! ## DHT: Disc
//! In the context of the DHT, the disc is the set of data with a hash at any location, that have
//! a timestamp before the end of the most recent full time slice.
//!
//! ## DHT: Ring
//! In the context of the DHT, a ring is the set of data with a hash at any location, that have a
//! timestamp between the start and end times of a given partial time slice.
//!
//! ## Combined hash
//! A combined hash is a value that is computed by combining hashes. For example, taking all the
//! data hashes within a sector and a full time slice and combining them to produce a single hash.
//!
//! ## Top hash
//! A top hash is a hash value that is computed by combining some combined hashes. For example,
//! taking all the combined hashes for a ring and combining them to produce a single hash.
//!
//! The natural names are given to these in the code. For example "disc top hash" is the top hash
//! formed by combining full time slice combined hashes and then combining those over sectors.
//!
//! Note that a top hash is not necessarily complete. When computing a disc top hash for example,
//! only the portion of the location space that is covered by both parties who are involved in
//! computing a DHT diff is considered. This is still referred to as a disc top hash in the code,
//! although in reality it is actually only a part of the disc.
//!
