//! Integration test: verify that relay traffic produces OTEL metrics.
//!
//! Spins up an axum relay server, connects two clients, exchanges a
//! message, then flushes an in-memory OTEL exporter and asserts that
//! the expected metric names appear with values > 0.

#![cfg(feature = "iroh-relay")]

use axum::Router;
use axum::routing::get;
use futures::{SinkExt, StreamExt};
use iroh_base::{RelayUrl, SecretKey};
use iroh_relay_holochain::{
    client::ClientBuilder,
    dns::DnsResolver,
    protos::relay::{ClientToRelayMsg, Datagrams, RelayToClientMsg},
};
use opentelemetry_sdk::metrics::{
    InMemoryMetricExporter, PeriodicReader, SdkMeterProvider,
    data::{AggregatedMetrics, MetricData},
};
use rand_chacha::rand_core::SeedableRng;
use std::collections::HashMap;
use std::net::Ipv4Addr;
use tokio::net::TcpListener;

#[tokio::test]
async fn relay_metrics_exported_via_otel() {
    // --- OTEL setup with in-memory exporter ---
    let exporter = InMemoryMetricExporter::default();
    let reader = PeriodicReader::builder(exporter.clone()).build();
    let meter_provider =
        SdkMeterProvider::builder().with_reader(reader).build();
    opentelemetry::global::set_meter_provider(meter_provider.clone());

    // --- Relay server ---
    let state = kitsune2_bootstrap_srv::iroh_relay_axum::create_relay_state();
    let _otel_metrics = kitsune2_bootstrap_srv::iroh_relay_metrics::register(
        state.metrics.clone(),
    );

    let app = Router::new()
        .route(
            "/relay",
            get(kitsune2_bootstrap_srv::iroh_relay_axum::relay_handler),
        )
        .with_state(state);

    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server_handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let relay_url: RelayUrl = format!("http://{addr}/relay").parse().unwrap();
    let resolver = DnsResolver::new();

    // --- Connect two clients and exchange a message ---
    let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(99);

    let a_secret = SecretKey::generate(&mut rng);
    let mut client_a =
        ClientBuilder::new(relay_url.clone(), a_secret, resolver.clone())
            .connect()
            .await
            .unwrap();

    let b_secret = SecretKey::generate(&mut rng);
    let b_key = b_secret.public();
    let mut client_b = ClientBuilder::new(relay_url, b_secret, resolver)
        .connect()
        .await
        .unwrap();

    let msg = Datagrams::from("hello");
    client_a
        .send(ClientToRelayMsg::Datagrams {
            dst_endpoint_id: b_key,
            datagrams: msg.clone(),
        })
        .await
        .unwrap();

    let received = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        client_b.next(),
    )
    .await
    .expect("timeout waiting for message")
    .expect("stream ended")
    .unwrap();

    match received {
        RelayToClientMsg::Datagrams { datagrams, .. } => {
            assert_eq!(datagrams, msg);
        }
        other => panic!("unexpected message: {other:?}"),
    }

    drop(client_a);
    drop(client_b);
    server_handle.abort();

    // --- Flush and inspect exported metrics ---
    meter_provider.force_flush().unwrap();

    let resource_metrics = exporter.get_finished_metrics().unwrap();

    // Collect all metric names and their summed values.
    let mut metric_values: HashMap<String, u64> = HashMap::new();
    for rm in &resource_metrics {
        for sm in rm.scope_metrics() {
            for metric in sm.metrics() {
                if let AggregatedMetrics::U64(data) = metric.data()
                    && let MetricData::Sum(sum) = data
                {
                    let total: u64 =
                        sum.data_points().map(|dp| dp.value()).sum();
                    metric_values.insert(metric.name().to_string(), total);
                }
            }
        }
    }

    // After connecting two clients and sending a message, these
    // counters must have been incremented.
    assert!(
        *metric_values.get("relay.bytes_sent").unwrap_or(&0) > 0,
        "expected relay.bytes_sent > 0, got {metric_values:?}"
    );
    assert!(
        *metric_values.get("relay.bytes_recv").unwrap_or(&0) > 0,
        "expected relay.bytes_recv > 0, got {metric_values:?}"
    );
    assert!(
        *metric_values.get("relay.accepts").unwrap_or(&0) >= 2,
        "expected relay.accepts >= 2, got {metric_values:?}"
    );
    assert!(
        *metric_values.get("relay.unique_client_keys").unwrap_or(&0) >= 2,
        "expected relay.unique_client_keys >= 2, got {metric_values:?}"
    );
}
