//! Test helpers

use bytes::Bytes;
use kitsune2_api::agent::{AgentInfo, Verifier};
use kitsune2_api::{builder, DynOpStore, SpaceId};
use kitsune2_core::default_builder;
use kitsune2_core::factories::{CoreBootstrapFactory, MemOpStoreFactory};
use std::sync::Arc;

#[derive(Debug)]
struct DummyVerifier;
impl Verifier for DummyVerifier {
    fn verify(
        &self,
        _agent_info: &AgentInfo,
        _message: &[u8],
        _signature: &[u8],
    ) -> bool {
        unimplemented!()
    }
}

pub async fn test_store() -> DynOpStore {
    let builder = builder::Builder {
        verifier: Arc::new(DummyVerifier),
        bootstrap: CoreBootstrapFactory::create(),
        ..default_builder()
    }
    .with_default_config()
    .unwrap();
    MemOpStoreFactory::create()
        .create(
            Arc::new(builder),
            SpaceId::from(Bytes::from_static("test-space".as_bytes())),
        )
        .await
        .unwrap()
}
