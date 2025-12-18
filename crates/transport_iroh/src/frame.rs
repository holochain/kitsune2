use bytes::Bytes;
use kitsune2_api::{K2Error, K2Result, Url};

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
/// Returns an error if the total frame size exceeds `max_frame_bytes`.
fn encode_frame_header(
    ty: FrameType,
    data_len: usize,
    max_frame_bytes: usize,
) -> K2Result<Vec<u8>> {
    if data_len + FRAME_HEADER_LEN > max_frame_bytes {
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
/// Payload data can be either the preflight or data.
/// The preflight consists of:
/// - 4 bytes for the URL length
/// - The URL
/// - The preflight bytes
///
/// Data is just the bytes of the data.
///
/// # Errors
///
/// Returns an error when `max_frame_bytes` are exceeded.
pub(super) fn encode_frame(
    frame: Frame,
    max_frame_bytes: usize,
) -> K2Result<Bytes> {
    match frame {
        Frame::Preflight((url, preflight)) => {
            let url_bytes = Bytes::copy_from_slice(url.as_str().as_bytes());
            let mut data = vec![];
            data.extend_from_slice(&(url_bytes.len() as u32).to_be_bytes());
            data.extend_from_slice(&url_bytes);
            data.extend_from_slice(&preflight);
            let mut frame = encode_frame_header(
                FrameType::Preflight,
                data.len(),
                max_frame_bytes,
            )?;
            frame.extend(&data);
            Ok(Bytes::copy_from_slice(&frame))
        }
        Frame::Data(data) => {
            let mut frame = encode_frame_header(
                FrameType::Data,
                data.len(),
                max_frame_bytes,
            )?;
            frame.extend(&data);
            Ok(Bytes::copy_from_slice(&frame))
        }
    }
}

/// Decodes a frame header from raw byte data into a frame type and data length.
///
/// The frame header consists of:
/// - 1 byte for frame type (0 for Preflight, 1 for Data)
/// - 4 bytes for data length (big-endian u32)
///
/// The frame type and data length are returned as separate values.
///
/// # Errors
///
/// Returns an error if the data is shorter than the header, contains an invalid
/// frame type, or the data length plus frame header length exceed the `max_frame_bytes`.
pub(super) fn decode_frame_header(
    data: &[u8],
    max_frame_bytes: usize,
) -> K2Result<(FrameType, usize)> {
    if data.len() < FRAME_HEADER_LEN {
        return Err(K2Error::other(
            "iroh frame shorter than header".to_string(),
        ));
    }
    // Parse frame type from header byte.
    let frame_type = FrameType::try_from(data[0])?;
    // Extract data length from next 4 bytes.
    let data_len =
        u32::from_be_bytes([data[1], data[2], data[3], data[4]]) as usize;
    if data_len + FRAME_HEADER_LEN > max_frame_bytes {
        return Err(K2Error::other("iroh frame too large".to_string()));
    }
    Ok((frame_type, data_len))
}

/// Decodes a preflight frame from raw byte data.
///
/// The preflight consists of:
/// - 4 bytes for the URL length
/// - The URL
/// - The preflight bytes
///
/// The URL and the preflight bytes are returned as separate values.
///
/// # Errors
///
/// Returns an error if the data is shorter than 4 bytes for the URL length, or shorter
/// than 4 bytes for the URL length plus the bytes of the URL, and if the URL has an
/// invalid format.
pub(super) fn decode_frame_preflight(data: &[u8]) -> K2Result<(Url, Bytes)> {
    // If there are less than 4 bytes that indicate the URL length, return an error.
    if data.len() < 4 {
        return Err(K2Error::other("preflight data too short for URL length"));
    }
    // The first 4 bytes of the data are the URL length...
    let url_len =
        u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as usize;
    // If the data length is shorter than 4 bytes for the URL length plus the bytes
    // for the URL, return an error.
    if data.len() < 4 + url_len {
        return Err(K2Error::other("preflight data too short for actual URL"));
    }
    // ...followed by the URL
    let url = Url::from_str(
        std::str::from_utf8(&data[4..4 + url_len])
            .map_err(|err| K2Error::other_src("invalid peer url", err))?,
    )?;
    // The preflight takes up the rest of the data,
    // after the URL length and the URL.
    let preflight_bytes = Bytes::copy_from_slice(&data[4 + url_len..]);
    Ok((url, preflight_bytes))
}
