//! Test utilities associated with spaces.

use bytes::Bytes;
use kitsune2_api::{id::Id, SpaceId};

/// A test space id.
pub const TEST_SPACE_ID: SpaceId =
    SpaceId(Id(Bytes::from_static(b"test_space")));
