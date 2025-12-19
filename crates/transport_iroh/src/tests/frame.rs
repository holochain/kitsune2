use super::{
    decode_frame_preflight, FrameType, FRAME_HEADER_LEN, MAX_FRAME_BYTES,
};
use crate::frame::decode_frame_header;
use crate::{encode_frame, Frame};
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
fn decode_valid_frame_header_preflight() {
    let mut header = vec![];
    let frame_type = FrameType::Preflight;
    let data_len = 10usize;
    header.push(frame_type as u8);
    header.extend_from_slice(&(data_len as u32).to_be_bytes());

    let (actual_frame_type, actual_data_len) =
        decode_frame_header(&header).unwrap();

    assert_eq!(
        actual_frame_type, frame_type,
        "expected preflight but got {:?}",
        actual_frame_type
    );
    assert_eq!(
        actual_data_len, data_len,
        "expected data length of {} but got {}",
        data_len, actual_data_len
    );
}

#[test]
fn decode_valid_frame_header_data() {
    let mut header = vec![];
    let frame_type = FrameType::Data;
    let data_len = 100usize;
    header.push(frame_type as u8);
    header.extend_from_slice(&(data_len as u32).to_be_bytes());

    let (actual_frame_type, actual_data_len) =
        decode_frame_header(&header).unwrap();

    assert_eq!(
        actual_frame_type, frame_type,
        "expected data but got {:?}",
        actual_frame_type
    );
    assert_eq!(
        actual_data_len, data_len,
        "expected data length of {} but got {}",
        data_len, actual_data_len
    );
}

#[test]
fn decode_invalid_frame_header_unknown_type() {
    let mut header = vec![];
    let unknown_frame_type = 100u8;
    let data_len = 10usize;
    header.push(unknown_frame_type);
    header.extend_from_slice(&(data_len as u32).to_be_bytes());

    let err = decode_frame_header(&header).unwrap_err();

    assert!(
        err.to_string().contains("unknown iroh frame type"),
        "unexpected error, got {err}"
    );
}

#[test]
fn decode_invalid_frame_header_data_exceeds_max_frame_bytes() {
    let mut header = vec![];
    let frame_type = FrameType::Data;
    let data_len = MAX_FRAME_BYTES + 1;
    header.push(frame_type as u8);
    header.extend_from_slice(&(data_len as u32).to_be_bytes());

    let err = decode_frame_header(&header).unwrap_err();

    assert!(
        err.to_string().contains("iroh frame too large"),
        "unexpected error, got {err}"
    );
}

#[test]
fn decode_frame_valid_preflight() {
    let url = Url::from_str("http://some.url:0/withendpointid").unwrap();
    let url_bytes = Bytes::from(url.clone());
    let preflight_bytes = Bytes::from_static(b"the preflight");
    let mut endcoded_frame = vec![];
    endcoded_frame.extend_from_slice(&(url_bytes.len() as u32).to_be_bytes());
    endcoded_frame.extend_from_slice(&url_bytes);
    endcoded_frame.extend_from_slice(&preflight_bytes);

    let (actual_url, actual_preflight_bytes) =
        decode_frame_preflight(&endcoded_frame).unwrap();

    assert_eq!(actual_url, url);
    assert_eq!(actual_preflight_bytes, preflight_bytes);
}

#[test]
fn decode_frame_invalid_preflight_url_too_short() {
    let mut encoded_frame = vec![];
    // Too few URL length bytes
    encoded_frame.extend_from_slice(&2u16.to_be_bytes());

    let err = decode_frame_preflight(&encoded_frame).unwrap_err();

    assert!(
        err.to_string()
            .contains("preflight data too short for URL length"),
        "unexpected error, got {err}"
    );
}

#[test]
fn decode_frame_invalid_preflight_payload_too_short_for_url() {
    let url = Url::from_str("http://some.url:0/withendpointid").unwrap();
    let url_bytes = Bytes::from(url);
    let mut encoded_frame = vec![];
    // Too many URL length bytes
    encoded_frame.extend_from_slice(&200u32.to_be_bytes());
    encoded_frame.extend_from_slice(&url_bytes);

    let err = decode_frame_preflight(&encoded_frame).unwrap_err();

    assert!(
        err.to_string()
            .contains("preflight data too short for actual URL"),
        "unexpected error, got {err}"
    );
}
