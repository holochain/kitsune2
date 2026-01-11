//! Unit tests for stream I/O abstractions

use crate::stream_io::mock::MockSendStream;
use crate::stream_io::SendStreamTrait;
use bytes::Bytes;

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
