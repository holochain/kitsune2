# kitsune2

p2p / dht communication framework

## Testing the bootstrap server metrics exporter

Create a new file called `otel-config.yml`:

```yaml
receivers:
  otlp:
    protocols:
      http:
        endpoint: 0.0.0.0:4318

exporters:
  debug:
    verbosity: detailed

service:
  pipelines:
    metrics:
      receivers: [otlp]
      exporters: [debug]
```

Then start the metrics service with:

```shell
docker run --rm -p 4318:4318  -v "$PWD/otel-config.yaml:/etc/otelcol/config.yaml" otel/opentelemetry-collector:latest
```

The bootstrap server can then be configured to report metrics to the OTLP endpoint:

```shell
cargo run -- --listen 0.0.0.0:9000 --otlp-endpoint http://localhost:4318/v1/metrics
```

You will see metrics in the Docker log as they are reported.
