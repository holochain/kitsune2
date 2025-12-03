use crate::MAX_FRAME_BYTES;
use bytes::Bytes;
use iroh::endpoint::Connection;
use kitsune2_api::{K2Error, K2Result, Url};
use std::sync::Arc;

pub(super) const FRAME_HEADER_LEN: usize = 5;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum FrameType {
    // Preflight consists of peer URL and preflight
    Preflight = 0,
    Data = 1,
}

impl TryFrom<u8> for FrameType {
    type Error = K2Error;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(FrameType::Preflight),
            1 => Ok(FrameType::Data),
            _ => {
                Err(K2Error::other(format!("unknown iroh frame type: {value}")))
            }
        }
    }
}

#[derive(Debug)]
pub(super) enum Frame {
    Preflight((Url, Bytes)),
    Data(Bytes),
}

/// Encodes a frame header for the given frame type and payload length.
///
/// The frame format consists of:
/// - 1 byte for frame type (0 for Preflight, 1 for Data)
/// - 4 bytes for payload length (big-endian u32)
///
/// Returns an error if the total frame size exceeds [`MAX_FRAME_BYTES`].
fn encode_frame_header(ty: FrameType, data_len: usize) -> K2Result<Vec<u8>> {
    if data_len + FRAME_HEADER_LEN > MAX_FRAME_BYTES {
        return Err(K2Error::other("frame too large"));
    }
    let mut header = Vec::with_capacity(FRAME_HEADER_LEN);
    header.push(ty as u8);
    header.extend_from_slice(&(data_len as u32).to_be_bytes());
    Ok(header)
}

/// Encodes a given frame.
///
/// The frame format consists of:
/// - 1 byte for frame type (0 for Preflight, 1 for Data)
/// - 4 bytes for payload length (big-endian u32)
/// - The payload data following the header
///
/// Payload data can be either the preflight or a message.
/// The preflight consists of:
/// - 4 bytes for the URL length
/// - The URL
/// - The preflight bytes
///
/// Messages are just the bytes of the payload.
///
/// # Errors
///
/// Returns an error when max frame bytes is exceeded.
pub(super) fn encode_frame(frame: Frame) -> K2Result<Bytes> {
    match frame {
        Frame::Preflight((url, preflight)) => {
            let url_bytes = Bytes::copy_from_slice(url.as_str().as_bytes());
            let mut data = vec![];
            data.extend_from_slice(&(url_bytes.len() as u32).to_be_bytes());
            data.extend_from_slice(&url_bytes);
            data.extend_from_slice(&preflight);
            let mut frame =
                encode_frame_header(FrameType::Preflight, data.len())?;
            frame.extend(&data);
            Ok(Bytes::copy_from_slice(&frame))
        }
        Frame::Data(data) => {
            let mut frame = encode_frame_header(FrameType::Data, data.len())?;
            frame.extend(&data);
            Ok(Bytes::copy_from_slice(&frame))
        }
    }
}

/// Decodes a frame from raw byte data into a frame type and payload.
///
/// The frame format consists of:
/// - 1 byte for frame type (0 for Preflight, 1 for Data)
/// - 4 bytes for payload length (big-endian u32)
/// - The payload data following the header
///
/// Payload data can be either the preflight or a message.
/// The preflight consists of:
/// - 4 bytes for the URL length
/// - The URL
/// - The preflight bytes
///
/// Messages are just the bytes of the payload.
///
/// # Errors
///
/// Returns an error if the data is shorter than the header, contains an invalid frame type,
/// or if the payload length doesn't match the actual data length.
///
/// In case of preflight frames, an error is returned for invalid URLs.
pub(super) fn decode_frame(data: Vec<u8>) -> K2Result<Frame> {
    if data.len() < FRAME_HEADER_LEN {
        return Err(K2Error::other(
            "iroh frame shorter than header".to_string(),
        ));
    }
    // Parse frame type from header byte.
    let frame_type = FrameType::try_from(data[0])?;
    // Extract payload length from next 4 bytes.
    let payload_len =
        u32::from_be_bytes([data[1], data[2], data[3], data[4]]) as usize;
    if payload_len != data.len() - FRAME_HEADER_LEN {
        return Err(K2Error::other(
            "iroh frame payload length mismatch".to_string(),
        ));
    }
    // Extract payload data after header.
    let payload = Bytes::copy_from_slice(&data[FRAME_HEADER_LEN..]);
    let frame = match frame_type {
        FrameType::Preflight => {
            if payload.len() < 4 {
                return Err(K2Error::other(
                    "preflight payload too short for URL length",
                ));
            }
            // The first 4 bytes of the payload are the URL length...
            let url_len = u32::from_be_bytes([
                payload[0], payload[1], payload[2], payload[3],
            ]) as usize;
            // ...followed by the URL
            if payload.len() < 4 + url_len {
                return Err(K2Error::other(
                    "preflight payload too short for actual URL",
                ));
            }
            let url = Url::from_str(
                std::str::from_utf8(&payload[4..4 + url_len]).map_err(
                    |err| K2Error::other_src("invalid peer url", err),
                )?,
            )?;
            // The preflight takes up the rest of the payload,
            // after the URL length and the URL.
            let preflight_bytes =
                Bytes::copy_from_slice(&payload[4 + url_len..]);
            Frame::Preflight((url, preflight_bytes))
        }
        FrameType::Data => Frame::Data(payload),
    };
    Ok(frame)
}

/// Sends a frame over the iroh connection.
///
/// Opens a unidirectional stream, writes the frame and finishes the stream.
/// The frame size is limited to [`MAX_FRAME_BYTES`].
pub(super) async fn send_frame(
    conn: &Arc<Connection>,
    frame: Frame,
) -> K2Result<()> {
    let frame = encode_frame(frame)?;
    let mut stream = conn.open_uni().await.map_err(|err| {
        K2Error::other_src("failed to open iroh send stream", err)
    })?;
    stream
        .write_all(&frame)
        .await
        .map_err(|err| K2Error::other_src("failed to send frame", err))?;
    stream.finish().map_err(|err| {
        K2Error::other_src("failed to finish iroh stream", err)
    })?;
    Ok(())
}
