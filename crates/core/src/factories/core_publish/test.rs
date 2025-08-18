use super::CorePublishConfig;
use crate::{
    default_test_builder,
    factories::{core_publish::CorePublish, MemoryOp},
};
use kitsune2_api::{
    AgentId, AgentInfo, AgentInfoSigned, BoxFut, Builder, DhtArc, DynOpStore,
    DynPeerMetaStore, DynPeerStore, DynTransport, DynVerifier, K2Result,
    MockTransport, Publish, Signer, SpaceHandler, SpaceId, Timestamp,
    TxBaseHandler, TxHandler, TxSpaceHandler, Url,
};
use kitsune2_test_utils::{
    agent::{TestLocalAgent, TestVerifier},
    enable_tracing, iter_check,
    space::TEST_SPACE_ID,
};
use std::{sync::Arc, time::Duration};

#[tokio::test(flavor = "multi_thread")]
async fn published_ops_can_be_retrieved() {
    enable_tracing();

    let Test {
        publish: core_publish_1,
        op_store: op_store_1,
        transport: _transport_1,
        ..
    } = Test::setup().await;
    let Test {
        publish: _core_publish2,
        op_store: op_store_2,
        url: url_2,
        transport: _transport_2,
        ..
    } = Test::setup().await;

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

    let Test {
        publish: core_publish_1,
        op_store: op_store_1,
        transport: _transport_1,
        ..
    } = Test::setup().await;
    let Test {
        publish: _core_publish2,
        op_store: op_store_2,
        url: url_2,
        transport: _transport_2,
        ..
    } = Test::setup().await;

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

    let Test {
        publish: core_publish_1,
        transport: _transport_1,
        ..
    } = Test::setup().await;
    let Test {
        publish: _core_publish2,
        peer_store: peer_store_2,
        url: url_2,
        transport: _transport_2,
        ..
    } = Test::setup().await;

    let agent_id: AgentId = bytes::Bytes::from_static(b"test-agent").into();
    let space_id: SpaceId = bytes::Bytes::from_static(b"test-space").into();
    let now = Timestamp::now();
    let later = Timestamp::from_micros(now.as_micros() + 72_000_000_000);
    let url = Some(url_2.clone());
    let storage_arc = DhtArc::Arc(42, u32::MAX / 13);

    let agent_info_signed = AgentInfoSigned::sign(
        &TestLocalAgent::default(),
        AgentInfo {
            agent: agent_id.clone(),
            space: space_id.clone(),
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

    let Test {
        publish: core_publish_1,
        transport: _transport_2,
        ..
    } = Test::setup().await;
    let Test {
        publish: _core_publish2,
        peer_store: peer_store_2,
        verifier: verifier_2,
        url: url_2,
        transport: _transport_2,
        ..
    } = Test::setup().await;

    let agent_id_invalid: AgentId =
        bytes::Bytes::from_static(b"test-agent").into();
    let agent_id_valid: AgentId =
        bytes::Bytes::from_static(b"test-agent-2").into();
    let space_id: SpaceId = bytes::Bytes::from_static(b"test-space").into();
    let now = Timestamp::now();
    let later = Timestamp::from_micros(now.as_micros() + 72_000_000_000);
    let url = Some(url_2.clone());
    let storage_arc = DhtArc::Arc(42, u32::MAX / 13);

    let agent_info_signed_invalid = AgentInfoSigned::sign(
        &InvalidSigner,
        AgentInfo {
            agent: agent_id_invalid.clone(),
            space: space_id.clone(),
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
    // used on the receiving side of the agent publish
    if AgentInfoSigned::decode(
        &verifier_2,
        agent_info_signed_invalid.encode().unwrap().as_bytes(),
    )
    .is_ok()
    {
        panic!("Signature of agent info used for this test should be invalid.")
    };

    let agent_info_signed_valid = AgentInfoSigned::sign(
        &TestLocalAgent::default(),
        AgentInfo {
            agent: agent_id_valid.clone(),
            space: space_id.clone(),
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
    let agent_inserted = iter_check!(1000, {
        let agent_inserted =
            peer_store_2.get(agent_id_valid.clone()).await.unwrap();
        if let Some(agent_info_signed) = agent_inserted {
            return agent_info_signed;
        }
    });

    assert_eq!(agent_inserted, agent_info_signed_valid);

    // Verify that the invalid agent info has not been inserted
    assert_eq!(peer_store_2.get(agent_id_invalid).await.unwrap(), None);
}

#[tokio::test(flavor = "multi_thread")]
async fn no_publish_to_unresponsive_url() {
    let unresponsive_url = Url::from_str("ws://unresponsi.ve:80").unwrap();
    let responsive_url = Url::from_str("ws://responsi.ve:80").unwrap();

    // Create a mock transport to check where publish sends to.
    // Set up a channel to know when the intended publish has been sent.
    let (expected_publish_sent_tx, mut expected_publish_sent_rx) =
        tokio::sync::mpsc::channel(1);
    let mut transport = MockTransport::new();
    let responsive_url_clone = responsive_url.clone();
    let unresponsive_url_clone = unresponsive_url.clone();
    transport
        .expect_send_module()
        .returning(move |url, _, _, _| {
            // If publish goes to unresponsive URL, test fails.
            if url == unresponsive_url_clone {
                panic!("published to unresponsive URL");
            }
            if url == responsive_url_clone {
                expected_publish_sent_tx.try_send(()).unwrap();
            }
            Box::pin(async move { Ok(()) })
        });
    transport
        .expect_register_module_handler()
        .returning(|_, _, _| {});
    let transport = Arc::new(transport);

    // Set up one publish module.
    let builder =
        Arc::new(default_test_builder().with_default_config().unwrap());
    let (publish, _, _, peer_meta_store) =
        create_publish(builder, transport).await;

    let op = MemoryOp::new(Timestamp::now(), vec![1]);
    let op_id = op.compute_op_id();
    // Set unresponsive URL in peer meta store.
    peer_meta_store
        .set_unresponsive(
            unresponsive_url.clone(),
            Timestamp::now(),
            Timestamp::now(),
        )
        .await
        .unwrap();

    // Publish to the unresponsive URL, which should be omitted.
    publish
        .publish_ops(vec![op_id.clone()], unresponsive_url)
        .await
        .unwrap();
    // Publish to another URL which is not unresponsive, so that this send
    // can be awaited to have happened, which implies that the previous
    // publish to the unresponsive URL has been omitted.
    publish
        .publish_ops(vec![op_id], responsive_url)
        .await
        .unwrap();

    // Timeout after waiting 100 ms for publish to send to the responsive URL.
    tokio::select! {
        _ = expected_publish_sent_rx.recv() => {},
        _ = tokio::time::sleep(Duration::from_millis(100)) => {
            panic!("publish not sent to responsive URL")
        },
    };
}

const INVALID_SIG: &[u8] = b"invalid-signature";

#[derive(Debug)]
struct InvalidSigner;

impl Signer for InvalidSigner {
    fn sign<'a, 'b: 'a, 'c: 'a>(
        &'a self,
        _agent_info: &'b AgentInfo,
        _encoded: &'c [u8],
    ) -> BoxFut<'a, K2Result<bytes::Bytes>> {
        Box::pin(async move { Ok(bytes::Bytes::from_static(INVALID_SIG)) })
    }
}

async fn create_publish(
    builder: Arc<Builder>,
    transport: DynTransport,
) -> (CorePublish, DynOpStore, DynPeerStore, DynPeerMetaStore) {
    let report = builder
        .report
        .create(builder.clone(), transport.clone())
        .await
        .unwrap();
    let op_store = builder
        .op_store
        .create(builder.clone(), TEST_SPACE_ID)
        .await
        .unwrap();
    let peer_meta_store = builder
        .peer_meta_store
        .create(builder.clone(), TEST_SPACE_ID)
        .await
        .unwrap();
    let fetch = builder
        .fetch
        .create(
            builder.clone(),
            TEST_SPACE_ID,
            report,
            op_store.clone(),
            peer_meta_store.clone(),
            transport.clone(),
        )
        .await
        .unwrap();
    let blocks = builder
        .blocks
        .create(builder.clone(), TEST_SPACE_ID)
        .await
        .unwrap();
    let peer_store = builder
        .peer_store
        .create(builder.clone(), TEST_SPACE_ID, blocks)
        .await
        .unwrap();
    let publish = CorePublish::new(
        CorePublishConfig::default(),
        TEST_SPACE_ID,
        builder,
        fetch,
        peer_store.clone(),
        peer_meta_store.clone(),
        transport.clone(),
    );
    (publish, op_store, peer_store, peer_meta_store)
}

struct Test {
    publish: CorePublish,
    op_store: DynOpStore,
    peer_store: DynPeerStore,
    transport: DynTransport,
    verifier: DynVerifier,
    url: Url,
}

impl Test {
    async fn setup() -> Self {
        let verifier = Arc::new(TestVerifier);

        let builder = Builder {
            verifier: verifier.clone(),
            ..crate::default_test_builder()
        }
        .with_default_config()
        .unwrap();
        let builder = Arc::new(builder);

        #[derive(Debug)]
        struct NoopHandler;
        impl TxBaseHandler for NoopHandler {}
        impl TxHandler for NoopHandler {}
        impl TxSpaceHandler for NoopHandler {
            fn are_all_agents_at_url_blocked(
                &self,
                _peer_url: &Url,
            ) -> K2Result<bool> {
                Ok(false)
            }
        }
        impl SpaceHandler for NoopHandler {}

        let transport = builder
            .transport
            .create(builder.clone(), Arc::new(NoopHandler))
            .await
            .unwrap();

        let url = transport
            .register_space_handler(TEST_SPACE_ID, Arc::new(NoopHandler))
            .unwrap();

        let (publish, op_store, peer_store, _) =
            create_publish(builder, transport.clone()).await;

        Self {
            publish,
            op_store,
            peer_store,
            verifier,
            url,
            transport,
        }
    }
}
