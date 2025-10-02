mod incoming_request_queue;
mod incoming_response_queue;
mod outgoing_request_queue;

#[cfg(test)]
pub(crate) mod test_utils {
    use crate::factories::MemoryOp;
    use kitsune2_api::{Timestamp, Url};
    use rand::RngCore;

    pub fn random_peer_url() -> Url {
        let id = rand::thread_rng().next_u32();
        Url::from_str(format!("ws://test:80/{id}")).unwrap()
    }

    pub fn make_op(data: Vec<u8>) -> MemoryOp {
        MemoryOp::new(Timestamp::now(), data)
    }
}

#[cfg(test)]
mod tests {
    use kitsune2_api::*;
    use kitsune2_test_utils::{
        agent::AgentBuilder, enable_tracing, iter_check,
    };
    use std::sync::Arc;

    #[tokio::test]
    async fn invoke_report_module() {
        enable_tracing();
        static GOT_SPACE: std::sync::atomic::AtomicBool =
            std::sync::atomic::AtomicBool::new(false);
        static FETCHED_OP_COUNT: std::sync::atomic::AtomicU64 =
            std::sync::atomic::AtomicU64::new(0);
        static FETCHED_SIZE_BYTES: std::sync::atomic::AtomicU64 =
            std::sync::atomic::AtomicU64::new(0);

        #[derive(Debug)]
        struct MockReport;
        impl Report for MockReport {
            fn space(
                &self,
                _space_id: SpaceId,
                _local_agent_store: DynLocalAgentStore,
            ) {
                GOT_SPACE.store(true, std::sync::atomic::Ordering::SeqCst);
            }
            fn fetched_op(
                &self,
                _space_id: SpaceId,
                _source: Url,
                _op_id: OpId,
                size_bytes: u64,
            ) {
                FETCHED_OP_COUNT
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                FETCHED_SIZE_BYTES
                    .fetch_add(size_bytes, std::sync::atomic::Ordering::SeqCst);
            }
        }
        #[derive(Debug)]
        struct MockReportFactory;
        impl ReportFactory for MockReportFactory {
            fn default_config(&self, _config: &mut Config) -> K2Result<()> {
                Ok(())
            }
            fn validate_config(&self, _config: &Config) -> K2Result<()> {
                Ok(())
            }
            fn create(
                &self,
                _builder: Arc<Builder>,
                _tx: crate::DynTransport,
            ) -> BoxFut<'static, K2Result<DynReport>> {
                Box::pin(async move {
                    let out: DynReport = Arc::new(MockReport);
                    Ok(out)
                })
            }
        }
        let builder = Builder {
            report: Arc::new(MockReportFactory),
            ..crate::default_test_builder()
        }
        .with_default_config()
        .unwrap();

        let kitsune = builder.build().await.unwrap();

        #[derive(Debug)]
        struct MockSpaceHandler;
        impl SpaceHandler for MockSpaceHandler {
            fn recv_notify(
                &self,
                _from_peer: Url,
                _space_id: SpaceId,
                _data: bytes::Bytes,
            ) -> K2Result<()> {
                Ok(())
            }
        }
        #[derive(Debug)]
        struct MockKitsuneHandler;
        impl KitsuneHandler for MockKitsuneHandler {
            fn create_space(
                &self,
                _space_id: SpaceId,
            ) -> BoxFut<'_, K2Result<DynSpaceHandler>> {
                let s: DynSpaceHandler = Arc::new(MockSpaceHandler);
                Box::pin(async move { Ok(s) })
            }
        }
        kitsune
            .register_handler(Arc::new(MockKitsuneHandler))
            .await
            .unwrap();
        let space = kitsune
            .space(kitsune2_test_utils::space::TEST_SPACE_ID)
            .await
            .unwrap();
        let local_agent: DynLocalAgent =
            Arc::new(kitsune2_test_utils::agent::TestLocalAgent::default());
        space.local_agent_join(local_agent.clone()).await.unwrap();

        let url = space.current_url().unwrap();

        assert!(GOT_SPACE.load(std::sync::atomic::Ordering::SeqCst));

        #[derive(Debug)]
        struct MockTxHandler;
        impl TxBaseHandler for MockTxHandler {}
        impl TxHandler for MockTxHandler {}

        let builder = Arc::new(crate::default_test_builder());
        let tx: Arc<dyn Transport> = builder
            .transport
            .create(builder.clone(), Arc::new(MockTxHandler))
            .await
            .unwrap();

        // We need to add an agent info of the sending peer to the receiving
        // peer's peer store so that it won't consider the peer blocked and
        // drop the message.
        // TODO: Change kitsune to be able to deal with that case without
        // requiring manual insert, then remove it.
        let url_sender = iter_check!(200, {
            let stats = tx.dump_network_stats().await.unwrap();
            let peer_url = stats.transport_stats.peer_urls.first();
            if let Some(url) = peer_url {
                return url.clone();
            }
        });

        let local_agent_2: DynLocalAgent =
            Arc::new(kitsune2_test_utils::agent::TestLocalAgent::default());

        let agent_sender = AgentBuilder::default()
            .with_url(Some(url_sender.clone()))
            .build(local_agent_2);

        space.peer_store().insert(vec![agent_sender]).await.unwrap();

        tx.send_module(
            url,
            kitsune2_test_utils::space::TEST_SPACE_ID,
            super::super::MOD_NAME.to_string(),
            serialize_response_message(vec![
                crate::factories::MemoryOp::new(Timestamp::now(), vec![1])
                    .into(),
                crate::factories::MemoryOp::new(Timestamp::now(), vec![2])
                    .into(),
            ]),
        )
        .await
        .unwrap();

        for _ in 0..40 {
            let op_count =
                FETCHED_OP_COUNT.load(std::sync::atomic::Ordering::SeqCst);
            let size_bytes =
                FETCHED_SIZE_BYTES.load(std::sync::atomic::Ordering::SeqCst);

            if op_count == 2 && size_bytes == 90 {
                // test pass
                return;
            }

            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }

        panic!("failed to get report of fetched ops");
    }
}
