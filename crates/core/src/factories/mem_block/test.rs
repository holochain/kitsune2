use kitsune2_api::{Block, BlockTarget};

use super::MemBlock;

#[tokio::test]
async fn unblocked_target_reports_is_blocked_false() {
    let block = MemBlock::default();
    let target =
        BlockTarget::Agent(bytes::Bytes::from_static(b"test-agent").into());

    assert!(!block.is_blocked(&target).await.unwrap());
}

#[tokio::test]
async fn blocking_target_reports_is_blocked() {
    let block = MemBlock::default();
    let target =
        BlockTarget::Agent(bytes::Bytes::from_static(b"test-agent").into());

    block.block(target.clone()).await.unwrap();

    assert!(block.is_blocked(&target).await.unwrap());
}

#[tokio::test]
async fn can_block_same_target_multiple_times() {
    let block = MemBlock::default();
    let target =
        BlockTarget::Agent(bytes::Bytes::from_static(b"test-agent").into());

    block.block(target.clone()).await.unwrap();
    block.block(target.clone()).await.unwrap();

    assert!(block.is_blocked(&target).await.unwrap());
}

#[tokio::test]
async fn targets_are_not_repeated_in_store() {
    let block = MemBlock::default();
    let target =
        BlockTarget::Agent(bytes::Bytes::from_static(b"test-agent").into());

    block.block(target.clone()).await.unwrap();
    block.block(target.clone()).await.unwrap();

    assert_eq!(block.0.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn only_one_target_is_blocked_when_checking() {
    let block = MemBlock::default();
    let target_1 =
        BlockTarget::Agent(bytes::Bytes::from_static(b"test-agent-1").into());
    let target_2 =
        BlockTarget::Agent(bytes::Bytes::from_static(b"test-agent-2").into());

    block.block(target_1.clone()).await.unwrap();

    assert!(!block.are_all_blocked(&[target_1, target_2]).await.unwrap());
}

#[tokio::test]
async fn all_targets_are_blocked_when_checking() {
    let block = MemBlock::default();
    let target_1 =
        BlockTarget::Agent(bytes::Bytes::from_static(b"test-agent-1").into());
    let target_2 =
        BlockTarget::Agent(bytes::Bytes::from_static(b"test-agent-2").into());

    block.block(target_1.clone()).await.unwrap();
    block.block(target_2.clone()).await.unwrap();

    assert!(block.are_all_blocked(&[target_1, target_2]).await.unwrap());
}
