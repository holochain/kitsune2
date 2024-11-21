use bytes::Bytes;
use gossip::types::GossipMessage;
use gossip::{initiate_gossip_round, respond_to_gossip_message};
use kitsune2_api::id::Id;
use kitsune2_api::{AgentId, HostApi, MetaOp, OpId, Timestamp};
use kitsune2_memory::{
    Kitsune2MemoryError, Kitsune2MemoryOpStore, Kitsune2MemoryPeerMetaStore,
};
use std::collections::HashSet;
use std::sync::Arc;

#[tokio::test(flavor = "multi_thread")]
async fn gossip_round_with_no_ops() {
    let (host_api_1, agent_id_1) = new_test_peer("agent 1");
    let (host_api_2, agent_id_2) = new_test_peer("agent 2");

    let initiate_message =
        initiate_gossip_round(host_api_1.clone(), agent_id_2.clone())
            .await
            .unwrap();
    match &initiate_message {
        GossipMessage::PeerInitiate(hello) => {
            assert_eq!(hello.chunk_states.len(), 0);
            assert_eq!(hello.new_ops_since, Timestamp::from_micros(0));
            assert_eq!(hello.new_ops_max_bytes, 1024);
        }
        other => panic!("Expected PeerInitiate, got {:?}", other),
    }

    let accept_message = respond_to_gossip_message(
        host_api_2.clone(),
        agent_id_1.clone(),
        initiate_message,
    )
    .await
    .unwrap()
    .expect("Should have responded with an accept message");
    match &accept_message {
        GossipMessage::PeerAccept {
            peer_hello,
            new_op_hashes,
            ..
        } => {
            assert_eq!(peer_hello.chunk_states.len(), 0);
            assert_eq!(peer_hello.new_ops_since, Timestamp::from_micros(0));
            assert_eq!(peer_hello.new_ops_max_bytes, 1024);
            assert!(new_op_hashes.is_empty());
        }
        other => panic!("Expected PeerAccept, got {:?}", other),
    }

    let complete_message = respond_to_gossip_message(
        host_api_1.clone(),
        agent_id_2.clone(),
        accept_message,
    )
    .await
    .unwrap()
    .expect("Should have responded with a complete message");
    match &complete_message {
        GossipMessage::PeerCompleteAfterAccept { new_op_hashes, .. } => {
            assert!(new_op_hashes.is_empty());
        }
        other => panic!("Expected PeerCompleteAfterAccept, got {:?}", other),
    }

    let no_more_messages = respond_to_gossip_message(
        host_api_2,
        agent_id_1.clone(),
        complete_message,
    )
    .await
    .unwrap();
    assert!(no_more_messages.is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn gossip_round_with_one_op_each_ops() {
    let (host_api_1, agent_id_1) = new_test_peer("agent 1");
    let (host_api_2, agent_id_2) = new_test_peer("agent 2");

    let test_op_1 = new_test_op("op 1");
    host_api_1
        .op_store
        .ingest_op_list(vec![test_op_1.clone()])
        .await
        .unwrap();

    let test_op_2 = new_test_op("op 2");
    host_api_2
        .op_store
        .ingest_op_list(vec![test_op_2.clone()])
        .await
        .unwrap();

    let initiate_message =
        initiate_gossip_round(host_api_1.clone(), agent_id_2.clone())
            .await
            .unwrap();
    match &initiate_message {
        GossipMessage::PeerInitiate(hello) => {
            assert_eq!(hello.chunk_states.len(), 0);
            assert_eq!(hello.new_ops_since, Timestamp::from_micros(0));
            assert_eq!(hello.new_ops_max_bytes, 1024);
        }
        other => panic!("Expected PeerInitiate, got {:?}", other),
    }

    let accept_message = respond_to_gossip_message(
        host_api_2.clone(),
        agent_id_1.clone(),
        initiate_message,
    )
    .await
    .unwrap()
    .expect("Should have responded with an accept message");
    match &accept_message {
        GossipMessage::PeerAccept {
            peer_hello,
            new_op_hashes,
            bookmark_timestamp,
        } => {
            assert_eq!(peer_hello.chunk_states.len(), 0);
            assert_eq!(peer_hello.new_ops_since, Timestamp::from_micros(0));
            assert_eq!(peer_hello.new_ops_max_bytes, 1024);
            assert_eq!(new_op_hashes.len(), 1);
            assert_eq!(new_op_hashes[0], OpId(Id(Bytes::from("op 2"))));
            assert_eq!(*bookmark_timestamp, test_op_2.timestamp);
        }
        other => panic!("Expected PeerAccept, got {:?}", other),
    }

    let complete_message = respond_to_gossip_message(
        host_api_1.clone(),
        agent_id_2.clone(),
        accept_message,
    )
    .await
    .unwrap()
    .expect("Should have responded with a complete message");
    match &complete_message {
        GossipMessage::PeerCompleteAfterAccept {
            new_op_hashes,
            bookmark_timestamp,
        } => {
            assert_eq!(new_op_hashes.len(), 1);
            assert_eq!(new_op_hashes[0], OpId(Id(Bytes::from("op 1"))));
            assert_eq!(*bookmark_timestamp, test_op_1.timestamp);
        }
        other => panic!("Expected PeerCompleteAfterAccept, got {:?}", other),
    }

    let no_more_messages = respond_to_gossip_message(
        host_api_2,
        agent_id_1.clone(),
        complete_message,
    )
    .await
    .unwrap();
    assert!(no_more_messages.is_none());
}

fn new_test_peer(
    agent_name: &'static str,
) -> (HostApi<Kitsune2MemoryError>, AgentId) {
    let op_store = Kitsune2MemoryOpStore::default();
    let peer_meta_store = Kitsune2MemoryPeerMetaStore::default();

    let host_api = HostApi {
        op_store: Arc::new(op_store),
        peer_meta_store: Arc::new(peer_meta_store),
    };

    (host_api, AgentId(Id(Bytes::from(agent_name))))
}

fn new_test_op(op_id: &'static str) -> MetaOp {
    MetaOp {
        op_id: OpId(Id(Bytes::from(op_id))),
        timestamp: Timestamp::now(),
        op_data: "op data".as_bytes().to_vec(),
        op_flags: HashSet::with_capacity(0),
    }
}
