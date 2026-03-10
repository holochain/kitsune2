#![cfg(feature = "metrics")]

use bytes::Bytes;
use kitsune2_api::Url;
use kitsune2_test_utils::retry_fn_until_timeout;
use kitsune2_test_utils::space::TEST_SPACE_ID;
use kitsune2_transport_iroh::test_utils::{
    IrohTransportTestHarness, MockTxHandler,
};
use opentelemetry_sdk::metrics::{
    InMemoryMetricExporter, PeriodicReader, SdkMeterProvider,
    data::{AggregatedMetrics, MetricData},
};
use std::sync::Arc;
use std::time::Duration;

#[tokio::test(flavor = "multi_thread")]
async fn connection_counter() {
    let exporter = InMemoryMetricExporter::default();

    // Create a PeriodicReader with a short interval
    let reader = PeriodicReader::builder(exporter.clone())
        .with_interval(Duration::from_millis(500))
        .build();

    // Build the meter provider
    let meter_provider =
        SdkMeterProvider::builder().with_reader(reader).build();
    opentelemetry::global::set_meter_provider(meter_provider.clone());

    // Set up two endpoints and establish a connection.
    let harness = IrohTransportTestHarness::new().await;
    let dummy_url = Url::from_str("http://url.not.set:0/0").unwrap();

    let handler_1 = Arc::new(MockTxHandler::default());
    let ep_1 = harness.build_transport(handler_1.clone()).await;
    ep_1.register_space_handler(TEST_SPACE_ID, handler_1.clone());

    assert!(
        ep_1.get_connected_peers().await.unwrap().is_empty(),
        "peers connected to ep_1 should be empty"
    );
    let handler_2 = Arc::new(MockTxHandler::default());
    let ep_2 = harness.build_transport(handler_2.clone()).await;
    ep_2.register_space_handler(TEST_SPACE_ID, handler_2.clone());

    // Wait for URLs to be updated
    retry_fn_until_timeout(
        || async {
            let ep_1_url = handler_1.current_url.lock().unwrap().clone();
            let ep_2_url = handler_2.current_url.lock().unwrap().clone();
            ep_1_url != dummy_url && ep_2_url != dummy_url
        },
        Some(5000),
        Some(500),
    )
    .await
    .unwrap();

    // Flush metrics
    meter_provider.force_flush().unwrap();

    // Expect connection counter metric to not have recorded anything.
    let resource_metrics = exporter.get_finished_metrics().unwrap();
    assert!(resource_metrics.is_empty());

    // Send a message from ep_1 to ep_2 to establish a connection.
    let ep2_url = handler_2.current_url.lock().unwrap().clone();
    ep_1.send_space_notify(
        ep2_url,
        TEST_SPACE_ID,
        Bytes::from_static(b"hello"),
    )
    .await
    .unwrap();

    // Expect connection counter metric to have recorded the connection.
    // Both endpoints are using the same process, therefore the same global
    // metrics provider, so it should be 2 connections, one from each
    // endpoint's perspective.
    retry_fn_until_timeout(
        || async {
            let resource_metrics = exporter.get_finished_metrics().unwrap();
            if resource_metrics.is_empty() {
                return false;
            }
            let last_resource_metric = resource_metrics.last().unwrap();

            // The resource metric contains exactly one scoped metric.
            let scoped_metrics =
                last_resource_metric.scope_metrics().collect::<Vec<_>>();
            assert_eq!(scoped_metrics.len(), 1);
            // Scoped metrics contain exactly one metric.
            let metrics = scoped_metrics[0].metrics().collect::<Vec<_>>();
            assert_eq!(metrics.len(), 1);
            assert_eq!(metrics[0].name(), "kitsune2.transport.connections");
            assert_eq!(
                metrics[0].description(),
                "The number of active connections"
            );
            if let AggregatedMetrics::I64(MetricData::Sum(sum)) =
                metrics[0].data()
            {
                let data_points = sum.data_points().collect::<Vec<_>>();
                assert_eq!(data_points.len(), 1);
                assert_eq!(data_points[0].attributes().count(), 0);
                // The count should be 2
                data_points[0].value() == 2
            } else {
                panic!("expected an i64 metric");
            }
        },
        Some(5000),
        Some(500),
    )
    .await
    .expect("timed out waiting for connection counter to go up to 2");

    // ep 2 disconnects
    drop(ep_2);

    // Check that connection count has gone down to 1.
    // 1 because ep_2 is just dropped, so doesn't record any metric, but ep_1
    // records that ep_2 has disconnected.
    retry_fn_until_timeout(
        || async {
            // A resource metric gets written at every periodic read.
            let resource_metrics = exporter.get_finished_metrics().unwrap();
            if resource_metrics.len() < 2 {
                return false;
            }
            // Check last recorded metric.
            let last_resource_metric = resource_metrics.last().unwrap();

            // The resource metric contains exactly one scoped metric.
            let scoped_metrics =
                last_resource_metric.scope_metrics().collect::<Vec<_>>();
            assert_eq!(scoped_metrics.len(), 1);

            // The scoped metric contains exactly one metric.
            let metrics = scoped_metrics[0].metrics().collect::<Vec<_>>();
            assert_eq!(metrics.len(), 1);
            assert_eq!(metrics[0].name(), "kitsune2.transport.connections");
            assert_eq!(
                metrics[0].description(),
                "The number of active connections"
            );
            if let AggregatedMetrics::I64(MetricData::Sum(sum)) =
                metrics[0].data()
            {
                let data_points = sum.data_points().collect::<Vec<_>>();
                assert_eq!(data_points.len(), 1);
                assert_eq!(data_points[0].attributes().count(), 0);
                // The count should be 1
                data_points[0].value() == 1
            } else {
                panic!("expected an i64 metric");
            }
        },
        Some(5000),
        Some(500),
    )
    .await
    .expect("timed out waiting for connection counter to go down to 1");
}
