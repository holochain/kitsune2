//! Constants used in the DHT model.

use std::time::Duration;

/// The smallest unit of time in the DHT.
pub const UNIT_TIME: Duration = Duration::from_secs(15 * 60); // 15 minutes

/// The size of a hash sector in the hash partition of the DHT.
///
/// Results in 512 sectors, because 32 - 23 = 9, and 2^9 = 512.
pub const SECTOR_SIZE: u32 = 1u32 << 23;
