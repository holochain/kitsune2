//! Constants used in the DHT model.

use std::time::Duration;

/// The smallest unit of time in the DHT.
pub const UNIT_TIME: Duration = Duration::from_secs(15 * 60); // 15 minutes
