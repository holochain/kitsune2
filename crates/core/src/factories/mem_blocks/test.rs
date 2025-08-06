use kitsune2_api::{AgentId, BlockTarget, Blocks, Id};

use super::MemBlocks;

const BLOCK_TARGET_AGENT_1: BlockTarget =
    BlockTarget::Agent(AgentId(Id(bytes::Bytes::from_static(b"test-agent-1"))));
const BLOCK_TARGET_AGENT_2: BlockTarget =
    BlockTarget::Agent(AgentId(Id(bytes::Bytes::from_static(b"test-agent-2"))));

#[tokio::test]
async fn unblocked_target_reports_is_blocked_false() {
    let blocks = MemBlocks::default();

    assert!(!blocks.is_blocked(BLOCK_TARGET_AGENT_1).await.unwrap());
}

#[tokio::test]
async fn blocking_target_reports_is_blocked() {
    let blocks = MemBlocks::default();

    blocks.block(BLOCK_TARGET_AGENT_1).await.unwrap();

    assert!(blocks.is_blocked(BLOCK_TARGET_AGENT_1).await.unwrap());
}

#[tokio::test]
async fn can_block_same_target_multiple_times() {
    let blocks = MemBlocks::default();

    blocks.block(BLOCK_TARGET_AGENT_1).await.unwrap();
    blocks.block(BLOCK_TARGET_AGENT_1).await.unwrap();

    assert!(blocks.is_blocked(BLOCK_TARGET_AGENT_1).await.unwrap());
}

#[tokio::test]
async fn targets_are_not_repeated_in_store() {
    let blocks = MemBlocks::default();

    blocks.block(BLOCK_TARGET_AGENT_1).await.unwrap();
    blocks.block(BLOCK_TARGET_AGENT_1).await.unwrap();

    assert_eq!(blocks.0.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn only_one_target_is_blocked_when_checking() {
    let blocks = MemBlocks::default();

    blocks.block(BLOCK_TARGET_AGENT_1).await.unwrap();

    assert!(!blocks
        .are_all_blocked(vec![BLOCK_TARGET_AGENT_1, BLOCK_TARGET_AGENT_2])
        .await
        .unwrap());
}

#[tokio::test]
async fn all_targets_are_blocked_when_checking() {
    let blocks = MemBlocks::default();

    blocks.block(BLOCK_TARGET_AGENT_1).await.unwrap();
    blocks.block(BLOCK_TARGET_AGENT_2).await.unwrap();

    assert!(blocks
        .are_all_blocked(vec![BLOCK_TARGET_AGENT_1, BLOCK_TARGET_AGENT_2])
        .await
        .unwrap());
}
