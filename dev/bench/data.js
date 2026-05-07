window.BENCHMARK_DATA = {
  "lastUpdate": 1778157880761,
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
      },
      {
        "commit": {
          "author": {
            "email": "christian.visintin@veeso.dev",
            "name": "Christian Visintin",
            "username": "veeso"
          },
          "committer": {
            "email": "christian.visintin@veeso.dev",
            "name": "Christian Visintin",
            "username": "veeso"
          },
          "distinct": true,
          "id": "622745e7dd369fc48be9e53c50686b80e4a23560",
          "message": "ci: temporarily run bench on this branch",
          "timestamp": "2026-05-05T09:55:46+02:00",
          "tree_id": "d61134061b4f487630e299b7052a340ba3997343",
          "url": "https://github.com/holochain/kitsune2/commit/622745e7dd369fc48be9e53c50686b80e4a23560"
        },
        "date": 1777967986473,
        "tool": "cargo",
        "benches": [
          {
            "name": "local_relay/throughput/payload/1KiB",
            "value": 90167,
            "range": "± 1149",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/8KiB",
            "value": 95103,
            "range": "± 849",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/32KiB",
            "value": 112644,
            "range": "± 1974",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/roundtrip/1KiB/localhost",
            "value": 41963491,
            "range": "± 49693",
            "unit": "ns/iter"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "christian.visintin@veeso.dev",
            "name": "Christian Visintin",
            "username": "veeso"
          },
          "committer": {
            "email": "christian.visintin@veeso.dev",
            "name": "Christian Visintin",
            "username": "veeso"
          },
          "distinct": true,
          "id": "517f4a3b64a772e48393bd71ce019c13511b5586",
          "message": "ci: run cargo bench across the workspace on push to main\n\nAdd a `Bench` workflow that enumerates `[[bench]]` targets via\n`cargo metadata` and runs each one with `--output-format bencher` so the\nresults can be tracked by `benchmark-action/github-action-benchmark`.\nHistory is persisted on the `bench-history` branch and any benchmark\nthat drifts more than 150% versus the previous run fails the workflow.\n\nAlso fix the `iroh_relay_bench` so it actually runs: iroh-relay 0.98\nrequires a TLS client config to be set on `ClientBuilder` even when\ntalking to a local `http://` / `ws://` relay, otherwise the connect\npath panics with `MissingCryptoProvider`. Use the in-tree dangerous\nclient config and enable the `tls-aws-lc-rs` and `test-utils` features\non the `iroh-relay` dev-dependency to make it available.\n\nCloses #530",
          "timestamp": "2026-05-06T09:36:14+02:00",
          "tree_id": "f99a331ceb200ab4fe1a1ca0bf67d044d62c3286",
          "url": "https://github.com/holochain/kitsune2/commit/517f4a3b64a772e48393bd71ce019c13511b5586"
        },
        "date": 1778053530073,
        "tool": "cargo",
        "benches": [
          {
            "name": "local_relay/throughput/payload/1KiB",
            "value": 86888,
            "range": "± 718",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/8KiB",
            "value": 96950,
            "range": "± 893",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/32KiB",
            "value": 111924,
            "range": "± 2549",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/roundtrip/1KiB/localhost",
            "value": 41999861,
            "range": "± 19515",
            "unit": "ns/iter"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "49699333+dependabot[bot]@users.noreply.github.com",
            "name": "dependabot[bot]",
            "username": "dependabot[bot]"
          },
          "committer": {
            "email": "ThetaSinner@users.noreply.github.com",
            "name": "ThetaSinner",
            "username": "ThetaSinner"
          },
          "distinct": true,
          "id": "b39eaad6e29ceb422d7e2c05891281b2b6357686",
          "message": "build(deps): bump johnwason/vcpkg-action from 7 to 8\n\nBumps [johnwason/vcpkg-action](https://github.com/johnwason/vcpkg-action) from 7 to 8.\n- [Release notes](https://github.com/johnwason/vcpkg-action/releases)\n- [Commits](https://github.com/johnwason/vcpkg-action/compare/v7...v8)\n\n---\nupdated-dependencies:\n- dependency-name: johnwason/vcpkg-action\n  dependency-version: '8'\n  dependency-type: direct:production\n  update-type: version-update:semver-major\n...\n\nSigned-off-by: dependabot[bot] <support@github.com>",
          "timestamp": "2026-05-07T13:35:52+01:00",
          "tree_id": "4fba322a8d84b0248ab8700201149b598c99276d",
          "url": "https://github.com/holochain/kitsune2/commit/b39eaad6e29ceb422d7e2c05891281b2b6357686"
        },
        "date": 1778157880379,
        "tool": "cargo",
        "benches": [
          {
            "name": "local_relay/throughput/payload/1KiB",
            "value": 65394,
            "range": "± 479",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/8KiB",
            "value": 71313,
            "range": "± 1240",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/32KiB",
            "value": 85688,
            "range": "± 1363",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/roundtrip/1KiB/localhost",
            "value": 41999727,
            "range": "± 1470",
            "unit": "ns/iter"
          }
        ]
      }
    ]
  }
}