//! Bridges iroh-relay-holochain metrics to OpenTelemetry.
//!
//! The iroh relay server tracks metrics using `iroh_metrics::Counter` atomics.
//! This module registers OTEL observable counters whose callbacks read those
//! atomics at export time, so the metrics flow into whatever OTLP backend is
//! configured via `--otlp_endpoint`.

use iroh_relay_holochain::server::Metrics;
use opentelemetry::metrics::ObservableCounter;
use std::sync::Arc;

/// Holds the OTEL instrument handles returned by registration.
///
/// The observable counter callbacks remain active for as long as this struct
/// is alive. Drop it to unregister the callbacks.
#[must_use]
pub struct RelayOtelMetrics {
    _instruments: Vec<ObservableCounter<u64>>,
}

/// Register OTEL observable counters that bridge iroh-relay metrics.
///
/// Each counter reads the corresponding `iroh_metrics::Counter` atomic
/// when the OTEL SDK triggers an export. When no OTLP endpoint is
/// configured the meter is a no-op and the overhead is negligible.
pub fn register(metrics: Arc<Metrics>) -> RelayOtelMetrics {
    let meter = opentelemetry::global::meter("iroh-relay");

    // A small helper to cut down on the per-metric boilerplate.
    macro_rules! counter {
        ($name:expr, $desc:expr, $field:ident) => {{
            let m = metrics.clone();
            meter
                .u64_observable_counter($name)
                .with_description($desc)
                .with_callback(move |observer| {
                    observer.observe(m.$field.get(), &[]);
                })
                .build()
        }};
        ($name:expr, $desc:expr, $unit:expr, $field:ident) => {{
            let m = metrics.clone();
            meter
                .u64_observable_counter($name)
                .with_description($desc)
                .with_unit($unit)
                .with_callback(move |observer| {
                    observer.observe(m.$field.get(), &[]);
                })
                .build()
        }};
    }

    let instruments = vec![
        counter!(
            "relay.bytes_sent",
            "Total bytes sent through the relay",
            "By",
            bytes_sent
        ),
        counter!(
            "relay.bytes_recv",
            "Total bytes received by the relay",
            "By",
            bytes_recv
        ),
        counter!("relay.accepts", "Total relay connections accepted", accepts),
        counter!(
            "relay.disconnects",
            "Total relay client disconnections",
            disconnects
        ),
        counter!(
            "relay.unique_client_keys",
            "Total unique client keys seen",
            unique_client_keys
        ),
        counter!(
            "relay.send_packets_dropped",
            "Total send packets dropped",
            send_packets_dropped
        ),
        counter!(
            "relay.other_packets_dropped",
            "Total non-send packets dropped",
            other_packets_dropped
        ),
        counter!(
            "relay.bytes_rx_ratelimited",
            "Total bytes rate-limited on receive",
            "By",
            bytes_rx_ratelimited_total
        ),
        counter!(
            "relay.conns_rx_ratelimited",
            "Total connections that had frames rate-limited",
            conns_rx_ratelimited_total
        ),
    ];

    RelayOtelMetrics {
        _instruments: instruments,
    }
}
