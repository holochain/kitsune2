//! OpenTelemetry metrics initialization for the bootstrap server.
//!
//! Sets up the OTLP metric exporter and installs it as the global meter
//! provider. This is feature-independent — both the SBD and iroh-relay
//! backends use the same provider.

use opentelemetry::global;
use opentelemetry_otlp::WithExportConfig;

/// Initialize the global OTEL meter provider with an OTLP HTTP exporter.
///
/// If `otlp_endpoint` is `None`, this is a no-op and all meters returned
/// by `opentelemetry::global::meter()` will be silent.
///
/// Returns the [`opentelemetry_sdk::metrics::SdkMeterProvider`] when an
/// endpoint is configured so the caller can flush pending metric batches
/// during graceful shutdown.
pub fn enable_otlp_metrics(
    otlp_endpoint: Option<&str>,
) -> std::io::Result<Option<opentelemetry_sdk::metrics::SdkMeterProvider>> {
    let Some(endpoint) = otlp_endpoint else {
        tracing::info!("OTLP metrics export not configured");
        return Ok(None);
    };

    tracing::info!("Enabling OpenTelemetry metrics export");

    let exporter = opentelemetry_otlp::MetricExporter::builder()
        .with_http()
        .with_protocol(opentelemetry_otlp::Protocol::HttpBinary)
        .with_endpoint(endpoint)
        .build()
        .map_err(|e| {
            std::io::Error::other(format!(
                "failed to create OTLP exporter: {e}"
            ))
        })?;

    let meter_provider =
        opentelemetry_sdk::metrics::SdkMeterProvider::builder()
            .with_periodic_exporter(exporter)
            .build();

    global::set_meter_provider(meter_provider.clone());

    Ok(Some(meter_provider))
}
