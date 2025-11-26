use crate::MAX_FRAME_BYTES;
use bytes::Bytes;
use iroh::endpoint::Connection;
use kitsune2_api::{K2Error, K2Result};
use std::sync::Arc;

pub(super) const FRAME_HEADER_LEN: usize = 5;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum FrameType {
    PeerUrl = 0,
    Payload = 1,
}

impl TryFrom<u8> for FrameType {
    type Error = K2Error;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(FrameType::PeerUrl),
            1 => Ok(FrameType::Payload),
            _ => {
                Err(K2Error::other(format!("unknown iroh frame type: {value}")))
            }
        }
    }
}

/// Decodes a frame from raw byte data into a frame type and payload.
///
/// The frame format consists of:
/// - 1 byte for frame type (0 for PeerUrl, 1 for Payload)
/// - 4 bytes for payload length (big-endian u32)
/// - The payload data following the header
///
/// Returns an error if the data is shorter than the header, contains an invalid frame type,
/// or if the payload length doesn't match the actual data length.
pub(super) fn decode_frame(data: Vec<u8>) -> K2Result<(FrameType, Bytes)> {
    if data.len() < FRAME_HEADER_LEN {
        return Err(K2Error::other(
            "iroh frame shorter than header".to_string(),
        ));
    }
    // Parse frame type from header byte.
    let frame_type = FrameType::try_from(data[0])?;
    // Extract payload length from next 4 bytes.
    let len = u32::from_be_bytes([data[1], data[2], data[3], data[4]]) as usize;
    if data.len() - FRAME_HEADER_LEN != len {
        return Err(K2Error::other(
            "iroh frame payload length mismatch".to_string(),
        ));
    }
    // Extract payload data after header.
    let payload = Bytes::copy_from_slice(&data[FRAME_HEADER_LEN..]);
    Ok((frame_type, payload))
}

/// Encodes a frame header for the given frame type and payload length.
///
/// The frame format consists of:
/// - 1 byte for frame type (0 for PeerUrl, 1 for Payload)
/// - 4 bytes for payload length (big-endian u32)
///
/// Returns an error if the total frame size exceeds [`MAX_FRAME_BYTES`].
pub(super) fn encode_frame_header(
    ty: FrameType,
    data_len: usize,
) -> K2Result<[u8; FRAME_HEADER_LEN]> {
    if data_len + FRAME_HEADER_LEN > MAX_FRAME_BYTES {
        return Err(K2Error::other("frame too large"));
    }
    let mut header = [0u8; FRAME_HEADER_LEN];
    header[0] = ty as u8;
    header[1..5].copy_from_slice(&(data_len as u32).to_be_bytes());
    Ok(header)
}

/// Sends a frame over the iroh connection.
///
/// The frame format consists of:
/// - 1 byte for frame type (0 for PeerUrl, 1 for Payload)
/// - 4 bytes for payload length (big-endian u32)
/// - The payload data following the header
///
/// Opens a unidirectional stream, writes the frame header and data,
/// and finishes the stream. The frame size is limited to [`MAX_FRAME_BYTES`].
pub(super) async fn send_frame(
    conn: &Arc<Connection>,
    ty: FrameType,
    data: Bytes,
) -> K2Result<()> {
    let mut stream = conn.open_uni().await.map_err(|err| {
        K2Error::other_src("failed to open iroh send stream", err)
    })?;
    let header = encode_frame_header(ty, data.len())?;
    stream.write_all(&header).await.map_err(|err| {
        K2Error::other_src("failed to send frame header", err)
    })?;
    stream.write_all(&data).await.map_err(|err| {
        K2Error::other_src("failed to send frame payload", err)
    })?;
    stream.finish().map_err(|err| {
        K2Error::other_src("failed to finish iroh stream", err)
    })?;
    Ok(())
}
