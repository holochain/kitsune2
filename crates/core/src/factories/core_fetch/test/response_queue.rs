use super::utils::{random_agent_id, random_id, random_op_id};
use crate::{
    default_builder,
    factories::{
        core_fetch::{CoreFetch, CoreFetchConfig},
        test_utils::AgentBuilder,
    },
};
use kitsune2_api::{
    fetch::{deserialize_ops, Fetch},
    transport::Transport,
    MetaOp, OpStore, SpaceId, Timestamp, Url,
};
use kitsune2_memory::{Kitsune2MemoryOp, Kitsune2MemoryOpStore};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

#[derive(Debug)]
pub struct MockTransport {
    responses_sent: Arc<Mutex<Vec<(Vec<MetaOp>, Url)>>>,
}

impl MockTransport {
    fn new() -> Arc<MockTransport> {
        Arc::new(Self {
            responses_sent: Arc::new(Mutex::new(Vec::new())),
        })
    }
}

impl Transport for MockTransport {
    fn send_module(
        &self,
        peer: kitsune2_api::Url,
        _space: kitsune2_api::SpaceId,
        _module: String,
        data: bytes::Bytes,
    ) -> kitsune2_api::BoxFut<'_, kitsune2_api::K2Result<()>> {
        Box::pin(async move {
            let ops = deserialize_ops(data).unwrap();
            self.responses_sent.lock().unwrap().push((ops, peer));

            Ok(())
        })
    }

    fn disconnect(
        &self,
        _peer: Url,
        _reason: Option<String>,
    ) -> kitsune2_api::BoxFut<'_, ()> {
        unimplemented!()
    }

    fn register_module_handler(
        &self,
        _space: SpaceId,
        _module: String,
        _handler: kitsune2_api::transport::DynTxModuleHandler,
    ) {
        unimplemented!()
    }

    fn register_space_handler(
        &self,
        _space: SpaceId,
        _handler: kitsune2_api::transport::DynTxSpaceHandler,
    ) {
        unimplemented!()
    }

    fn send_space_notify(
        &self,
        _peer: Url,
        _space: SpaceId,
        _data: bytes::Bytes,
    ) -> kitsune2_api::BoxFut<'_, kitsune2_api::K2Result<()>> {
        unimplemented!()
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn respond_to_multiple_requests() {
    let builder = Arc::new(default_builder().with_default_config().unwrap());
    let peer_store = builder.peer_store.create(builder.clone()).await.unwrap();
    let op_store = Arc::new(Kitsune2MemoryOpStore::default());
    let mock_transport = MockTransport::new();
    let config = CoreFetchConfig::default();
    let space_id = SpaceId::from(random_id());

    let op_id_1 = random_op_id();
    let op_id_2 = random_op_id();
    let agent_id_1 = random_agent_id();
    let agent_info_1 = AgentBuilder {
        agent: Some(agent_id_1.clone()),
        url: Some(Some(Url::from_str("wss://127.0.0.1:1").unwrap())),
        ..Default::default()
    }
    .build();
    let agent_url_1 = agent_info_1.url.clone().unwrap();

    let op_id_3 = random_op_id();
    let op_id_4 = random_op_id();
    let agent_id_2 = random_agent_id();
    let agent_info_2 = AgentBuilder {
        agent: Some(agent_id_2.clone()),
        url: Some(Some(Url::from_str("wss://127.0.0.1:2").unwrap())),
        ..Default::default()
    }
    .build();
    let agent_url_2 = agent_info_2.url.clone().unwrap();
    peer_store
        .insert(vec![agent_info_1.clone(), agent_info_2.clone()])
        .await
        .unwrap();

    // Insert ops to be read and sent into op store.
    let op_1 = MetaOp {
        op_id: op_id_1.clone(),
        op_data: serde_json::to_vec(&Kitsune2MemoryOp::new(
            op_id_1.clone(),
            Timestamp::now(),
            vec![1; 128],
        ))
        .unwrap(),
    };
    let op_2 = MetaOp {
        op_id: op_id_2.clone(),
        op_data: serde_json::to_vec(&Kitsune2MemoryOp::new(
            op_id_2.clone(),
            Timestamp::now(),
            vec![2; 128],
        ))
        .unwrap(),
    };
    let op_3 = MetaOp {
        op_id: op_id_3.clone(),
        op_data: serde_json::to_vec(&Kitsune2MemoryOp::new(
            op_id_3.clone(),
            Timestamp::now(),
            vec![3; 128],
        ))
        .unwrap(),
    };
    // Insert op 1, 2, 3 into op store. Op 4 will not be returned in the response.
    let ops_to_store = vec![op_1.clone(), op_2.clone(), op_3.clone()];
    op_store.process_incoming_ops(ops_to_store).await.unwrap();

    let fetch = CoreFetch::new(
        config.clone(),
        space_id.clone(),
        peer_store.clone(),
        op_store,
        mock_transport.clone(),
    );

    // Receive 2 op requests.
    let requested_ops_1 = vec![op_id_1.clone(), op_id_2.clone()];
    let requested_ops_2 = vec![op_id_3.clone(), op_id_4.clone()];
    futures::future::join_all([
        fetch.respond_with_ops(requested_ops_1, agent_id_1.clone()),
        fetch.respond_with_ops(requested_ops_2, agent_id_2.clone()),
    ])
    .await;

    tokio::time::timeout(Duration::from_millis(10), async {
        loop {
            tokio::time::sleep(Duration::from_millis(1)).await;
            if mock_transport.responses_sent.lock().unwrap().len() == 2 {
                break;
            }
        }
    })
    .await
    .unwrap();

    assert!(mock_transport
        .responses_sent
        .lock()
        .unwrap()
        .contains(&(vec![op_1, op_2], agent_url_1)));
    // Only op 3 is in op store.
    assert!(mock_transport
        .responses_sent
        .lock()
        .unwrap()
        .contains(&(vec![op_3], agent_url_2)));
}

#[tokio::test(flavor = "multi_thread")]
async fn no_response_sent_when_no_ops_found() {
    let builder = Arc::new(default_builder().with_default_config().unwrap());
    let peer_store = builder.peer_store.create(builder.clone()).await.unwrap();
    let op_store = Arc::new(Kitsune2MemoryOpStore::default());
    let mock_transport = MockTransport::new();
    let config = CoreFetchConfig::default();
    let space_id = SpaceId::from(random_id());

    let op_id_1 = random_op_id();
    let op_id_2 = random_op_id();
    let agent_id = random_agent_id();
    let agent_info = AgentBuilder {
        agent: Some(agent_id.clone()),
        url: Some(Some(Url::from_str("wss://127.0.0.1:1").unwrap())),
        ..Default::default()
    }
    .build();
    peer_store.insert(vec![agent_info.clone()]).await.unwrap();

    let fetch = CoreFetch::new(
        config.clone(),
        space_id.clone(),
        peer_store.clone(),
        op_store,
        mock_transport.clone(),
    );

    // Receive op request.
    let requested_ops = vec![op_id_1.clone(), op_id_2.clone()];
    fetch
        .respond_with_ops(requested_ops, agent_id.clone())
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(20)).await;

    assert!(mock_transport.responses_sent.lock().unwrap().is_empty());
}
