use std::sync::Arc;

use kitsune2_api::{
    AgentId, AgentInfo, AgentInfoSigned, BoxFut, Builder, DhtArc, DynOpStore,
    DynPeerStore, DynVerifier, K2Result, Publish, Signer, SpaceHandler,
    SpaceId, Timestamp, TxBaseHandler, TxHandler, TxSpaceHandler, Url,
};
use kitsune2_test_utils::{
    agent::{TestLocalAgent, TestVerifier},
    enable_tracing, iter_check,
    space::TEST_SPACE_ID,
};

use crate::factories::{core_publish::CorePublish, MemoryOp};

use super::CorePublishConfig;

const INVALID_SIG: &[u8] = b"invalid-signature";

#[derive(Debug)]
struct IinvalidSigner;

impl Signer for IinvalidSigner {
    fn sign<'a, 'b: 'a, 'c: 'a>(
        &'a self,
        _agent_info: &'b AgentInfo,
        _encoded: &'c [u8],
    ) -> BoxFut<'a, K2Result<bytes::Bytes>> {
        Box::pin(async move { Ok(bytes::Bytes::from_static(INVALID_SIG)) })
    }
}

async fn setup_test(
    config: &CorePublishConfig,
) -> (CorePublish, DynOpStore, DynPeerStore, DynVerifier, Url) {
    let verifier = Arc::new(TestVerifier);

    let builder = Builder {
        verifier: verifier.clone(),
        ..crate::default_test_builder()
    }
    .with_default_config()
    .unwrap();

    let builder = Arc::new(builder);

    let op_store = builder
        .op_store
        .create(builder.clone(), TEST_SPACE_ID)
        .await
        .unwrap();

    #[derive(Debug)]
    struct NoopHandler;
    impl TxBaseHandler for NoopHandler {}
    impl TxHandler for NoopHandler {}
    impl TxSpaceHandler for NoopHandler {}
    impl SpaceHandler for NoopHandler {}

    let transport = builder
        .transport
        .create(builder.clone(), Arc::new(NoopHandler))
        .await
        .unwrap();

    let fetch = builder
        .fetch
        .create(
            builder.clone(),
            TEST_SPACE_ID,
            op_store.clone(),
            transport.clone(),
        )
        .await
        .unwrap();

    let peer_store = builder.peer_store.create(builder.clone()).await.unwrap();

    let url =
        transport.register_space_handler(TEST_SPACE_ID, Arc::new(NoopHandler));

    (
        CorePublish::new(
            config.clone(),
            TEST_SPACE_ID,
            builder,
            fetch,
            peer_store.clone(),
            transport,
        ),
        op_store,
        peer_store,
        verifier,
        url.unwrap(),
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn published_ops_can_be_retrieved() {
    enable_tracing();

    let (core_publish_1, op_store_1, _, _, _) =
        setup_test(&CorePublishConfig::default()).await;
    let (_core_publish2, op_store_2, _, _, url_2) =
        setup_test(&CorePublishConfig::default()).await;

    let incoming_op_1 = MemoryOp::new(Timestamp::now(), vec![1]);
    let incoming_op_id_1 = incoming_op_1.compute_op_id();
    let incoming_op_2 = MemoryOp::new(Timestamp::now(), vec![2]);
    let incoming_op_id_2 = incoming_op_2.compute_op_id();

    op_store_1
        .process_incoming_ops(vec![incoming_op_1.into(), incoming_op_2.into()])
        .await
        .unwrap();

    core_publish_1
        .publish_ops(
            vec![incoming_op_id_1.clone(), incoming_op_id_2.clone()],
            url_2,
        )
        .await
        .unwrap();

    let ops = iter_check!(1000, {
        let ops = op_store_2
            .retrieve_ops(vec![
                incoming_op_id_1.clone(),
                incoming_op_id_2.clone(),
            ])
            .await
            .unwrap();

        if ops.len() == 2 {
            return ops;
        }
    });

    assert!(ops.len() == 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn publish_to_invalid_url_does_not_impede_subsequent_publishes() {
    enable_tracing();

    let (core_publish_1, op_store_1, _, _, _) =
        setup_test(&CorePublishConfig::default()).await;
    let (_core_publish2, op_store_2, _, _, url_2) =
        setup_test(&CorePublishConfig::default()).await;

    let incoming_op_1 = MemoryOp::new(Timestamp::now(), vec![1]);
    let incoming_op_id_1 = incoming_op_1.compute_op_id();
    let incoming_op_2 = MemoryOp::new(Timestamp::now(), vec![2]);
    let incoming_op_id_2 = incoming_op_2.compute_op_id();

    op_store_1
        .process_incoming_ops(vec![incoming_op_1.into(), incoming_op_2.into()])
        .await
        .unwrap();

    let op_ids = vec![incoming_op_id_1.clone(), incoming_op_id_2.clone()];

    // Publish to a non-existing Url
    core_publish_1
        .publish_ops(
            op_ids.clone(),
            Url::from_str("ws://notanexistingurl:80").unwrap(),
        )
        .await
        .unwrap();

    // Publish to an existing Url to verify that the prior publishing
    // to a non-existing Url did not cause an error that would
    // break the publishing flow
    core_publish_1.publish_ops(op_ids, url_2).await.unwrap();

    let ops = iter_check!(1000, {
        let ops = op_store_2
            .retrieve_ops(vec![
                incoming_op_id_1.clone(),
                incoming_op_id_2.clone(),
            ])
            .await
            .unwrap();

        if ops.len() == 2 {
            return ops;
        }
    });

    assert!(ops.len() == 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn published_agent_can_be_retrieved() {
    enable_tracing();

    let (core_publish_1, _, _, _, _) =
        setup_test(&CorePublishConfig::default()).await;
    let (_core_publish2, _, peer_store_2, _, url_2) =
        setup_test(&CorePublishConfig::default()).await;

    let agent_id: AgentId = bytes::Bytes::from_static(b"test-agent").into();
    let space: SpaceId = bytes::Bytes::from_static(b"test-space").into();
    let now = Timestamp::now();
    let later = Timestamp::from_micros(now.as_micros() + 72_000_000_000);
    let url = Some(url_2.clone());
    let storage_arc = DhtArc::Arc(42, u32::MAX / 13);

    let agent_info_signed = AgentInfoSigned::sign(
        &TestLocalAgent::default(),
        AgentInfo {
            agent: agent_id.clone(),
            space: space.clone(),
            created_at: now,
            expires_at: later,
            is_tombstone: false,
            url: url.clone(),
            storage_arc,
        },
    )
    .await
    .unwrap();

    core_publish_1
        .publish_agent(agent_info_signed.clone(), url_2)
        .await
        .unwrap();

    let agent = iter_check!(1000, {
        let agent = peer_store_2.get(agent_id.clone()).await.unwrap();
        if let Some(agent_info_signed) = agent {
            return agent_info_signed;
        }
    });

    assert_eq!(agent, agent_info_signed)
}

#[tokio::test(flavor = "multi_thread")]
async fn invalid_agent_is_not_inserted_into_peer_store_and_subsequent_publishes_succeed(
) {
    enable_tracing();

    let (core_publish_1, _, _, _, _) =
        setup_test(&CorePublishConfig::default()).await;
    let (_core_publish2, _, peer_store_2, verifier_2, url_2) =
        setup_test(&CorePublishConfig::default()).await;

    let agent_id_invalid: AgentId =
        bytes::Bytes::from_static(b"test-agent").into();
    let agent_id_valid: AgentId =
        bytes::Bytes::from_static(b"test-agent-2").into();
    let space: SpaceId = bytes::Bytes::from_static(b"test-space").into();
    let now = Timestamp::now();
    let later = Timestamp::from_micros(now.as_micros() + 72_000_000_000);
    let url = Some(url_2.clone());
    let storage_arc = DhtArc::Arc(42, u32::MAX / 13);

    let agent_info_signed_invalid = AgentInfoSigned::sign(
        &IinvalidSigner,
        AgentInfo {
            agent: agent_id_invalid.clone(),
            space: space.clone(),
            created_at: now,
            expires_at: later,
            is_tombstone: false,
            url: url.clone(),
            storage_arc,
        },
    )
    .await
    .unwrap();

    // Verify that the signature is indeed invalid according to the verifier
    // used in CorePublish
    if let Ok(_) = AgentInfoSigned::decode(
        &verifier_2,
        agent_info_signed_invalid.encode().unwrap().as_bytes(),
    ) {
        panic!("Signature of agent info used for this test should be invalid.")
    };

    let agent_info_signed_valid = AgentInfoSigned::sign(
        &TestLocalAgent::default(),
        AgentInfo {
            agent: agent_id_valid.clone(),
            space: space.clone(),
            created_at: now,
            expires_at: later,
            is_tombstone: false,
            url: url.clone(),
            storage_arc,
        },
    )
    .await
    .unwrap();

    // Publish an agent with an invalid signature
    core_publish_1
        .publish_agent(agent_info_signed_invalid.clone(), url_2.clone())
        .await
        .unwrap();

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // Publish an agent with a valid signature
    core_publish_1
        .publish_agent(agent_info_signed_valid.clone(), url_2)
        .await
        .unwrap();

    // Verify that the agent with the valid signature has been inserted
    let agen_inserted = iter_check!(1000, {
        let agen_inserted =
            peer_store_2.get(agent_id_valid.clone()).await.unwrap();
        if let Some(agent_info_signed) = agen_inserted {
            return agent_info_signed;
        }
    });

    assert_eq!(agen_inserted, agent_info_signed_valid);

    // Verify that the invalid agent info has not been inserted
    assert_eq!(peer_store_2.get(agent_id_invalid).await.unwrap(), None);
}
