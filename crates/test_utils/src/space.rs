//! Test utilities associated with spaces.

use bytes::Bytes;
use kitsune2_api::*;

/// A test space id.
pub const TEST_SPACE_ID: SpaceId =
    SpaceId(Id(Bytes::from_static(b"test_space")));
/// Publish module
pub const MODULE_PUBLISH: &str = "Publish";
/// Gossip module
pub const MODULE_GOSSIP: &str = "k2gossip";
