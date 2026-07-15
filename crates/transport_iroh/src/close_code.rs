//! Application-level close codes for iroh connections.
//!
//! A QUIC application close carries a numeric error code plus reason bytes.
//! These codes let the remote peer distinguish an intentional close from a
//! network failure, so it can release the connection quietly.

/// Application close code carried in the QUIC CONNECTION_CLOSE frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum CloseCode {
    /// Unknown close code treated as a connection failure.
    Unspecified = 0,
    /// The peer closed the connection intentionally. The close reason bytes
    /// carry a human-readable reason. Not a failure.
    Graceful = 1,
    /// The connection lost simultaneous-open resolution and was replaced by
    /// a preferred connection to the same peer. Not a failure.
    Superseded = 2,
}

impl CloseCode {
    /// Numeric wire representation of this close code.
    pub fn as_u8(self) -> u8 {
        self as u8
    }

    /// Maps a wire error code to a [`CloseCode`].
    ///
    /// Future unknown codes map to [`CloseCode::Unspecified`].
    pub fn from_wire(code: u64) -> Self {
        match code {
            1 => CloseCode::Graceful,
            2 => CloseCode::Superseded,
            _ => CloseCode::Unspecified,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_known_codes() {
        for code in [
            CloseCode::Unspecified,
            CloseCode::Graceful,
            CloseCode::Superseded,
        ] {
            assert_eq!(CloseCode::from_wire(code.as_u8() as u64), code);
        }
    }

    #[test]
    fn unknown_wire_code_maps_to_unspecified() {
        assert_eq!(CloseCode::from_wire(3), CloseCode::Unspecified);
        assert_eq!(CloseCode::from_wire(u64::MAX), CloseCode::Unspecified);
    }
}
