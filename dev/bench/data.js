window.BENCHMARK_DATA = {
  "lastUpdate": 1777888358685,
  "repoUrl": "https://github.com/holochain/kitsune2",
  "entries": {
    "Kitsune2 Benchmarks": [
      {
        "commit": {
          "author": {
            "name": "Christian Visintin",
            "username": "veeso",
            "email": "christian.visintin@veeso.dev"
          },
          "committer": {
            "name": "Christian Visintin",
            "username": "veeso",
            "email": "christian.visintin@veeso.dev"
          },
          "id": "13059a9cbb812df746b82b24c3d140b00b4f4d12",
          "message": "fix(bootstrap_srv): supply iroh relay client TLS config in bench\n\niroh-relay 0.98 requires `tls_client_config` to be set on\n`ClientBuilder` before calling `connect`, otherwise the connect path\npanics with `MissingCryptoProvider`. Use the in-tree dangerous client\nconfig (no cert verification) since the bench targets a local\n`http://`/`ws://` relay only. Enables the `tls-aws-lc-rs` and\n`test-utils` features on `iroh-relay` as a dev-dependency to make the\nhelper available.",
          "timestamp": "2026-05-04T09:32:54Z",
          "url": "https://github.com/holochain/kitsune2/commit/13059a9cbb812df746b82b24c3d140b00b4f4d12"
        },
        "date": 1777888358350,
        "tool": "cargo",
        "benches": [
          {
            "name": "local_relay/throughput/payload/1KiB",
            "value": 91419,
            "range": "± 637",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/8KiB",
            "value": 95369,
            "range": "± 538",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/32KiB",
            "value": 108407,
            "range": "± 2887",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/roundtrip/1KiB/localhost",
            "value": 41958606,
            "range": "± 77548",
            "unit": "ns/iter"
          }
        ]
      }
    ]
  }
}