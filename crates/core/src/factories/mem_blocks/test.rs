use kitsune2_api::{BlockTarget, Blocks};

use super::MemBlocks;

#[tokio::test]
async fn unblocked_target_reports_is_blocked_false() {
    let blocks = MemBlocks::default();
    let target =
        BlockTarget::Agent(bytes::Bytes::from_static(b"test-agent").into());

    assert!(!blocks.is_blocked(target).await.unwrap());
}

#[tokio::test]
async fn blocking_target_reports_is_blocked() {
    let blocks = MemBlocks::default();
    let target =
        BlockTarget::Agent(bytes::Bytes::from_static(b"test-agent").into());

    blocks.block(target.clone()).await.unwrap();

    assert!(blocks.is_blocked(target).await.unwrap());
}

#[tokio::test]
async fn can_block_same_target_multiple_times() {
    let blocks = MemBlocks::default();
    let target =
        BlockTarget::Agent(bytes::Bytes::from_static(b"test-agent").into());

    blocks.block(target.clone()).await.unwrap();
    blocks.block(target.clone()).await.unwrap();

    assert!(blocks.is_blocked(target).await.unwrap());
}

#[tokio::test]
async fn targets_are_not_repeated_in_store() {
    let blocks = MemBlocks::default();
    let target =
        BlockTarget::Agent(bytes::Bytes::from_static(b"test-agent").into());

    blocks.block(target.clone()).await.unwrap();
    blocks.block(target.clone()).await.unwrap();

    assert_eq!(blocks.0.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn only_one_target_is_blocked_when_checking() {
    let blocks = MemBlocks::default();
    let target_1 =
        BlockTarget::Agent(bytes::Bytes::from_static(b"test-agent-1").into());
    let target_2 =
        BlockTarget::Agent(bytes::Bytes::from_static(b"test-agent-2").into());

    blocks.block(target_1.clone()).await.unwrap();

    assert!(!blocks
        .are_all_blocked(vec![target_1, target_2])
        .await
        .unwrap());
}

#[tokio::test]
async fn all_targets_are_blocked_when_checking() {
    let blocks = MemBlocks::default();
    let target_1 =
        BlockTarget::Agent(bytes::Bytes::from_static(b"test-agent-1").into());
    let target_2 =
        BlockTarget::Agent(bytes::Bytes::from_static(b"test-agent-2").into());

    blocks.block(target_1.clone()).await.unwrap();
    blocks.block(target_2.clone()).await.unwrap();

    assert!(blocks
        .are_all_blocked(vec![target_1, target_2])
        .await
        .unwrap());
}
