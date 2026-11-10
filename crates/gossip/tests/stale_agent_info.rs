use kitsune2_api::{DhtArc, Timestamp, Url};
use kitsune2_gossip::RespondTestHarness;
use kitsune2_test_utils::agent::AgentBuilder;
use std::time::Duration;

/// A stale response selected before a bootstrap update may arrive after it.
#[tokio::test]
async fn in_flight_stale_gossip_response_does_not_replace_bootstrap_update() {
    // Alice receives updates; Bob caches Sue's old advertisement.
    let alice_harness = RespondTestHarness::create().await;
    let mut bob_harness = RespondTestHarness::create().await;
    let alice = alice_harness.create_agent(DhtArc::FULL).await;
    let bob = bob_harness.create_agent(DhtArc::FULL).await;
    let sue = alice_harness.create_agent(DhtArc::FULL).await;
    let old_url = Url::from_str("ws://test:80/old").unwrap();
    let new_url = Url::from_str("ws://test:80/new").unwrap();
    let now = Timestamp::now();
    let older = AgentBuilder {
        created_at: Some(now),
        url: Some(Some(old_url.clone())),
        ..Default::default()
    }
    .build(sue.local.clone());
    let newer = AgentBuilder {
        created_at: Some(now + Duration::from_secs(1)),
        url: Some(Some(new_url.clone())),
        ..Default::default()
    }
    .build(sue.local.clone());
    bob_harness.peer_store().insert(vec![older]).await.unwrap();

    // Alice requests Sue while Bob still has Sue's old advertisement.
    let (session_id, stale_response) = bob_harness
        .prepare_agents_response(&bob, &alice, sue.agent.clone(), now)
        .await;

    // Alice learns Sue's new advertisement while Bob's response is in flight.
    alice_harness
        .peer_store()
        .insert(vec![newer.clone()])
        .await
        .unwrap();

    // Bob's delayed response delivers Sue's old advertisement to Alice.
    alice_harness
        .receive_agents_response(&alice, &bob, session_id, stale_response)
        .await;

    // Alice's peer store retains Sue's new advertisement.
    assert_eq!(
        alice_harness
            .peer_store()
            .get(sue.agent.clone())
            .await
            .unwrap()
            .unwrap(),
        newer
    );

    // Alice resolves Sue only through the new endpoint.
    assert_eq!(
        alice_harness
            .known_peers()
            .get_by_url(new_url)
            .await
            .unwrap(),
        vec![sue.agent.clone()]
    );
    assert!(
        alice_harness
            .known_peers()
            .get_by_url(old_url)
            .await
            .unwrap()
            .is_empty()
    );
}
