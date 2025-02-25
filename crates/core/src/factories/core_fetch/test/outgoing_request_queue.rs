use super::test_utils::random_peer_url;
use crate::{
    default_test_builder,
    factories::{
        core_fetch::{CoreFetch, CoreFetchConfig},
        MemOpStoreFactory,
    },
};
use kitsune2_api::*;
use kitsune2_test_utils::{
    id::{create_op_id_list, random_op_id},
    iter_check,
    space::TEST_SPACE_ID,
};
use prost::Message;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

type RequestsSent = Vec<(OpId, Url)>;

struct TestCase {
    fetch: CoreFetch,
    requests_sent: Arc<Mutex<RequestsSent>>,
}

async fn setup_test(
    config: &CoreFetchConfig,
    faulty_connection: bool,
) -> TestCase {
    let builder =
        Arc::new(default_test_builder().with_default_config().unwrap());
    let op_store = MemOpStoreFactory::create()
        .create(builder.clone(), TEST_SPACE_ID)
        .await
        .unwrap();
    let requests_sent = Arc::new(Mutex::new(Vec::new()));
    let mock_transport =
        make_mock_transport(requests_sent.clone(), faulty_connection);

    let fetch = CoreFetch::new(
        config.clone(),
        TEST_SPACE_ID,
        op_store,
        mock_transport.clone(),
    );

    TestCase {
        fetch,
        requests_sent,
    }
}

fn make_mock_transport(
    requests_sent: Arc<Mutex<RequestsSent>>,
    faulty_connection: bool,
) -> Arc<MockTransport> {
    let mut mock_transport = MockTransport::new();
    mock_transport.expect_send_module().returning({
        let requests_sent = requests_sent.clone();
        move |peer, space, module, data| {
            assert_eq!(space, TEST_SPACE_ID);
            assert_eq!(module, crate::factories::core_fetch::MOD_NAME);
            Box::pin({
                let requests_sent = requests_sent.clone();
                async move {
                    let fetch_message = K2FetchMessage::decode(data).unwrap();
                    let op_ids: Vec<OpId> =
                        match fetch_message.fetch_message_type() {
                            FetchMessageType::Request => {
                                let request =
                                    FetchRequest::decode(fetch_message.data)
                                        .unwrap();
                                request.into()
                            }
                            _ => panic!("unexpected fetch message type"),
                        };
                    let mut lock = requests_sent.lock().unwrap();
                    op_ids.into_iter().for_each(|op_id| {
                        lock.push((op_id, peer.clone()));
                    });

                    if faulty_connection {
                        Err(K2Error::other("connection timed out"))
                    } else {
                        Ok(())
                    }
                }
            })
        }
    });
    mock_transport
        .expect_register_module_handler()
        .returning(|_, _, _| ());
    Arc::new(mock_transport)
}

#[tokio::test(flavor = "multi_thread")]
#[cfg_attr(
    windows,
    ignore = "outgoing_request_queue.rs:154:6: called `Result::unwrap()` on an `Err` value: Elapsed(())"
)]
async fn outgoing_request_queue() {
    let config = CoreFetchConfig {
        re_insert_outgoing_request_delay_ms: 50,
        ..Default::default()
    };
    let TestCase {
        fetch,
        requests_sent,
        ..
    } = setup_test(&config, false).await;

    let op_id = random_op_id();
    let op_list = vec![op_id.clone()];
    let peer_url = random_peer_url();

    assert!(requests_sent.lock().unwrap().is_empty());

    // Add 1 op.
    fetch.request_ops(op_list, peer_url.clone()).await.unwrap();

    // Let the fetch request be sent multiple times. As only 1 op was added to the queue,
    // this proves that it is being re-added to the queue after sending a request for it.
    iter_check!({
        if requests_sent.lock().unwrap().len() >= 2 {
            break;
        }
    });

    // Clear set of ops to fetch to stop sending requests.
    fetch.state.lock().unwrap().requests.clear();

    let num_requests_sent = requests_sent.lock().unwrap().len();

    // Check that all requests have been made for the 1 op to the agent.
    assert!(requests_sent
        .lock()
        .unwrap()
        .iter()
        .all(|request| request == &(op_id.clone(), peer_url.clone())));

    // Give time for more requests to be sent, which shouldn't happen now that the set of
    // ops to fetch is cleared.
    tokio::time::sleep(Duration::from_millis(
        (config.re_insert_outgoing_request_delay_ms * 2) as u64,
    ))
    .await;

    // No more requests should have been sent.
    // Ideally it were possible to check that no more fetch request have been passed back into
    // the internal channel, but that would require a custom wrapper around the channel.
    let requests_sent = requests_sent.lock().unwrap().clone();
    assert_eq!(requests_sent.len(), num_requests_sent);
}

#[tokio::test(flavor = "multi_thread")]
async fn happy_op_fetch_from_multiple_agents() {
    let config = CoreFetchConfig {
        parallel_request_count: 5,
        ..Default::default()
    };
    let TestCase {
        fetch,
        requests_sent,
        ..
    } = setup_test(&config, false).await;

    let op_list_1 = create_op_id_list(10);
    let op_list_2 = create_op_id_list(20);
    let op_list_3 = create_op_id_list(30);
    let total_ops = op_list_1.len() + op_list_2.len() + op_list_3.len();

    let peer_url_1 = random_peer_url();
    let peer_url_2 = random_peer_url();
    let peer_url_3 = random_peer_url();

    let mut expected_requests = Vec::new();
    op_list_1
        .clone()
        .into_iter()
        .for_each(|op_id| expected_requests.push((op_id, peer_url_1.clone())));
    op_list_2
        .clone()
        .into_iter()
        .for_each(|op_id| expected_requests.push((op_id, peer_url_2.clone())));
    op_list_3
        .clone()
        .into_iter()
        .for_each(|op_id| expected_requests.push((op_id, peer_url_3.clone())));
    let mut expected_ops = Vec::new();
    op_list_1
        .clone()
        .into_iter()
        .for_each(|op_id| expected_ops.push((op_id, peer_url_1.clone())));
    op_list_2
        .clone()
        .into_iter()
        .for_each(|op_id| expected_ops.push((op_id, peer_url_2.clone())));
    op_list_3
        .clone()
        .into_iter()
        .for_each(|op_id| expected_ops.push((op_id, peer_url_3.clone())));

    futures::future::join_all([
        fetch.request_ops(op_list_1.clone(), peer_url_1.clone()),
        fetch.request_ops(op_list_2.clone(), peer_url_2.clone()),
        fetch.request_ops(op_list_3.clone(), peer_url_3.clone()),
    ])
    .await;

    // Check that at least one request was sent for each op.
    iter_check!({
        let requests_sent = requests_sent.lock().unwrap().clone();
        if requests_sent.len() >= total_ops
            && expected_requests
                .iter()
                .all(|expected_op| requests_sent.contains(expected_op))
        {
            break;
        }
    });

    // Check that op ids are still part of ops to fetch.
    let lock = fetch.state.lock().unwrap();
    assert!(expected_ops.iter().all(|v| lock.requests.contains(v)));
}

#[tokio::test(flavor = "multi_thread")]
async fn unresponsive_agents_are_put_on_back_off_list() {
    let TestCase {
        fetch,
        requests_sent,
        ..
    } = setup_test(&CoreFetchConfig::default(), true).await;

    let op_list_1 = create_op_id_list(5);
    let op_list_2 = create_op_id_list(5);

    let peer_url_1 = random_peer_url();
    let peer_url_2 = random_peer_url();

    // Add all ops to the queue.
    futures::future::join_all([
        fetch.request_ops(op_list_1.clone(), peer_url_1.clone()),
        fetch.request_ops(op_list_2.clone(), peer_url_2.clone()),
    ])
    .await;

    // Wait for one request for each agent.
    let expected_peer_url = [peer_url_1, peer_url_2];
    iter_check!({
        let requests_sent = requests_sent.lock().unwrap();
        let request_destinations = requests_sent
            .iter()
            .map(|(_, agent_id)| agent_id)
            .collect::<Vec<_>>();
        if expected_peer_url
            .iter()
            .all(|peer_url| request_destinations.contains(&peer_url))
        {
            // Check all agents are on back off list.
            let back_off_list = &mut fetch.state.lock().unwrap().back_off_list;
            if expected_peer_url
                .iter()
                .all(|peer_url| back_off_list.is_peer_on_back_off(peer_url))
            {
                break;
            }
        }
    });
}

#[tokio::test(flavor = "multi_thread")]
async fn agent_on_back_off_is_removed_from_list_after_successful_send() {
    let config = CoreFetchConfig {
        first_back_off_interval_ms: 10,
        ..Default::default()
    };
    let TestCase {
        fetch,
        requests_sent,
    } = setup_test(&config, false).await;

    let op_list = create_op_id_list(1);
    let peer_url = random_peer_url();

    let first_back_off_interval = {
        let mut lock = fetch.state.lock().unwrap();
        lock.back_off_list.back_off_peer(&peer_url);

        assert!(lock.back_off_list.is_peer_on_back_off(&peer_url));

        lock.back_off_list.first_back_off_interval_ms
    };

    tokio::time::sleep(Duration::from_millis(first_back_off_interval as u64))
        .await;

    fetch.request_ops(op_list, peer_url.clone()).await.unwrap();

    iter_check!({
        if !requests_sent.lock().unwrap().is_empty() {
            break;
        }
    });

    assert!(fetch.state.lock().unwrap().back_off_list.state.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn requests_are_dropped_when_max_back_off_expired() {
    let config = CoreFetchConfig {
        first_back_off_interval_ms: 10,
        last_back_off_interval_ms: 10,
        re_insert_outgoing_request_delay_ms: 10,
        ..Default::default()
    };
    let TestCase {
        fetch,
        requests_sent,
    } = setup_test(&config, true).await;

    let op_list_1 = create_op_id_list(2);
    let peer_url_1 = random_peer_url();

    // Create a second agent to later check that their ops have not been removed.
    let op_list_2 = create_op_id_list(2);
    let peer_url_2 = random_peer_url();

    fetch
        .request_ops(op_list_1.clone(), peer_url_1.clone())
        .await
        .unwrap();

    // Add control agent's ops to set.
    fetch
        .request_ops(op_list_2, peer_url_2.clone())
        .await
        .unwrap();

    // Back off agent the maximum possible number of times.
    let last_back_off_interval = {
        let mut lock = fetch.state.lock().unwrap();
        assert!(op_list_1.iter().all(|op_id| {
            lock.requests.contains(&(op_id.clone(), peer_url_1.clone()))
        }));
        for _ in 0..config.num_back_off_intervals + 1 {
            lock.back_off_list.back_off_peer(&peer_url_1);
        }

        lock.back_off_list.last_back_off_interval_ms
    };

    // Wait for back off interval to expire. Afterwards the request should fail again and all
    // of the agent's requests should be removed from the set.
    tokio::time::sleep(Duration::from_millis(last_back_off_interval as u64))
        .await;

    assert!(fetch
        .state
        .lock()
        .unwrap()
        .back_off_list
        .has_last_back_off_expired(&peer_url_1));

    let current_number_of_requests_to_agent_1 = requests_sent
        .lock()
        .unwrap()
        .iter()
        .filter(|(_, a)| *a != peer_url_1)
        .count();

    // Wait for another request attempt to agent 1, which should remove all of their requests
    // from the set.
    iter_check!({
        if requests_sent
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, a)| *a != peer_url_1)
            .count()
            > current_number_of_requests_to_agent_1
        {
            break;
        }
    });

    assert!(fetch
        .state
        .lock()
        .unwrap()
        .requests
        .iter()
        .all(|(_, peer_url)| *peer_url != peer_url_1),);
}
