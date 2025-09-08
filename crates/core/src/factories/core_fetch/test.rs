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
    use std::sync::Arc;

    #[tokio::test]
    async fn invoke_report_module() {
        static GOT_SPACE: std::sync::atomic::AtomicBool =
            std::sync::atomic::AtomicBool::new(false);
        static FETCHED_OP_COUNT: std::sync::atomic::AtomicU64 =
            std::sync::atomic::AtomicU64::new(0);
        static FETCHED_SIZE_BYTES: std::sync::atomic::AtomicU64 =
            std::sync::atomic::AtomicU64::new(0);

        #[derive(Debug)]
        struct R;
        impl Report for R {
            fn as_any(&self) -> &dyn std::any::Any {
                self
            }
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
        struct RF;
        impl ReportFactory for RF {
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
                    let out: DynReport = Arc::new(R);
                    Ok(out)
                })
            }
        }
        let builder = Builder {
            report: Arc::new(RF),
            ..crate::default_test_builder()
        }
        .with_default_config()
        .unwrap();

        let k = builder.build().await.unwrap();

        #[derive(Debug)]
        struct S;
        impl SpaceHandler for S {
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
        struct K;
        impl KitsuneHandler for K {
            fn create_space(
                &self,
                _space_id: SpaceId,
            ) -> BoxFut<'_, K2Result<DynSpaceHandler>> {
                let s: DynSpaceHandler = Arc::new(S);
                Box::pin(async move { Ok(s) })
            }
        }
        k.register_handler(Arc::new(K)).await.unwrap();
        let s = k
            .space(kitsune2_test_utils::space::TEST_SPACE_ID)
            .await
            .unwrap();
        let local_agent: DynLocalAgent =
            Arc::new(kitsune2_test_utils::agent::TestLocalAgent::default());
        s.local_agent_join(local_agent.clone()).await.unwrap();

        let url = s.current_url().unwrap();

        assert!(GOT_SPACE.load(std::sync::atomic::Ordering::SeqCst));

        #[derive(Debug)]
        struct T;
        impl TxBaseHandler for T {}
        impl TxHandler for T {}

        let builder = Arc::new(crate::default_test_builder());
        let tx = builder
            .transport
            .create(builder.clone(), Arc::new(T))
            .await
            .unwrap();

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
