//! Unit tests for stream I/O abstractions

use crate::stream::mock::{MockRecvStream, MockSendStream};
use crate::stream::{RecvStream, SendStream};
use bytes::Bytes;

// SendStream tests

#[tokio::test]
async fn mock_send_stream_captures_data() {
    let mock_stream = MockSendStream::new();

    let test_data = b"hello world";
    let result = mock_stream.write_all(test_data).await;

    assert!(result.is_ok(), "write should succeed");

    let written = mock_stream.get_written_data();
    assert_eq!(written.len(), 1, "should have one write");
    assert_eq!(written[0], test_data, "data should match");
}

#[tokio::test]
async fn mock_send_stream_multiple_writes() {
    let mock_stream = MockSendStream::new();

    let data1 = b"first";
    let data2 = b"second";

    mock_stream.write_all(data1).await.unwrap();
    mock_stream.write_all(data2).await.unwrap();

    let written = mock_stream.get_written_data();
    assert_eq!(written.len(), 2);
    assert_eq!(written[0], data1);
    assert_eq!(written[1], data2);
}

#[tokio::test]
async fn mock_send_stream_can_fail() {
    let mock_stream = MockSendStream::with_failure();

    let test_data = b"test";
    let result = mock_stream.write_all(test_data).await;

    assert!(result.is_err(), "write should fail");
    let error_msg = result.unwrap_err().to_string();
    assert!(
        error_msg.contains("mock stream write failure"),
        "error message should contain expected text, got: {}",
        error_msg
    );

    let written = mock_stream.get_written_data();
    assert_eq!(written.len(), 0, "no data should be written on failure");
}

#[tokio::test]
async fn mock_send_stream_frame_encoding_integration() {
    // This demonstrates how we can now test frame encoding + stream writing
    // without requiring actual network I/O
    use crate::frame::{encode_frame, Frame};

    let mock_stream = MockSendStream::new();

    let test_data = Bytes::from_static(b"test payload");
    let frame = encode_frame(Frame::Data(test_data.clone()), 1024 * 1024)
        .expect("frame encoding should succeed");

    mock_stream
        .write_all(&frame)
        .await
        .expect("write should succeed");

    let written = mock_stream.get_written_data();
    assert_eq!(written.len(), 1);

    // Verify the frame was written correctly
    let written_frame = &written[0];
    assert_eq!(written_frame, &frame[..]);
}

// RecvStream tests

#[tokio::test]
async fn mock_recv_stream_reads_data() {
    let test_data = vec![1, 2, 3, 4, 5];
    let mock_stream = MockRecvStream::new(test_data.clone());

    let mut buf = vec![0u8; 5];
    let result = mock_stream.read_exact(&mut buf).await;

    assert!(result.is_ok(), "read should succeed");
    assert_eq!(buf, test_data, "read data should match");
    assert_eq!(mock_stream.get_read_position(), 5);
}

#[tokio::test]
async fn mock_recv_stream_multiple_reads() {
    let test_data = vec![1, 2, 3, 4, 5, 6, 7, 8];
    let mock_stream = MockRecvStream::new(test_data);

    let mut buf1 = vec![0u8; 3];
    mock_stream.read_exact(&mut buf1).await.unwrap();
    assert_eq!(buf1, vec![1, 2, 3]);
    assert_eq!(mock_stream.get_read_position(), 3);

    let mut buf2 = vec![0u8; 5];
    mock_stream.read_exact(&mut buf2).await.unwrap();
    assert_eq!(buf2, vec![4, 5, 6, 7, 8]);
    assert_eq!(mock_stream.get_read_position(), 8);
}

#[tokio::test]
async fn mock_recv_stream_read_beyond_available_data() {
    let test_data = vec![1, 2, 3];
    let mock_stream = MockRecvStream::new(test_data);

    let mut buf = vec![0u8; 5];
    let result = mock_stream.read_exact(&mut buf).await;

    assert!(result.is_err(), "read should fail");
    let error_msg = result.unwrap_err().to_string();
    assert!(
        error_msg.contains("not enough data to read"),
        "error should indicate insufficient data, got: {}",
        error_msg
    );
}

#[tokio::test]
async fn mock_recv_stream_can_fail() {
    let mock_stream = MockRecvStream::with_failure();

    let mut buf = vec![0u8; 5];
    let result = mock_stream.read_exact(&mut buf).await;

    assert!(result.is_err(), "read should fail");
    let error_msg = result.unwrap_err().to_string();
    assert!(
        error_msg.contains("mock stream read failure"),
        "error message should contain expected text, got: {}",
        error_msg
    );
}

#[tokio::test]
async fn mock_recv_stream_frame_decoding_integration() {
    // This demonstrates how we can now test stream reading + frame decoding
    // without requiring actual network I/O
    use crate::frame::{encode_frame, Frame};
    use crate::{decode_frame_header, FRAME_HEADER_LEN};

    let test_data = Bytes::from_static(b"test payload");
    let frame = encode_frame(Frame::Data(test_data.clone()), 1024 * 1024)
        .expect("frame encoding should succeed");

    let mock_stream = MockRecvStream::new(frame.to_vec());

    // Read and decode frame header
    let mut header = vec![0u8; FRAME_HEADER_LEN];
    mock_stream
        .read_exact(&mut header)
        .await
        .expect("header read should succeed");

    let (_frame_type, data_len) = decode_frame_header(&header, 1024 * 1024)
        .expect("header decode should succeed");

    // Read frame data
    let mut data = vec![0u8; data_len];
    mock_stream
        .read_exact(&mut data)
        .await
        .expect("data read should succeed");

    assert_eq!(data, test_data.as_ref(), "decoded data should match");
}
