use super::{decode_frame, FrameType, FRAME_HEADER_LEN, MAX_FRAME_BYTES};
use bytes::Bytes;

#[test]
fn decode_frame_valid_peer_url() {
    let ty = FrameType::PeerUrl;
    let payload = Bytes::from("http://some.url:0/withendpointid");
    let mut data = vec![ty as u8];
    data.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    data.extend_from_slice(&payload);

    let result = decode_frame(data).unwrap();
    assert_eq!(result.0, ty);
    assert_eq!(result.1, payload);
}

#[test]
fn decode_frame_valid_payload() {
    let ty = FrameType::Payload;
    let payload = Bytes::from("test payload");
    let mut data = vec![ty as u8];
    data.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    data.extend_from_slice(&payload);

    let result = decode_frame(data).unwrap();
    assert_eq!(result.0, ty);
    assert_eq!(result.1, payload);
}

#[test]
fn decode_frame_too_short() {
    let data = vec![0u8; FRAME_HEADER_LEN - 1];
    let err = decode_frame(data).unwrap_err();
    assert!(err.to_string().contains("iroh frame shorter than header"));
}

#[test]
fn decode_frame_invalid_frame_type() {
    let mut data = vec![2u8]; // Invalid type
    data.extend_from_slice(&(0u32).to_be_bytes()); // Empty payload
    let err = decode_frame(data).unwrap_err();
    assert!(err.to_string().contains("unknown iroh frame type"));
}

#[test]
fn decode_frame_payload_length_mismatch() {
    let ty = FrameType::Payload;
    let payload_len = 10;
    let actual_len = 5;
    let mut data = vec![ty as u8];
    data.extend_from_slice(&(payload_len as u32).to_be_bytes()); // Claim 10 bytes
    data.extend_from_slice(&vec![0u8; actual_len]); // Only 5 bytes

    let err = decode_frame(data).unwrap_err();
    assert!(err
        .to_string()
        .contains("iroh frame payload length mismatch"));
}

#[test]
fn decode_frame_zero_payload() {
    let ty = FrameType::PeerUrl;
    let payload = Bytes::new();
    let mut data = vec![ty as u8];
    data.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    data.extend_from_slice(&payload);

    let result = decode_frame(data).unwrap();
    assert_eq!(result.0, ty);
    assert_eq!(result.1, payload);
}

#[test]
fn decode_frame_max_payload() {
    let ty = FrameType::Payload;
    let payload_len = MAX_FRAME_BYTES - FRAME_HEADER_LEN;
    let payload = Bytes::from(vec![0u8; payload_len]);
    let mut data = vec![ty as u8];
    data.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    data.extend_from_slice(&payload);

    let result = decode_frame(data).unwrap();
    assert_eq!(result.0, ty);
    assert_eq!(result.1, payload);
}
