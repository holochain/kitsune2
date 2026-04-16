//! Integration test: verify that relay traffic produces OTEL metrics
//! through the production server wiring.
//!
//! Boots a real `BootstrapSrv` (which installs the relay metrics guard
//! via the production `http.rs` code path), exchanges traffic between
//! two iroh endpoints, then flushes an in-memory OTEL exporter and
//! asserts that the expected metric names appear with values > 0.

#![cfg(feature = "iroh-relay")]

use iroh::{EndpointAddr, RelayConfig, RelayMap, RelayUrl};
use iroh_base::SecretKey;
use kitsune2_bootstrap_srv::{BootstrapSrv, Config};
use opentelemetry_sdk::metrics::{
    InMemoryMetricExporter, PeriodicReader, SdkMeterProvider,
    data::{AggregatedMetrics, MetricData},
};
use rand_chacha::rand_core::SeedableRng;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;

const TEST_ALPN: &[u8] = b"relay-metrics-test";

#[test]
fn relay_metrics_exported_via_otel() {
    let _ = rustls::crypto::ring::default_provider().install_default();

    // --- OTEL setup with in-memory exporter ---
    // Install *before* BootstrapSrv::new so the production code path
    // picks up this meter provider when it calls register().
    let exporter = InMemoryMetricExporter::default();
    let reader = PeriodicReader::builder(exporter.clone()).build();
    let meter_provider =
        SdkMeterProvider::builder().with_reader(reader).build();
    opentelemetry::global::set_meter_provider(meter_provider.clone());

    // --- Boot the real server ---
    let s = BootstrapSrv::new(Config {
        prune_interval: std::time::Duration::from_millis(5),
        ..Config::testing()
    })
    .unwrap();
    let addr = s.listen_addrs()[0];

    // --- Connect two iroh endpoints and exchange a message ---
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .unwrap();

    rt.block_on(async {
        let relay_url =
            RelayUrl::from_str(&format!("http://{addr:?}")).unwrap();
        let relay_map = RelayMap::empty();
        relay_map.insert(
            relay_url.clone(),
            Arc::new(RelayConfig {
                quic: None,
                url: relay_url.clone(),
            }),
        );

        let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(99);

        let ep_1 = iroh::Endpoint::empty_builder()
            .relay_mode(iroh::RelayMode::Custom(relay_map.clone()))
            .secret_key(SecretKey::generate(&mut rng))
            .alpns(vec![TEST_ALPN.to_vec()])
            .ca_roots_config(
                iroh_relay::tls::CaRootsConfig::insecure_skip_verify(),
            )
            .bind()
            .await
            .unwrap();

        let ep_2 = iroh::Endpoint::empty_builder()
            .relay_mode(iroh::RelayMode::Custom(relay_map))
            .secret_key(SecretKey::generate(&mut rng))
            .alpns(vec![TEST_ALPN.to_vec()])
            .ca_roots_config(
                iroh_relay::tls::CaRootsConfig::insecure_skip_verify(),
            )
            .bind()
            .await
            .unwrap();

        let ep_2_addr = EndpointAddr::new(ep_2.id()).with_relay_url(relay_url);

        let message = b"hello";

        let accept_task = tokio::spawn(async move {
            let conn = ep_2.accept().await.unwrap().await.unwrap();
            let mut recv = conn.accept_uni().await.unwrap();
            let data = recv.read_to_end(1_000).await.unwrap();
            conn.close(0u8.into(), b"");
            ep_2.close().await;
            data
        });

        let conn = ep_1.connect(ep_2_addr, TEST_ALPN).await.unwrap();
        let mut send_stream = conn.open_uni().await.unwrap();
        send_stream.write_all(message).await.unwrap();
        send_stream.finish().unwrap();

        assert_eq!(accept_task.await.unwrap(), message);
        ep_1.close().await;
    });

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

    // After connecting two endpoints and sending a message through the
    // relay, these counters must have been incremented.
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
