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
pub fn enable_otlp_metrics(otlp_endpoint: Option<&str>) -> std::io::Result<()> {
    let Some(endpoint) = otlp_endpoint else {
        tracing::info!("OTLP metrics export not configured");
        return Ok(());
    };

    tracing::info!("Enabling OpenTelemetry metrics export to {endpoint}");

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

    global::set_meter_provider(meter_provider);

    Ok(())
}
