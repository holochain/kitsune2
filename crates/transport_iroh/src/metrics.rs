use std::sync::OnceLock;

static CONNECTION_COUNTER_METRIC: OnceLock<ConnectionCounterMetric> =
    OnceLock::new();

pub(crate) type ConnectionCounterMetric =
    opentelemetry::metrics::UpDownCounter<i64>;

pub(crate) fn connection_counter_metric() -> &'static ConnectionCounterMetric {
    CONNECTION_COUNTER_METRIC.get_or_init(|| {
        opentelemetry::global::meter("kitsune2")
            .i64_up_down_counter("kitsune2.transport.connections")
            .with_description("The number of open connections")
            .build()
    })
}
