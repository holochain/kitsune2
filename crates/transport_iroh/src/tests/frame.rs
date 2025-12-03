use crate::{encode_frame, Frame};

use super::{decode_frame, FrameType, FRAME_HEADER_LEN, MAX_FRAME_BYTES};
use bytes::Bytes;
use kitsune2_api::Url;

#[test]
fn encode_frame_too_large() {
    let excessive_data_len = MAX_FRAME_BYTES - FRAME_HEADER_LEN + 1;
    let frame =
        Frame::Data(Bytes::copy_from_slice(&vec![0u8; excessive_data_len]));
    let err = encode_frame(frame).unwrap_err();
    assert!(err.to_string().contains("frame too large"));
}

#[test]
fn encode_frame_valid_preflight() {
    let url = Url::from_str("http://some.url:0/withendpointid").unwrap();
    let preflight_bytes = Bytes::from_static(b"the preflight");
    let frame = Frame::Preflight((url.clone(), preflight_bytes.clone()));
    let encoded_frame = encode_frame(frame).unwrap();

    let url_len = url.as_str().len();
    // 4 bytes for the URL length + URL bytes + preflight bytes
    let expected_payload_len = (4 + url_len + preflight_bytes.len()) as u32;

    assert_eq!(encoded_frame[0], FrameType::Preflight as u8);
    let actual_payload_len = u32::from_be_bytes([
        encoded_frame[1],
        encoded_frame[2],
        encoded_frame[3],
        encoded_frame[4],
    ]);
    assert_eq!(actual_payload_len, expected_payload_len);
    let actual_url_len = u32::from_be_bytes([
        encoded_frame[5],
        encoded_frame[6],
        encoded_frame[7],
        encoded_frame[8],
    ]);
    assert_eq!(actual_url_len, url_len as u32);
    let actual_url =
        Url::from_str(str::from_utf8(&encoded_frame[9..9 + url_len]).unwrap())
            .unwrap();
    assert_eq!(actual_url, url);
    let actual_preflight =
        Bytes::copy_from_slice(&encoded_frame[9 + url_len..]);
    assert_eq!(actual_preflight, preflight_bytes);
}

#[test]
fn encode_frame_valid_data() {
    let data = Bytes::from_static(b"some message");
    let frame = Frame::Data(data.clone());
    let encoded_frame = encode_frame(frame).unwrap();

    assert_eq!(encoded_frame[0], FrameType::Data as u8);
    let actual_payload_len = u32::from_be_bytes([
        encoded_frame[1],
        encoded_frame[2],
        encoded_frame[3],
        encoded_frame[4],
    ]);
    assert_eq!(actual_payload_len, data.len() as u32);
    let actual_data = Bytes::copy_from_slice(&encoded_frame[5..]);
    assert_eq!(actual_data, data);
}

#[test]
fn decode_frame_valid_preflight() {
    let ty = FrameType::Preflight;
    let url = Url::from_str("http://some.url:0/withendpointid").unwrap();
    let url_bytes = Bytes::from(url.clone());
    let preflight_bytes = Bytes::from_static(b"the preflight");
    let mut payload = vec![];
    payload.extend_from_slice(&(url_bytes.len() as u32).to_be_bytes());
    payload.extend_from_slice(&url_bytes);
    payload.extend_from_slice(&preflight_bytes);

    let mut encoded_frame = vec![ty as u8];
    encoded_frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    encoded_frame.extend_from_slice(&payload);

    let frame = decode_frame(encoded_frame).unwrap();
    match frame {
        Frame::Preflight((actual_url, actual_preflight)) => {
            assert_eq!(actual_url, url);
            assert_eq!(actual_preflight, preflight_bytes);
        }
        other => panic!("unexpected frame type {other:?}"),
    }
}

#[test]
fn decode_frame_valid_data() {
    let ty = FrameType::Data;
    let message = Bytes::from_static(b"important notification");
    let mut payload = vec![];
    payload.extend_from_slice(&message);

    let mut encoded_frame = vec![ty as u8];
    encoded_frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    encoded_frame.extend_from_slice(&payload);

    let frame = decode_frame(encoded_frame).unwrap();
    match frame {
        Frame::Data(actual_message) => {
            assert_eq!(actual_message, message);
        }
        other => panic!("unexpected frame type {other:?}"),
    }
}

#[test]
fn decode_frame_invalid_preflight_url_too_short() {
    let ty = FrameType::Preflight;
    let mut encoded_frame = vec![ty as u8];
    // Too few payload length bytes
    encoded_frame.extend_from_slice(&(2u32).to_be_bytes());
    encoded_frame.extend_from_slice(&[0u8; 2]);

    let err = decode_frame(encoded_frame).unwrap_err();
    assert!(
        err.to_string()
            .contains("preflight payload too short for URL length"),
        "unexpected error, got {err}"
    );
}

#[test]
fn decode_frame_invalid_preflight_payload_too_short_for_url() {
    let ty = FrameType::Preflight;
    let url = Url::from_str("http://some.url:0/withendpointid").unwrap();
    let url_bytes = Bytes::from(url);
    let mut payload = vec![];
    // Too many URL length bytes
    payload.extend_from_slice(&(200u32).to_be_bytes());
    payload.extend_from_slice(&url_bytes);

    let mut encoded_frame = vec![ty as u8];
    encoded_frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    encoded_frame.extend_from_slice(&payload);

    let err = decode_frame(encoded_frame).unwrap_err();
    assert!(
        err.to_string()
            .contains("preflight payload too short for actual URL"),
        "unexpected error, got {err}"
    );
}

#[test]
fn decode_frame_too_short() {
    let encoded_frame = vec![0u8; FRAME_HEADER_LEN - 1];
    let err = decode_frame(encoded_frame).unwrap_err();
    assert!(err.to_string().contains("iroh frame shorter than header"));
}

#[test]
fn decode_frame_invalid_frame_type() {
    let mut encoded_frame = vec![2u8]; // Invalid type
    encoded_frame.extend_from_slice(&(0u32).to_be_bytes()); // Empty payload
    let err = decode_frame(encoded_frame).unwrap_err();
    assert!(err.to_string().contains("unknown iroh frame type"));
}

#[test]
fn decode_frame_payload_length_mismatch() {
    let ty = FrameType::Data;
    let payload_len = 10;
    let actual_len = 5;
    let mut encoded_frame = vec![ty as u8];
    encoded_frame.extend_from_slice(&(payload_len as u32).to_be_bytes()); // Claim 10 bytes
    encoded_frame.extend_from_slice(&vec![0u8; actual_len]); // Only 5 bytes

    let err = decode_frame(encoded_frame).unwrap_err();
    assert!(err
        .to_string()
        .contains("iroh frame payload length mismatch"));
}

#[test]
fn decode_frame_zero_data() {
    let ty = FrameType::Data;
    let payload = Bytes::new();
    let mut encoded_frame = vec![ty as u8];
    encoded_frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    encoded_frame.extend_from_slice(&payload);

    let frame = decode_frame(encoded_frame).unwrap();
    assert!(matches!(frame, Frame::Data(data) if data == payload));
}
