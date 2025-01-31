use crate::gossip::{DropAbortHandle, GossipResponse, K2Gossip};
use crate::peer_meta_store::K2PeerMetaStore;
use crate::protocol::{deserialize_gossip_message, GossipMessage};
use crate::K2GossipConfig;
use bytes::Bytes;
use kitsune2_api::transport::{TxBaseHandler, TxHandler};
use kitsune2_api::Timestamp;
use kitsune2_core::default_test_builder;
use kitsune2_dht::Dht;
use kitsune2_test_utils::space::TEST_SPACE_ID;
use rand::RngCore;
use std::sync::Arc;
use tokio::sync::RwLock;

pub(crate) struct RespondTestHarness {
    pub(crate) gossip: K2Gossip,
    response_rx: tokio::sync::mpsc::Receiver<GossipResponse>,
}

impl RespondTestHarness {
    pub(crate) async fn create() -> Self {
        RespondTestHarness::create_with_config(Default::default()).await
    }

    pub(crate) async fn create_with_config(config: K2GossipConfig) -> Self {
        let builder =
            Arc::new(default_test_builder().with_default_config().unwrap());

        let op_store = builder
            .op_store
            .create(builder.clone(), TEST_SPACE_ID)
            .await
            .unwrap();

        let dht = Dht::try_from_store(Timestamp::now(), op_store.clone())
            .await
            .unwrap();

        #[derive(Debug)]
        struct NoopHandler;

        impl TxBaseHandler for NoopHandler {}
        impl TxHandler for NoopHandler {}

        let transport = builder
            .transport
            .create(builder.clone(), Arc::new(NoopHandler))
            .await
            .unwrap();

        let (tx, rx) = tokio::sync::mpsc::channel(1);

        Self {
            gossip: K2Gossip {
                config: Arc::new(config),
                initiated_round_state: Default::default(),
                accepted_round_states: Default::default(),
                dht: Arc::new(RwLock::new(dht)),
                space_id: TEST_SPACE_ID,
                peer_store: builder
                    .peer_store
                    .create(builder.clone())
                    .await
                    .unwrap(),
                local_agent_store: builder
                    .local_agent_store
                    .create(builder.clone())
                    .await
                    .unwrap(),
                peer_meta_store: Arc::new(K2PeerMetaStore::new(
                    builder
                        .peer_meta_store
                        .create(builder.clone())
                        .await
                        .unwrap(),
                    TEST_SPACE_ID,
                )),
                op_store: op_store.clone(),
                fetch: builder
                    .fetch
                    .create(
                        builder.clone(),
                        TEST_SPACE_ID,
                        op_store.clone(),
                        transport,
                    )
                    .await
                    .unwrap(),
                agent_verifier: builder.verifier.clone(),
                response_tx: tx,
                _response_task: Arc::new(DropAbortHandle {
                    name: "response_task".to_string(),
                    handle: tokio::spawn(async move {}).abort_handle(),
                }),
                _initiate_task: Default::default(),
                _timeout_task: Default::default(),
                _dht_update_task: Default::default(),
            },
            response_rx: rx,
        }
    }

    pub(crate) async fn wait_for_response(&mut self) -> GossipMessage {
        let received = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            self.response_rx.recv(),
        )
        .await
        .unwrap()
        .unwrap();

        deserialize_gossip_message(received.0).unwrap()
    }
}

pub(crate) fn test_session_id() -> Bytes {
    let mut session_id = bytes::BytesMut::zeroed(12);
    rand::thread_rng().fill_bytes(&mut session_id);

    session_id.freeze()
}
