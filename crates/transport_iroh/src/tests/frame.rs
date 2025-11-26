use super::{
    decode_frame, encode_frame_header, FrameType, FRAME_HEADER_LEN,
    MAX_FRAME_BYTES,
};
use bytes::Bytes;

#[test]
fn encode_frame_header_valid_peer_url() {
    let ty = FrameType::PeerUrl;
    let data_len = 100;
    let header = encode_frame_header(ty, data_len).unwrap();
    assert_eq!(header[0], ty as u8);
    assert_eq!(
        u32::from_be_bytes([header[1], header[2], header[3], header[4]]),
        data_len as u32
    );
}

#[test]
fn encode_frame_header_valid_payload() {
    let ty = FrameType::Payload;
    let data_len = 0;
    let header = encode_frame_header(ty, data_len).unwrap();
    assert_eq!(header[0], ty as u8);
    assert_eq!(
        u32::from_be_bytes([header[1], header[2], header[3], header[4]]),
        data_len as u32
    );
}

#[test]
fn encode_frame_header_too_large() {
    let data_len = MAX_FRAME_BYTES - FRAME_HEADER_LEN + 1;
    let err = encode_frame_header(FrameType::Payload, data_len).unwrap_err();
    assert!(err.to_string().contains("frame too large"));
}

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
