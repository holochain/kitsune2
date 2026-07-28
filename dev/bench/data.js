window.BENCHMARK_DATA = {
  "lastUpdate": 1785254706347,
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
      },
      {
        "commit": {
          "author": {
            "email": "ThetaSinner@users.noreply.github.com",
            "name": "ThetaSinner",
            "username": "ThetaSinner"
          },
          "committer": {
            "email": "ThetaSinner@users.noreply.github.com",
            "name": "ThetaSinner",
            "username": "ThetaSinner"
          },
          "distinct": true,
          "id": "1e76f0b810f50ca19b6b94ed71e168771de98780",
          "message": "feat: export iroh relay metrics via OpenTelemetry\n\nBridge iroh-relay-holochain metrics (bytes_sent, bytes_recv, accepts,\ndisconnects, unique_client_keys, packets dropped, rate limiting) to\nOTEL using observable counters that read the iroh-metrics atomics at\nexport time.\n\nMove OTEL meter provider initialization out of the SBD feature gate\nso metrics export works regardless of which relay backend is active.\n\nAdd integration test that verifies the full chain: relay handles\nclient traffic, iroh-metrics atomics increment, OTEL exporter\ncaptures the values.\n\n# Conflicts:\n#\tCargo.lock\n#\tCargo.toml\n#\tcrates/bootstrap_srv/Cargo.toml",
          "timestamp": "2026-05-13T15:34:29+01:00",
          "tree_id": "1a3722cc90586a72848fd9a8f001b214dbb20b71",
          "url": "https://github.com/holochain/kitsune2/commit/1e76f0b810f50ca19b6b94ed71e168771de98780"
        },
        "date": 1778683348893,
        "tool": "cargo",
        "benches": [
          {
            "name": "local_relay/throughput/payload/1KiB",
            "value": 72841,
            "range": "± 4882",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/8KiB",
            "value": 76882,
            "range": "± 4179",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/32KiB",
            "value": 92583,
            "range": "± 4644",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/roundtrip/1KiB/localhost",
            "value": 41999846,
            "range": "± 6339",
            "unit": "ns/iter"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "ThetaSinner@users.noreply.github.com",
            "name": "ThetaSinner",
            "username": "ThetaSinner"
          },
          "committer": {
            "email": "ThetaSinner@users.noreply.github.com",
            "name": "ThetaSinner",
            "username": "ThetaSinner"
          },
          "distinct": true,
          "id": "7f6986c0b062f695a158571cd71822e04297d585",
          "message": "chore: bump iroh to 1.0.0-rc.0 and refresh workspace dependencies\n\nBumps to the iroh 1.0 release-candidate line and rolls a batch of other\nworkspace deps. Adapts the bootstrap_srv relay integration and the\ntransport_iroh connection layer to the new iroh / iroh-relay APIs.\n\nNotable holds: opentelemetry stays at 0.30 (sbd-server 0.4.0 pins it\ntransitively) and schemars stays at 0.9 (tx5-connection 0.8.1 pins it).\nBump those once upstreams publish compatible releases.\n\nAPI adaptations:\n- iroh-relay 1.0: StreamError is now an alias for AnyError; switch to\n  from_std. ServerConfig / QuicConfig / client::Config are\n  non-exhaustive; use the new() / Default constructors.\n- AccessConfig::Restricted callback now takes &ClientRequest; rework\n  the axum handler to extract Request, run WebSocketUpgrade via\n  FromRequestParts, snapshot a fresh Parts, and thread it into the\n  handshake so the access-check call sees a real ClientRequest.\n- iroh-relay 1.0: dns module moved out into the iroh-dns crate; add it\n  as a workspace dev-dep and update imports.\n- iroh 1.0: Connection::paths() returns PathList<'_> directly (no\n  Watcher); drop the .get() and the n0_watcher::Watcher import.\n- rcgen 0.14: CertifiedKey::key_pair renamed to signing_key.",
          "timestamp": "2026-05-20T22:06:14+01:00",
          "tree_id": "a164c5d5cb2f1ef35184eb61edd3b3a5e8d94030",
          "url": "https://github.com/holochain/kitsune2/commit/7f6986c0b062f695a158571cd71822e04297d585"
        },
        "date": 1779311781032,
        "tool": "cargo",
        "benches": [
          {
            "name": "local_relay/throughput/payload/1KiB",
            "value": 81364,
            "range": "± 5935",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/8KiB",
            "value": 85597,
            "range": "± 4639",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/32KiB",
            "value": 106829,
            "range": "± 4231",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/roundtrip/1KiB/localhost",
            "value": 41965065,
            "range": "± 82428",
            "unit": "ns/iter"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "6267702+ThetaSinner@users.noreply.github.com",
            "name": "ThetaSinner",
            "username": "ThetaSinner"
          },
          "committer": {
            "email": "ThetaSinner@users.noreply.github.com",
            "name": "ThetaSinner",
            "username": "ThetaSinner"
          },
          "distinct": true,
          "id": "674525a8ebc80bae27fe480443bc2dc2b3f5c039",
          "message": "chore: Prepare next release",
          "timestamp": "2026-05-20T23:57:13+01:00",
          "tree_id": "1fcc7a05383fd905e2dc748f2db8ffb2f08956c9",
          "url": "https://github.com/holochain/kitsune2/commit/674525a8ebc80bae27fe480443bc2dc2b3f5c039"
        },
        "date": 1779318013617,
        "tool": "cargo",
        "benches": [
          {
            "name": "local_relay/throughput/payload/1KiB",
            "value": 80191,
            "range": "± 6101",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/8KiB",
            "value": 85180,
            "range": "± 4440",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/32KiB",
            "value": 99324,
            "range": "± 3028",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/roundtrip/1KiB/localhost",
            "value": 41999986,
            "range": "± 14183",
            "unit": "ns/iter"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdunster@users.noreply.github.com",
            "name": "Callum Dunster",
            "username": "cdunster"
          },
          "committer": {
            "email": "cdunster@users.noreply.github.com",
            "name": "Callum Dunster",
            "username": "cdunster"
          },
          "distinct": true,
          "id": "62344701e2a4e8eb292551fc15789ce038b42a97",
          "message": "test: fix the string comparision in metrics integration tests",
          "timestamp": "2026-05-28T14:46:13+02:00",
          "tree_id": "2f908b59e3b1632a406848ef17d4056cec5f6349",
          "url": "https://github.com/holochain/kitsune2/commit/62344701e2a4e8eb292551fc15789ce038b42a97"
        },
        "date": 1779972566183,
        "tool": "cargo",
        "benches": [
          {
            "name": "local_relay/throughput/payload/1KiB",
            "value": 70572,
            "range": "± 5012",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/8KiB",
            "value": 77173,
            "range": "± 3963",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/32KiB",
            "value": 91687,
            "range": "± 5401",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/roundtrip/1KiB/localhost",
            "value": 41753561,
            "range": "± 101275",
            "unit": "ns/iter"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdunster@users.noreply.github.com",
            "name": "Callum Dunster",
            "username": "cdunster"
          },
          "committer": {
            "email": "cdunster@users.noreply.github.com",
            "name": "Callum Dunster",
            "username": "cdunster"
          },
          "distinct": true,
          "id": "432b4845d44bb4a123641b1d89d32e74b7b5af2f",
          "message": "refactor: simplify logic to override metadata",
          "timestamp": "2026-06-02T15:03:21+02:00",
          "tree_id": "f37b3fadd4f28f359f0f192b634eac272f8dcd12",
          "url": "https://github.com/holochain/kitsune2/commit/432b4845d44bb4a123641b1d89d32e74b7b5af2f"
        },
        "date": 1780405598883,
        "tool": "cargo",
        "benches": [
          {
            "name": "local_relay/throughput/payload/1KiB",
            "value": 79562,
            "range": "± 3162",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/8KiB",
            "value": 82305,
            "range": "± 4688",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/32KiB",
            "value": 105484,
            "range": "± 5633",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/roundtrip/1KiB/localhost",
            "value": 41893193,
            "range": "± 185157",
            "unit": "ns/iter"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "synchwire@users.noreply.github.com",
            "name": "synchwire",
            "username": "synchwire"
          },
          "committer": {
            "email": "ThetaSinner@users.noreply.github.com",
            "name": "ThetaSinner",
            "username": "ThetaSinner"
          },
          "distinct": true,
          "id": "63af6d4ae5260b9d72ed3bd9bca34089c582cbcb",
          "message": "test(transport_iroh): add integration test for no false-unresponsive on relay drop\n\nTestBootstrapSrv now calls `Clients::shutdown()` after the kill signal\nfires, which gracefully closes all relay WebSocket connections. This lets\nconnected iroh endpoints detect relay loss immediately (via\n`RelayConnectionState::Disconnected` with `last_error`) rather than\nwaiting for the 60-second QUIC idle timeout.\n\nThe new integration test `no_unresponsive_when_relay_drops` uses this:\nit drops the bootstrap server, polls until the `is_home_relay_known_down`\nguard fires (outbound send returns `RELAY_NOT_CONNECTED_ERR` near-\ninstantly), then asserts that `set_unresponsive` was never called.",
          "timestamp": "2026-06-05T13:36:17+01:00",
          "tree_id": "0b3052120e5fdeacb2a79e910d3c04cecb1d2d16",
          "url": "https://github.com/holochain/kitsune2/commit/63af6d4ae5260b9d72ed3bd9bca34089c582cbcb"
        },
        "date": 1780663552259,
        "tool": "cargo",
        "benches": [
          {
            "name": "local_relay/throughput/payload/1KiB",
            "value": 57038,
            "range": "± 2931",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/8KiB",
            "value": 63291,
            "range": "± 2456",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/32KiB",
            "value": 80058,
            "range": "± 1753",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/roundtrip/1KiB/localhost",
            "value": 41997734,
            "range": "± 52812",
            "unit": "ns/iter"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdunster@users.noreply.github.com",
            "name": "Callum Dunster",
            "username": "cdunster"
          },
          "committer": {
            "email": "ThetaSinner@users.noreply.github.com",
            "name": "ThetaSinner",
            "username": "ThetaSinner"
          },
          "distinct": true,
          "id": "9b6d2397bbce83afade0ab30a69438dc88cfdf9a",
          "message": "chore: update iroh dependency to latest rc.1 release",
          "timestamp": "2026-06-09T13:03:49+01:00",
          "tree_id": "fd657be59bd07935ffaa1518d7b2654269bc2454",
          "url": "https://github.com/holochain/kitsune2/commit/9b6d2397bbce83afade0ab30a69438dc88cfdf9a"
        },
        "date": 1781007222580,
        "tool": "cargo",
        "benches": [
          {
            "name": "local_relay/throughput/payload/1KiB",
            "value": 72394,
            "range": "± 3191",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/8KiB",
            "value": 74076,
            "range": "± 3652",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/32KiB",
            "value": 90960,
            "range": "± 4805",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/roundtrip/1KiB/localhost",
            "value": 41996414,
            "range": "± 60195",
            "unit": "ns/iter"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "6267702+ThetaSinner@users.noreply.github.com",
            "name": "ThetaSinner",
            "username": "ThetaSinner"
          },
          "committer": {
            "email": "ThetaSinner@users.noreply.github.com",
            "name": "ThetaSinner",
            "username": "ThetaSinner"
          },
          "distinct": true,
          "id": "5c47e17712f917b916c4d0c0b817c8b8289e18b6",
          "message": "chore: Prepare next release",
          "timestamp": "2026-06-09T15:55:02+01:00",
          "tree_id": "8ff6a7f9d02fb4a19f07b6857e2d5c3b4b77dc08",
          "url": "https://github.com/holochain/kitsune2/commit/5c47e17712f917b916c4d0c0b817c8b8289e18b6"
        },
        "date": 1781017107933,
        "tool": "cargo",
        "benches": [
          {
            "name": "local_relay/throughput/payload/1KiB",
            "value": 80984,
            "range": "± 5778",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/8KiB",
            "value": 86010,
            "range": "± 5152",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/32KiB",
            "value": 115020,
            "range": "± 6492",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/roundtrip/1KiB/localhost",
            "value": 41999213,
            "range": "± 27863",
            "unit": "ns/iter"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "christian.visintin@veeso.dev",
            "name": "veeso",
            "username": "veeso"
          },
          "committer": {
            "email": "ThetaSinner@users.noreply.github.com",
            "name": "ThetaSinner",
            "username": "ThetaSinner"
          },
          "distinct": true,
          "id": "514a6592066c81d14283093eb734de548cf704cb",
          "message": "feat(bootstrap_srv): rate-limit inbound bytes on embedded iroh relay\n\nAdds per-connection inbound byte rate limiting at the axum WebSocket\nframe layer of the embedded iroh relay handler, using iroh 1.0.0's\nnow-public iroh_relay::server::streams::Bucket primitive. No fork: the\nBucket primitive that previously required pinning the holochain/iroh\nfork is public as of iroh 1.0.0 on crates.io.\n\nConfigurable via two Config fields and matching CLI flags\n(--relay-client-rx-bytes-per-second, --relay-client-rx-burst-bytes).\nOff by default. When the sustained rate is set without an explicit\nburst, the burst defaults to one tenth of bps to match iroh's own\nRateLimited::from_cfg behaviour.\n\nBumps the workspace iroh stack from 1.0.0-rc.1 to 1.0.0 and migrates\nthe rc-era CaRootsConfig/ca_roots_config to the renamed\nCaTlsConfig/ca_tls_config across bootstrap_srv and transport_iroh.\n\nCloses #501.",
          "timestamp": "2026-06-18T16:11:47+01:00",
          "tree_id": "e9267330477e8ab84dd3015cd4c8e54ce5be6cd0",
          "url": "https://github.com/holochain/kitsune2/commit/514a6592066c81d14283093eb734de548cf704cb"
        },
        "date": 1781796061389,
        "tool": "cargo",
        "benches": [
          {
            "name": "local_relay/throughput/payload/1KiB",
            "value": 57092,
            "range": "± 2093",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/8KiB",
            "value": 62797,
            "range": "± 2391",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/32KiB",
            "value": 80653,
            "range": "± 1692",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/roundtrip/1KiB/localhost",
            "value": 41999828,
            "range": "± 1453",
            "unit": "ns/iter"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "ThetaSinner@users.noreply.github.com",
            "name": "ThetaSinner",
            "username": "ThetaSinner"
          },
          "committer": {
            "email": "ThetaSinner@users.noreply.github.com",
            "name": "ThetaSinner",
            "username": "ThetaSinner"
          },
          "distinct": true,
          "id": "68f36ec6a9554e2c0ad1619d8f4686bac9ebb640",
          "message": "refactor: remove leftover tx5/sbd references\n\nThe tx5 removal deleted the sbd module and Cargo feature but left\ndead references to them behind:\n\n- bootstrap_srv: drop the broken sbd-feature clippy/test tasks from\n  Makefile.toml and the unused no-sbd / sbd-* CLI args\n- delete the unbuildable kitsune2_bootstrap_srv_sbd Docker image and\n  its CI build jobs in docker-build.yaml and test.yaml\n- refresh stale doc comments and CLAUDE.md that still named tx5/sbd\n\nThe holochain/sbd spec-auth.md links are kept; they point to the\nexternal auth protocol the code still implements.",
          "timestamp": "2026-06-25T12:33:34+01:00",
          "tree_id": "3dc2314bd42685eaf41d99b9e018cb6301225588",
          "url": "https://github.com/holochain/kitsune2/commit/68f36ec6a9554e2c0ad1619d8f4686bac9ebb640"
        },
        "date": 1782387808852,
        "tool": "cargo",
        "benches": [
          {
            "name": "local_relay/throughput/payload/1KiB",
            "value": 77756,
            "range": "± 6350",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/8KiB",
            "value": 84881,
            "range": "± 4861",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/32KiB",
            "value": 99275,
            "range": "± 3489",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/roundtrip/1KiB/localhost",
            "value": 41907101,
            "range": "± 58966",
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
          "id": "bb80fcae1e9194ef3df94813a26f3b83679de8dd",
          "message": "build(deps): bump holochain/actions/.github/workflows/changelog-preview-comment.yml\n\nBumps [holochain/actions/.github/workflows/changelog-preview-comment.yml](https://github.com/holochain/actions) from 1.8.0 to 1.14.0.\n- [Release notes](https://github.com/holochain/actions/releases)\n- [Commits](https://github.com/holochain/actions/compare/v1.8.0...v1.14.0)\n\n---\nupdated-dependencies:\n- dependency-name: holochain/actions/.github/workflows/changelog-preview-comment.yml\n  dependency-version: 1.14.0\n  dependency-type: direct:production\n  update-type: version-update:semver-minor\n...\n\nSigned-off-by: dependabot[bot] <support@github.com>",
          "timestamp": "2026-06-25T14:05:41+01:00",
          "tree_id": "57eb7efb0180f94f81e8c923c7c19967282ff28e",
          "url": "https://github.com/holochain/kitsune2/commit/bb80fcae1e9194ef3df94813a26f3b83679de8dd"
        },
        "date": 1782393498738,
        "tool": "cargo",
        "benches": [
          {
            "name": "local_relay/throughput/payload/1KiB",
            "value": 71802,
            "range": "± 4227",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/8KiB",
            "value": 75664,
            "range": "± 4246",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/32KiB",
            "value": 91275,
            "range": "± 3346",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/roundtrip/1KiB/localhost",
            "value": 41730630,
            "range": "± 126204",
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
          "id": "c3f7fe9825c0ae9d030452abf776014d8a5151f5",
          "message": "build(deps): bump actions/checkout from 6 to 7\n\nBumps [actions/checkout](https://github.com/actions/checkout) from 6 to 7.\n- [Release notes](https://github.com/actions/checkout/releases)\n- [Changelog](https://github.com/actions/checkout/blob/main/CHANGELOG.md)\n- [Commits](https://github.com/actions/checkout/compare/v6...v7)\n\n---\nupdated-dependencies:\n- dependency-name: actions/checkout\n  dependency-version: '7'\n  dependency-type: direct:production\n  update-type: version-update:semver-major\n...\n\nSigned-off-by: dependabot[bot] <support@github.com>",
          "timestamp": "2026-06-25T15:09:49+01:00",
          "tree_id": "573eb4d62b58283e2a4a5f49a87d5cd1963fa7b0",
          "url": "https://github.com/holochain/kitsune2/commit/c3f7fe9825c0ae9d030452abf776014d8a5151f5"
        },
        "date": 1782397224201,
        "tool": "cargo",
        "benches": [
          {
            "name": "local_relay/throughput/payload/1KiB",
            "value": 78861,
            "range": "± 3727",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/8KiB",
            "value": 85224,
            "range": "± 5478",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/32KiB",
            "value": 97140,
            "range": "± 5101",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/roundtrip/1KiB/localhost",
            "value": 41998236,
            "range": "± 55460",
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
          "id": "d83db6e15389d421372e36a61c7e7738217c5ae9",
          "message": "build(deps): bump holochain/actions/.github/workflows/prepare-release.yml\n\nBumps [holochain/actions/.github/workflows/prepare-release.yml](https://github.com/holochain/actions) from 1.8.0 to 1.14.0.\n- [Release notes](https://github.com/holochain/actions/releases)\n- [Commits](https://github.com/holochain/actions/compare/v1.8.0...v1.14.0)\n\n---\nupdated-dependencies:\n- dependency-name: holochain/actions/.github/workflows/prepare-release.yml\n  dependency-version: 1.14.0\n  dependency-type: direct:production\n  update-type: version-update:semver-minor\n...\n\nSigned-off-by: dependabot[bot] <support@github.com>",
          "timestamp": "2026-06-25T15:55:39+01:00",
          "tree_id": "7ec28364808cf9f590798e8ad6f4a6f4f4394c23",
          "url": "https://github.com/holochain/kitsune2/commit/d83db6e15389d421372e36a61c7e7738217c5ae9"
        },
        "date": 1782399538802,
        "tool": "cargo",
        "benches": [
          {
            "name": "local_relay/throughput/payload/1KiB",
            "value": 75081,
            "range": "± 6386",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/8KiB",
            "value": 82695,
            "range": "± 3519",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/32KiB",
            "value": 100363,
            "range": "± 4403",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/roundtrip/1KiB/localhost",
            "value": 42000714,
            "range": "± 3329",
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
          "id": "1fe9a48660a606067d9bc84d2d14e262f78a9150",
          "message": "build(deps): bump holochain/actions/.github/workflows/publish-release.yml\n\nBumps [holochain/actions/.github/workflows/publish-release.yml](https://github.com/holochain/actions) from 1.8.0 to 1.14.0.\n- [Release notes](https://github.com/holochain/actions/releases)\n- [Commits](https://github.com/holochain/actions/compare/v1.8.0...v1.14.0)\n\n---\nupdated-dependencies:\n- dependency-name: holochain/actions/.github/workflows/publish-release.yml\n  dependency-version: 1.14.0\n  dependency-type: direct:production\n  update-type: version-update:semver-minor\n...\n\nSigned-off-by: dependabot[bot] <support@github.com>",
          "timestamp": "2026-06-25T17:01:01+01:00",
          "tree_id": "da08f8c3d05d0479420d8848ddf7a66ee580f7df",
          "url": "https://github.com/holochain/kitsune2/commit/1fe9a48660a606067d9bc84d2d14e262f78a9150"
        },
        "date": 1782403454867,
        "tool": "cargo",
        "benches": [
          {
            "name": "local_relay/throughput/payload/1KiB",
            "value": 79554,
            "range": "± 3753",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/8KiB",
            "value": 83123,
            "range": "± 4137",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/32KiB",
            "value": 105131,
            "range": "± 5309",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/roundtrip/1KiB/localhost",
            "value": 42000650,
            "range": "± 2117",
            "unit": "ns/iter"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "6267702+ThetaSinner@users.noreply.github.com",
            "name": "ThetaSinner",
            "username": "ThetaSinner"
          },
          "committer": {
            "email": "ThetaSinner@users.noreply.github.com",
            "name": "ThetaSinner",
            "username": "ThetaSinner"
          },
          "distinct": true,
          "id": "207056277d05597e51d0d99aba13e3b79db67c92",
          "message": "chore: Prepare next release",
          "timestamp": "2026-06-25T19:18:10+01:00",
          "tree_id": "5a2d68fa79f9a4d49304347c510903cd1adfc19f",
          "url": "https://github.com/holochain/kitsune2/commit/207056277d05597e51d0d99aba13e3b79db67c92"
        },
        "date": 1782411857178,
        "tool": "cargo",
        "benches": [
          {
            "name": "local_relay/throughput/payload/1KiB",
            "value": 71872,
            "range": "± 3957",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/8KiB",
            "value": 75806,
            "range": "± 5243",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/32KiB",
            "value": 95624,
            "range": "± 4309",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/roundtrip/1KiB/localhost",
            "value": 41780005,
            "range": "± 87166",
            "unit": "ns/iter"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "synchwire@users.noreply.github.com",
            "name": "synchwire",
            "username": "synchwire"
          },
          "committer": {
            "email": "ThetaSinner@users.noreply.github.com",
            "name": "ThetaSinner",
            "username": "ThetaSinner"
          },
          "distinct": true,
          "id": "caa0b02bfbb9ba3d3a80be4e21bbd9ddd4b2d95a",
          "message": "feat(bootstrap_srv): bound relay connection establishment with a timeout\n\nThe bootstrap server serves the iroh relay through its own axum route\ninstead of iroh's `RelayService`, so it did not inherit iroh-relay's 30s\nconnection establish timeout (iroh PR #4083, still present in iroh-relay\n1.0). A stalled or malicious client could hold a half-open TCP/TLS\nconnection to `/relay` without ever completing the WebSocket upgrade,\ntying up server resources indefinitely.\n\nAdd an equivalent establish timeout at the connection level (the only\nplace that can see the pre-upgrade window; a Tower/route layer runs only\nafter hyper has parsed the request head):\n\n- `EstablishTimeoutAcceptor` bounds the inner acceptor's TLS handshake\n  and wraps the byte stream in `EstablishTimeoutStream`.\n- `EstablishTimeoutStream` enforces a deadline on the wait for the first\n  request byte, then becomes transparent. This is required because\n  hyper's auto builder blocks in HTTP-version detection on that first\n  byte before its header-read timeout engages, so a fully-silent client\n  would otherwise never be dropped.\n- `configure_establish_timeout` arms hyper's http1 `header_read_timeout`\n  (and the `TokioTimer` it requires) for the slow-trickle-headers case.\n\nA single 30s `ESTABLISH_TIMEOUT` mirrors upstream. Once the request head\nis read, neither timeout applies, so long-lived relay connections are\nunaffected; they remain bound by the existing relay handshake and\nper-client write timeouts.\n\nCloses #526",
          "timestamp": "2026-06-30T10:26:46+01:00",
          "tree_id": "0831e76923afd69ae89a27b72f544a5b8d215c71",
          "url": "https://github.com/holochain/kitsune2/commit/caa0b02bfbb9ba3d3a80be4e21bbd9ddd4b2d95a"
        },
        "date": 1782812251759,
        "tool": "cargo",
        "benches": [
          {
            "name": "local_relay/throughput/payload/1KiB",
            "value": 78349,
            "range": "± 5205",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/8KiB",
            "value": 82775,
            "range": "± 5576",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/32KiB",
            "value": 102799,
            "range": "± 5538",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/roundtrip/1KiB/localhost",
            "value": 41999699,
            "range": "± 13260",
            "unit": "ns/iter"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "6267702+ThetaSinner@users.noreply.github.com",
            "name": "ThetaSinner",
            "username": "ThetaSinner"
          },
          "committer": {
            "email": "ThetaSinner@users.noreply.github.com",
            "name": "ThetaSinner",
            "username": "ThetaSinner"
          },
          "distinct": true,
          "id": "9a497178f0c86ce8ee9d351b07f4eb378892e424",
          "message": "chore: Prepare next release",
          "timestamp": "2026-06-30T18:30:48+01:00",
          "tree_id": "f4381aab2f065f3746b266d8a3f030ac9d7aa707",
          "url": "https://github.com/holochain/kitsune2/commit/9a497178f0c86ce8ee9d351b07f4eb378892e424"
        },
        "date": 1782840834442,
        "tool": "cargo",
        "benches": [
          {
            "name": "local_relay/throughput/payload/1KiB",
            "value": 82178,
            "range": "± 5075",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/8KiB",
            "value": 87063,
            "range": "± 5798",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/32KiB",
            "value": 111564,
            "range": "± 5216",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/roundtrip/1KiB/localhost",
            "value": 42001497,
            "range": "± 36566",
            "unit": "ns/iter"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "synchwire@users.noreply.github.com",
            "name": "synchwire",
            "username": "synchwire"
          },
          "committer": {
            "email": "ThetaSinner@users.noreply.github.com",
            "name": "ThetaSinner",
            "username": "ThetaSinner"
          },
          "distinct": true,
          "id": "768b01b11dbe50c15fb36f554bbb77dcf7a5a351",
          "message": "feat(bootstrap_srv): add TLS security headers to relay HTTP responses\n\nUpstream iroh-relay adds Strict-Transport-Security and\nContent-Security-Policy headers to all responses when TLS is enabled;\nthe bootstrap_srv axum integration did not, since it serves the relay\nthrough its own axum routes rather than iroh's RelayService.\n\nAdd a tower_http::set_header::SetResponseHeaderLayer for each header,\ngated on the server's rustls config, so plain-HTTP listeners are\nunaffected. Header values are copied verbatim from iroh-relay 1.0.0's\nTLS_HEADERS constant to stay in sync.\n\nThe layer is applied last, after every route (including the relay\nroutes merged in from iroh_relay_axum) has been added to the router,\nsince axum's Router::layer only wraps routes that already exist at the\ntime it's called — applying it earlier would have silently excluded\n/relay, /ping, /generate_204, and /relay/register from the headers.\n\nCloses #503",
          "timestamp": "2026-07-02T09:49:11+01:00",
          "tree_id": "56d15a12d9746c5a3165ec66ad639922eded88e2",
          "url": "https://github.com/holochain/kitsune2/commit/768b01b11dbe50c15fb36f554bbb77dcf7a5a351"
        },
        "date": 1782982456884,
        "tool": "cargo",
        "benches": [
          {
            "name": "local_relay/throughput/payload/1KiB",
            "value": 71425,
            "range": "± 4189",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/8KiB",
            "value": 75978,
            "range": "± 4150",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/32KiB",
            "value": 95746,
            "range": "± 3873",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/roundtrip/1KiB/localhost",
            "value": 41999836,
            "range": "± 25492",
            "unit": "ns/iter"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "synchwire@users.noreply.github.com",
            "name": "synchwire",
            "username": "synchwire"
          },
          "committer": {
            "email": "ThetaSinner@users.noreply.github.com",
            "name": "ThetaSinner",
            "username": "ThetaSinner"
          },
          "distinct": true,
          "id": "03d21103cb8df66d7573dfe22f2636558b7cb0cf",
          "message": "feat(bootstrap_srv): negotiate relay protocol version, enabling V2\n\nThe embedded relay handler previously pinned the iroh relay protocol\nto V1 at every point (subprotocol echo, access-control request, and\nclient conn config). Now the newest version shared with the client is\nnegotiated from the Sec-Websocket-Protocol header and threaded through,\nso iroh 0.98+ clients get V2 while V1-only clients keep working.",
          "timestamp": "2026-07-02T15:51:12+01:00",
          "tree_id": "c0f02a7ea45610a9911203128762c9e633fc02a2",
          "url": "https://github.com/holochain/kitsune2/commit/03d21103cb8df66d7573dfe22f2636558b7cb0cf"
        },
        "date": 1783004108646,
        "tool": "cargo",
        "benches": [
          {
            "name": "local_relay/throughput/payload/1KiB",
            "value": 73147,
            "range": "± 5182",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/8KiB",
            "value": 75683,
            "range": "± 3252",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/32KiB",
            "value": 93602,
            "range": "± 4263",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/roundtrip/1KiB/localhost",
            "value": 41997513,
            "range": "± 55340",
            "unit": "ns/iter"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "hra@holochain.org",
            "name": "holochain-release-automation2"
          },
          "committer": {
            "email": "ThetaSinner@users.noreply.github.com",
            "name": "ThetaSinner",
            "username": "ThetaSinner"
          },
          "distinct": true,
          "id": "e4ee0ba609a720db571316ef7f1eb75fcca47f79",
          "message": "chore: update CODEOWNERS with shared content",
          "timestamp": "2026-07-15T12:37:41+01:00",
          "tree_id": "3970c588b2e0c7f1c2417d818388330a98492c31",
          "url": "https://github.com/holochain/kitsune2/commit/e4ee0ba609a720db571316ef7f1eb75fcca47f79"
        },
        "date": 1784116068218,
        "tool": "cargo",
        "benches": [
          {
            "name": "local_relay/throughput/payload/1KiB",
            "value": 83155,
            "range": "± 6048",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/8KiB",
            "value": 85949,
            "range": "± 8254",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/32KiB",
            "value": 104913,
            "range": "± 7170",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/roundtrip/1KiB/localhost",
            "value": 41999713,
            "range": "± 32698",
            "unit": "ns/iter"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "ThetaSinner@users.noreply.github.com",
            "name": "ThetaSinner",
            "username": "ThetaSinner"
          },
          "committer": {
            "email": "ThetaSinner@users.noreply.github.com",
            "name": "ThetaSinner",
            "username": "ThetaSinner"
          },
          "distinct": true,
          "id": "9021cfe5a3064d3a924d61c444559fd5ae853777",
          "message": "ci: Add more package ecosystems to dependabot.yml",
          "timestamp": "2026-07-15T13:48:09+01:00",
          "tree_id": "f2ef70ed70e889bcc7f866d02156e6870dc39171",
          "url": "https://github.com/holochain/kitsune2/commit/9021cfe5a3064d3a924d61c444559fd5ae853777"
        },
        "date": 1784119885717,
        "tool": "cargo",
        "benches": [
          {
            "name": "local_relay/throughput/payload/1KiB",
            "value": 81942,
            "range": "± 5225",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/8KiB",
            "value": 81670,
            "range": "± 3879",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/32KiB",
            "value": 100413,
            "range": "± 6378",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/roundtrip/1KiB/localhost",
            "value": 41997414,
            "range": "± 36203",
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
          "id": "9dda957592552d0e94f5d396f092961f554b629d",
          "message": "build(deps): bump rust-overlay from `25d75be` to `a788763`\n\nBumps [rust-overlay](https://github.com/oxalica/rust-overlay) from `25d75be` to `a788763`.\n- [Commits](https://github.com/oxalica/rust-overlay/compare/25d75be8139815a53560745fa060909777495105...a7887636a3959168bd1ba7ef24a8a70b168dcb56)\n\n---\nupdated-dependencies:\n- dependency-name: rust-overlay\n  dependency-version: a7887636a3959168bd1ba7ef24a8a70b168dcb56\n  dependency-type: direct:production\n...\n\nSigned-off-by: dependabot[bot] <support@github.com>",
          "timestamp": "2026-07-15T16:07:57+01:00",
          "tree_id": "6d127973f1e11912bcc3b0eea74203c6ff776900",
          "url": "https://github.com/holochain/kitsune2/commit/9dda957592552d0e94f5d396f092961f554b629d"
        },
        "date": 1784128994493,
        "tool": "cargo",
        "benches": [
          {
            "name": "local_relay/throughput/payload/1KiB",
            "value": 39287,
            "range": "± 2742",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/8KiB",
            "value": 42103,
            "range": "± 2584",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/32KiB",
            "value": 52980,
            "range": "± 2875",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/roundtrip/1KiB/localhost",
            "value": 41668364,
            "range": "± 178196",
            "unit": "ns/iter"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "28270981+jost-s@users.noreply.github.com",
            "name": "Jost",
            "username": "jost-s"
          },
          "committer": {
            "email": "28270981+jost-s@users.noreply.github.com",
            "name": "Jost",
            "username": "jost-s"
          },
          "distinct": true,
          "id": "9d1b3378a325244a4f77b3072dc8a5a54643fb0a",
          "message": "chore(transport-iroh): downgrade url conversion log level to trace",
          "timestamp": "2026-07-15T10:06:49-06:00",
          "tree_id": "5bd3c388f9a357e1e04a062b0e56dfd8db4d4777",
          "url": "https://github.com/holochain/kitsune2/commit/9d1b3378a325244a4f77b3072dc8a5a54643fb0a"
        },
        "date": 1784132149538,
        "tool": "cargo",
        "benches": [
          {
            "name": "local_relay/throughput/payload/1KiB",
            "value": 47072,
            "range": "± 2330",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/8KiB",
            "value": 50393,
            "range": "± 1879",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/32KiB",
            "value": 63913,
            "range": "± 1657",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/roundtrip/1KiB/localhost",
            "value": 41850530,
            "range": "± 240122",
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
          "id": "62cf3446e829eaed7f3948ed29eff6dfb712413c",
          "message": "build(deps): bump actions/setup-go from 6 to 7\n\nBumps [actions/setup-go](https://github.com/actions/setup-go) from 6 to 7.\n- [Release notes](https://github.com/actions/setup-go/releases)\n- [Commits](https://github.com/actions/setup-go/compare/v6...v7)\n\n---\nupdated-dependencies:\n- dependency-name: actions/setup-go\n  dependency-version: '7'\n  dependency-type: direct:production\n  update-type: version-update:semver-major\n...\n\nSigned-off-by: dependabot[bot] <support@github.com>",
          "timestamp": "2026-07-17T18:06:56+01:00",
          "tree_id": "fe163485312d3c01823db95aeb55bac6ef75e704",
          "url": "https://github.com/holochain/kitsune2/commit/62cf3446e829eaed7f3948ed29eff6dfb712413c"
        },
        "date": 1784308617681,
        "tool": "cargo",
        "benches": [
          {
            "name": "local_relay/throughput/payload/1KiB",
            "value": 79509,
            "range": "± 5022",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/8KiB",
            "value": 82510,
            "range": "± 4612",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/32KiB",
            "value": 100970,
            "range": "± 6454",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/roundtrip/1KiB/localhost",
            "value": 42001402,
            "range": "± 38468",
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
          "id": "766245b51a9e229f22e97c5cf6d887265ba76936",
          "message": "build(deps): bump bytes from 1.11.1 to 1.12.1\n\nBumps [bytes](https://github.com/tokio-rs/bytes) from 1.11.1 to 1.12.1.\n- [Release notes](https://github.com/tokio-rs/bytes/releases)\n- [Changelog](https://github.com/tokio-rs/bytes/blob/master/CHANGELOG.md)\n- [Commits](https://github.com/tokio-rs/bytes/compare/v1.11.1...v1.12.1)\n\n---\nupdated-dependencies:\n- dependency-name: bytes\n  dependency-version: 1.12.1\n  dependency-type: direct:production\n  update-type: version-update:semver-minor\n...\n\nSigned-off-by: dependabot[bot] <support@github.com>",
          "timestamp": "2026-07-20T19:03:49+01:00",
          "tree_id": "c36f64581a31fc974cec2b17f45a37d08e27f40e",
          "url": "https://github.com/holochain/kitsune2/commit/766245b51a9e229f22e97c5cf6d887265ba76936"
        },
        "date": 1784571290215,
        "tool": "cargo",
        "benches": [
          {
            "name": "local_relay/throughput/payload/1KiB",
            "value": 78022,
            "range": "± 4397",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/8KiB",
            "value": 80952,
            "range": "± 5528",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/32KiB",
            "value": 95208,
            "range": "± 4582",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/roundtrip/1KiB/localhost",
            "value": 41864891,
            "range": "± 107961",
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
          "id": "6739dcca78c910542f8252a99f9d9ee148e442b8",
          "message": "build(deps): bump holochain/actions/.github/workflows/changelog-preview-comment.yml\n\nBumps [holochain/actions/.github/workflows/changelog-preview-comment.yml](https://github.com/holochain/actions) from 1.14.0 to 1.16.0.\n- [Release notes](https://github.com/holochain/actions/releases)\n- [Commits](https://github.com/holochain/actions/compare/v1.14.0...v1.16.0)\n\n---\nupdated-dependencies:\n- dependency-name: holochain/actions/.github/workflows/changelog-preview-comment.yml\n  dependency-version: 1.16.0\n  dependency-type: direct:production\n  update-type: version-update:semver-minor\n...\n\nSigned-off-by: dependabot[bot] <support@github.com>",
          "timestamp": "2026-07-20T19:18:01+01:00",
          "tree_id": "cc8ff847316fc421709921a5edda351bd179739d",
          "url": "https://github.com/holochain/kitsune2/commit/6739dcca78c910542f8252a99f9d9ee148e442b8"
        },
        "date": 1784571723642,
        "tool": "cargo",
        "benches": [
          {
            "name": "local_relay/throughput/payload/1KiB",
            "value": 59297,
            "range": "± 2556",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/8KiB",
            "value": 63702,
            "range": "± 2907",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/32KiB",
            "value": 80299,
            "range": "± 1884",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/roundtrip/1KiB/localhost",
            "value": 42000750,
            "range": "± 3844",
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
          "id": "9a918fdaf6c50157585ba36ec7a7647b600fd1ac",
          "message": "build(deps): bump holochain/actions/.github/workflows/prepare-release.yml\n\nBumps [holochain/actions/.github/workflows/prepare-release.yml](https://github.com/holochain/actions) from 1.14.0 to 1.16.0.\n- [Release notes](https://github.com/holochain/actions/releases)\n- [Commits](https://github.com/holochain/actions/compare/v1.14.0...v1.16.0)\n\n---\nupdated-dependencies:\n- dependency-name: holochain/actions/.github/workflows/prepare-release.yml\n  dependency-version: 1.16.0\n  dependency-type: direct:production\n  update-type: version-update:semver-minor\n...\n\nSigned-off-by: dependabot[bot] <support@github.com>",
          "timestamp": "2026-07-20T19:18:59+01:00",
          "tree_id": "5d5ab2190b22448459ba8270cf506225607e3948",
          "url": "https://github.com/holochain/kitsune2/commit/9a918fdaf6c50157585ba36ec7a7647b600fd1ac"
        },
        "date": 1784571899609,
        "tool": "cargo",
        "benches": [
          {
            "name": "local_relay/throughput/payload/1KiB",
            "value": 57453,
            "range": "± 2515",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/8KiB",
            "value": 62795,
            "range": "± 2784",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/32KiB",
            "value": 82457,
            "range": "± 2248",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/roundtrip/1KiB/localhost",
            "value": 42000432,
            "range": "± 28941",
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
          "id": "6ebe76f180486760391c9bc27f47d4d67d497b73",
          "message": "build(deps): bump iroh-base from 1.0.0 to 1.0.2\n\nBumps [iroh-base](https://github.com/n0-computer/iroh) from 1.0.0 to 1.0.2.\n- [Release notes](https://github.com/n0-computer/iroh/releases)\n- [Changelog](https://github.com/n0-computer/iroh/blob/main/CHANGELOG.md)\n- [Commits](https://github.com/n0-computer/iroh/compare/v1.0.0...v1.0.2)\n\n---\nupdated-dependencies:\n- dependency-name: iroh-base\n  dependency-version: 1.0.2\n  dependency-type: direct:production\n  update-type: version-update:semver-patch\n...\n\nSigned-off-by: dependabot[bot] <support@github.com>",
          "timestamp": "2026-07-21T01:05:36+01:00",
          "tree_id": "f455ad9d356c120223755f521948394234aabd6a",
          "url": "https://github.com/holochain/kitsune2/commit/6ebe76f180486760391c9bc27f47d4d67d497b73"
        },
        "date": 1784593046980,
        "tool": "cargo",
        "benches": [
          {
            "name": "local_relay/throughput/payload/1KiB",
            "value": 51833,
            "range": "± 2032",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/8KiB",
            "value": 54429,
            "range": "± 2212",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/32KiB",
            "value": 68383,
            "range": "± 3655",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/roundtrip/1KiB/localhost",
            "value": 41994517,
            "range": "± 23639",
            "unit": "ns/iter"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "ThetaSinner@users.noreply.github.com",
            "name": "ThetaSinner",
            "username": "ThetaSinner"
          },
          "committer": {
            "email": "ThetaSinner@users.noreply.github.com",
            "name": "ThetaSinner",
            "username": "ThetaSinner"
          },
          "distinct": true,
          "id": "9bc792024b8143aa77940b44ff12cca40179107f",
          "message": "chore: Update Nix packages to 26.05",
          "timestamp": "2026-07-22T12:21:44+01:00",
          "tree_id": "c1d7781d93b9afa02b25318a90435f3a3cba3ca7",
          "url": "https://github.com/holochain/kitsune2/commit/9bc792024b8143aa77940b44ff12cca40179107f"
        },
        "date": 1784719901570,
        "tool": "cargo",
        "benches": [
          {
            "name": "local_relay/throughput/payload/1KiB",
            "value": 75806,
            "range": "± 3441",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/8KiB",
            "value": 78225,
            "range": "± 3291",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/32KiB",
            "value": 107033,
            "range": "± 4194",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/roundtrip/1KiB/localhost",
            "value": 41879455,
            "range": "± 89749",
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
          "id": "6aeabbd146bbde079f2c1e3bfe9e0c0da609950a",
          "message": "chore: tell Claude to always use `expect(\"poison\")` for mutex poison handling",
          "timestamp": "2026-07-27T14:13:00+02:00",
          "tree_id": "2cabdebdc394f9722b27ac722475e55af901288b",
          "url": "https://github.com/holochain/kitsune2/commit/6aeabbd146bbde079f2c1e3bfe9e0c0da609950a"
        },
        "date": 1785154969919,
        "tool": "cargo",
        "benches": [
          {
            "name": "local_relay/throughput/payload/1KiB",
            "value": 74129,
            "range": "± 3843",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/8KiB",
            "value": 76326,
            "range": "± 3776",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/32KiB",
            "value": 91170,
            "range": "± 2864",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/roundtrip/1KiB/localhost",
            "value": 41772165,
            "range": "± 130653",
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
          "id": "c3870c5dba9de219ff8304cb8aaa8dc5fbbac2df",
          "message": "feat(transport_iroh): close connections gracefully with close codes\n\nAn intentional disconnect previously closed the QUIC connection with a\nhardcoded code 0 and dropped the caller's reason, so the remote peer\ntreated it as a network failure and marked the closer unresponsive\nuntil its agent info expired.\n\nIntroduce a CloseCode enum (Unspecified, Graceful, Superseded) carried\nas the QUIC application close code. Transport::disconnect now sends the\ncaller's reason in the close frame with the Graceful code, and the\nremote reader releases the connection quietly, informing handlers via\npeer_disconnect with the reason instead of marking the peer\nunresponsive. Supersession is signalled by code as well, keeping the\nlegacy reason-string match for peers running older code.\n\nCloses #496",
          "timestamp": "2026-07-27T15:49:01+02:00",
          "tree_id": "9612c007f2d87d7e41c22379adfe5d3c91e84394",
          "url": "https://github.com/holochain/kitsune2/commit/c3870c5dba9de219ff8304cb8aaa8dc5fbbac2df"
        },
        "date": 1785160336233,
        "tool": "cargo",
        "benches": [
          {
            "name": "local_relay/throughput/payload/1KiB",
            "value": 73447,
            "range": "± 3381",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/8KiB",
            "value": 75935,
            "range": "± 3681",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/32KiB",
            "value": 92055,
            "range": "± 3700",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/roundtrip/1KiB/localhost",
            "value": 41879698,
            "range": "± 124395",
            "unit": "ns/iter"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "ThetaSinner@users.noreply.github.com",
            "name": "ThetaSinner",
            "username": "ThetaSinner"
          },
          "committer": {
            "email": "ThetaSinner@users.noreply.github.com",
            "name": "ThetaSinner",
            "username": "ThetaSinner"
          },
          "distinct": true,
          "id": "71685f6cdc264756b591ff2883cd0f579e23d9b6",
          "message": "fixup! ci: add static-fmt cargo-make task for use as a fast CI gate",
          "timestamp": "2026-07-28T11:44:17+01:00",
          "tree_id": "17431c5451988b12663eef5c693bfe65bb1051b9",
          "url": "https://github.com/holochain/kitsune2/commit/71685f6cdc264756b591ff2883cd0f579e23d9b6"
        },
        "date": 1785235651695,
        "tool": "cargo",
        "benches": [
          {
            "name": "local_relay/throughput/payload/1KiB",
            "value": 57801,
            "range": "± 2359",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/8KiB",
            "value": 63186,
            "range": "± 2078",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/32KiB",
            "value": 87702,
            "range": "± 2412",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/roundtrip/1KiB/localhost",
            "value": 41999440,
            "range": "± 14725",
            "unit": "ns/iter"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "6267702+ThetaSinner@users.noreply.github.com",
            "name": "ThetaSinner",
            "username": "ThetaSinner"
          },
          "committer": {
            "email": "ThetaSinner@users.noreply.github.com",
            "name": "ThetaSinner",
            "username": "ThetaSinner"
          },
          "distinct": true,
          "id": "585151a32f4937c210873ced5aac94503b908d9d",
          "message": "chore: Prepare next release",
          "timestamp": "2026-07-28T12:04:14+01:00",
          "tree_id": "1383bb5ce425a54b2634b3a4e354ca301700d1a8",
          "url": "https://github.com/holochain/kitsune2/commit/585151a32f4937c210873ced5aac94503b908d9d"
        },
        "date": 1785236860078,
        "tool": "cargo",
        "benches": [
          {
            "name": "local_relay/throughput/payload/1KiB",
            "value": 77407,
            "range": "± 4037",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/8KiB",
            "value": 78296,
            "range": "± 3597",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/32KiB",
            "value": 99508,
            "range": "± 3852",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/roundtrip/1KiB/localhost",
            "value": 41997807,
            "range": "± 50314",
            "unit": "ns/iter"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "ThetaSinner@users.noreply.github.com",
            "name": "ThetaSinner",
            "username": "ThetaSinner"
          },
          "committer": {
            "email": "ThetaSinner@users.noreply.github.com",
            "name": "ThetaSinner",
            "username": "ThetaSinner"
          },
          "distinct": true,
          "id": "e85e4a3df044e8ee861ac74541298fc23d1c9e4f",
          "message": "fix: clippy issues raised by upgraded clippy",
          "timestamp": "2026-07-28T12:51:34+01:00",
          "tree_id": "977ec93bdb37e1990f754e32e5100157416c4ffd",
          "url": "https://github.com/holochain/kitsune2/commit/e85e4a3df044e8ee861ac74541298fc23d1c9e4f"
        },
        "date": 1785240074373,
        "tool": "cargo",
        "benches": [
          {
            "name": "local_relay/throughput/payload/1KiB",
            "value": 57751,
            "range": "± 2853",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/8KiB",
            "value": 64017,
            "range": "± 2658",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/32KiB",
            "value": 82076,
            "range": "± 1501",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/roundtrip/1KiB/localhost",
            "value": 41999909,
            "range": "± 1671",
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
          "id": "c461de70c85a968a8ae2588ff733899aead57b6b",
          "message": "build(deps): bump the background group across 1 directory with 3 updates\n\nBumps the background group with 3 updates in the / directory: [crane](https://github.com/ipetkov/crane), [nixpkgs](https://github.com/nixos/nixpkgs) and [rust-overlay](https://github.com/oxalica/rust-overlay).\n\n\nUpdates `crane` from `f7d151e` to `7930f6c`\n- [Release notes](https://github.com/ipetkov/crane/releases)\n- [Commits](https://github.com/ipetkov/crane/compare/f7d151ec0bf52cf9662e2f59d7bea28588c2f070...7930f6c291de6f83c257839d434592aa085f290a)\n\nUpdates `nixpkgs` from `4382ed2` to `8623c4c`\n- [Commits](https://github.com/nixos/nixpkgs/compare/4382ed2b7a6839d4280a9b386db49cbc5907414d...8623c4c20aa4ca2f5fb81510d2944066c3fb0d96)\n\nUpdates `rust-overlay` from `0681750` to `8ec8a5a`\n- [Commits](https://github.com/oxalica/rust-overlay/compare/068175006cfb69d5b541a140ed93e361488c9e53...8ec8a5a41f8d8244e672829c9cd705416139d3f0)\n\n---\nupdated-dependencies:\n- dependency-name: crane\n  dependency-version: 7930f6c291de6f83c257839d434592aa085f290a\n  dependency-type: direct:production\n  dependency-group: background\n- dependency-name: nixpkgs\n  dependency-version: fd1462031fdee08f65fd0b4c6b64e22239a77870\n  dependency-type: direct:production\n  dependency-group: background\n- dependency-name: rust-overlay\n  dependency-version: 47759faaddf38fadaf172151ca9df8adae9c0b2e\n  dependency-type: direct:production\n  dependency-group: background\n...\n\nSigned-off-by: dependabot[bot] <support@github.com>",
          "timestamp": "2026-07-28T13:19:57+01:00",
          "tree_id": "d9da0a0abe62c13d38a41a632409ff7b6368da37",
          "url": "https://github.com/holochain/kitsune2/commit/c461de70c85a968a8ae2588ff733899aead57b6b"
        },
        "date": 1785241427185,
        "tool": "cargo",
        "benches": [
          {
            "name": "local_relay/throughput/payload/1KiB",
            "value": 80012,
            "range": "± 3172",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/8KiB",
            "value": 83497,
            "range": "± 3913",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/32KiB",
            "value": 98325,
            "range": "± 3949",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/roundtrip/1KiB/localhost",
            "value": 41967103,
            "range": "± 53780",
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
          "id": "39d970bdb699c45b0b32ad2a7b8261973d29f373",
          "message": "build(deps): bump the background group across 1 directory with 12 updates\n\nBumps the background group with 12 updates in the / directory:\n\n| Package | From | To |\n| --- | --- | --- |\n| [http](https://github.com/hyperium/http) | `1.4.0` | `1.4.2` |\n| [chrono](https://github.com/chronotope/chrono) | `0.4.44` | `0.4.45` |\n| [clap](https://github.com/clap-rs/clap) | `4.6.1` | `4.6.4` |\n| [prost](https://github.com/tokio-rs/prost) | `0.14.3` | `0.14.4` |\n| [serde](https://github.com/serde-rs/serde) | `1.0.228` | `1.0.229` |\n| [serde_json](https://github.com/serde-rs/json) | `1.0.149` | `1.0.151` |\n| [tokio](https://github.com/tokio-rs/tokio) | `1.52.3` | `1.53.1` |\n| [thiserror](https://github.com/dtolnay/thiserror) | `2.0.18` | `2.0.19` |\n| [rustls](https://github.com/rustls/rustls) | `0.23.40` | `0.23.42` |\n| [rustyline](https://github.com/kkawakam/rustyline) | `18.0.0` | `18.0.1` |\n| [prost-build](https://github.com/tokio-rs/prost) | `0.14.3` | `0.14.4` |\n| [iroh-base](https://github.com/n0-computer/iroh) | `1.0.2` | `1.0.3` |\n\n\n\nUpdates `http` from 1.4.0 to 1.4.2\n- [Release notes](https://github.com/hyperium/http/releases)\n- [Changelog](https://github.com/hyperium/http/blob/master/CHANGELOG.md)\n- [Commits](https://github.com/hyperium/http/compare/v1.4.0...v1.4.2)\n\nUpdates `chrono` from 0.4.44 to 0.4.45\n- [Release notes](https://github.com/chronotope/chrono/releases)\n- [Changelog](https://github.com/chronotope/chrono/blob/main/CHANGELOG.md)\n- [Commits](https://github.com/chronotope/chrono/compare/v0.4.44...v0.4.45)\n\nUpdates `clap` from 4.6.1 to 4.6.4\n- [Release notes](https://github.com/clap-rs/clap/releases)\n- [Changelog](https://github.com/clap-rs/clap/blob/master/CHANGELOG.md)\n- [Commits](https://github.com/clap-rs/clap/compare/clap_complete-v4.6.1...clap_complete-v4.6.4)\n\nUpdates `prost` from 0.14.3 to 0.14.4\n- [Release notes](https://github.com/tokio-rs/prost/releases)\n- [Changelog](https://github.com/tokio-rs/prost/blob/master/CHANGELOG.md)\n- [Commits](https://github.com/tokio-rs/prost/compare/v0.14.3...v0.14.4)\n\nUpdates `serde` from 1.0.228 to 1.0.229\n- [Release notes](https://github.com/serde-rs/serde/releases)\n- [Commits](https://github.com/serde-rs/serde/compare/v1.0.228...v1.0.229)\n\nUpdates `serde_json` from 1.0.149 to 1.0.151\n- [Release notes](https://github.com/serde-rs/json/releases)\n- [Commits](https://github.com/serde-rs/json/compare/v1.0.149...v1.0.151)\n\nUpdates `tokio` from 1.52.3 to 1.53.1\n- [Release notes](https://github.com/tokio-rs/tokio/releases)\n- [Commits](https://github.com/tokio-rs/tokio/compare/tokio-1.52.3...tokio-1.53.1)\n\nUpdates `thiserror` from 2.0.18 to 2.0.19\n- [Release notes](https://github.com/dtolnay/thiserror/releases)\n- [Commits](https://github.com/dtolnay/thiserror/compare/2.0.18...2.0.19)\n\nUpdates `rustls` from 0.23.40 to 0.23.42\n- [Release notes](https://github.com/rustls/rustls/releases)\n- [Changelog](https://github.com/rustls/rustls/blob/main/CHANGELOG.md)\n- [Commits](https://github.com/rustls/rustls/compare/v/0.23.40...v/0.23.42)\n\nUpdates `rustyline` from 18.0.0 to 18.0.1\n- [Release notes](https://github.com/kkawakam/rustyline/releases)\n- [Changelog](https://github.com/kkawakam/rustyline/blob/master/History.md)\n- [Commits](https://github.com/kkawakam/rustyline/compare/v18.0.0...v18.0.1)\n\nUpdates `prost-build` from 0.14.3 to 0.14.4\n- [Release notes](https://github.com/tokio-rs/prost/releases)\n- [Changelog](https://github.com/tokio-rs/prost/blob/master/CHANGELOG.md)\n- [Commits](https://github.com/tokio-rs/prost/compare/v0.14.3...v0.14.4)\n\nUpdates `iroh-base` from 1.0.2 to 1.0.3\n- [Release notes](https://github.com/n0-computer/iroh/releases)\n- [Changelog](https://github.com/n0-computer/iroh/blob/main/CHANGELOG.md)\n- [Commits](https://github.com/n0-computer/iroh/compare/v1.0.2...v1.0.3)\n\n---\nupdated-dependencies:\n- dependency-name: chrono\n  dependency-version: 0.4.45\n  dependency-type: direct:production\n  update-type: version-update:semver-patch\n  dependency-group: background\n- dependency-name: clap\n  dependency-version: 4.6.4\n  dependency-type: direct:production\n  update-type: version-update:semver-patch\n  dependency-group: background\n- dependency-name: http\n  dependency-version: 1.4.2\n  dependency-type: direct:production\n  update-type: version-update:semver-patch\n  dependency-group: background\n- dependency-name: iroh-base\n  dependency-version: 1.0.3\n  dependency-type: direct:production\n  update-type: version-update:semver-patch\n  dependency-group: background\n- dependency-name: prost\n  dependency-version: 0.14.4\n  dependency-type: direct:production\n  update-type: version-update:semver-patch\n  dependency-group: background\n- dependency-name: prost-build\n  dependency-version: 0.14.4\n  dependency-type: direct:production\n  update-type: version-update:semver-patch\n  dependency-group: background\n- dependency-name: rustls\n  dependency-version: 0.23.42\n  dependency-type: direct:production\n  update-type: version-update:semver-patch\n  dependency-group: background\n- dependency-name: rustyline\n  dependency-version: 18.0.1\n  dependency-type: direct:production\n  update-type: version-update:semver-patch\n  dependency-group: background\n- dependency-name: serde\n  dependency-version: 1.0.229\n  dependency-type: direct:production\n  update-type: version-update:semver-patch\n  dependency-group: background\n- dependency-name: serde_json\n  dependency-version: 1.0.151\n  dependency-type: direct:production\n  update-type: version-update:semver-patch\n  dependency-group: background\n- dependency-name: thiserror\n  dependency-version: 2.0.19\n  dependency-type: direct:production\n  update-type: version-update:semver-patch\n  dependency-group: background\n- dependency-name: tokio\n  dependency-version: 1.53.1\n  dependency-type: direct:production\n  update-type: version-update:semver-minor\n  dependency-group: background\n...\n\nSigned-off-by: dependabot[bot] <support@github.com>",
          "timestamp": "2026-07-28T13:49:17+01:00",
          "tree_id": "c4b875975dc1863a9e8a28f357e988830843c4bf",
          "url": "https://github.com/holochain/kitsune2/commit/39d970bdb699c45b0b32ad2a7b8261973d29f373"
        },
        "date": 1785243391383,
        "tool": "cargo",
        "benches": [
          {
            "name": "local_relay/throughput/payload/1KiB",
            "value": 74871,
            "range": "± 4321",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/8KiB",
            "value": 76131,
            "range": "± 3194",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/32KiB",
            "value": 99860,
            "range": "± 3866",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/roundtrip/1KiB/localhost",
            "value": 41892667,
            "range": "± 90705",
            "unit": "ns/iter"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "ThetaSinner@users.noreply.github.com",
            "name": "ThetaSinner",
            "username": "ThetaSinner"
          },
          "committer": {
            "email": "ThetaSinner@users.noreply.github.com",
            "name": "ThetaSinner",
            "username": "ThetaSinner"
          },
          "distinct": true,
          "id": "659638b4e020ca1735b25d67f2dcbbdd1da71270",
          "message": "fix: Update `ed25519-dalek` to be compatible with the `rand` upgrade and update the code to use the new `rand` API\n\n# Conflicts:\n#\tCargo.lock",
          "timestamp": "2026-07-28T14:45:01+01:00",
          "tree_id": "5abe728ca74da10ef3f37af3bbef03772a13aa5b",
          "url": "https://github.com/holochain/kitsune2/commit/659638b4e020ca1735b25d67f2dcbbdd1da71270"
        },
        "date": 1785246880637,
        "tool": "cargo",
        "benches": [
          {
            "name": "local_relay/throughput/payload/1KiB",
            "value": 54046,
            "range": "± 3138",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/8KiB",
            "value": 62085,
            "range": "± 2575",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/32KiB",
            "value": 85700,
            "range": "± 1953",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/roundtrip/1KiB/localhost",
            "value": 41999526,
            "range": "± 4307",
            "unit": "ns/iter"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "ThetaSinner@users.noreply.github.com",
            "name": "ThetaSinner",
            "username": "ThetaSinner"
          },
          "committer": {
            "email": "ThetaSinner@users.noreply.github.com",
            "name": "ThetaSinner",
            "username": "ThetaSinner"
          },
          "distinct": true,
          "id": "c34999247f737d7743207c45fbe97c00a7d1b7df",
          "message": "build(deps): Update `opentelemetry_sdk` and `opentelemetry-otlp` to 0.32",
          "timestamp": "2026-07-28T15:19:52+01:00",
          "tree_id": "7da2bfb709acb62fee7ca2c358bbd4698aa16f4d",
          "url": "https://github.com/holochain/kitsune2/commit/c34999247f737d7743207c45fbe97c00a7d1b7df"
        },
        "date": 1785248934772,
        "tool": "cargo",
        "benches": [
          {
            "name": "local_relay/throughput/payload/1KiB",
            "value": 75213,
            "range": "± 4830",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/8KiB",
            "value": 76936,
            "range": "± 3340",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/32KiB",
            "value": 92318,
            "range": "± 4063",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/roundtrip/1KiB/localhost",
            "value": 41953000,
            "range": "± 102759",
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
          "id": "833671627488559f42c3c897ace70826b7427914",
          "message": "build(deps): bump tower-http from 0.6.10 to 0.7.0\n\nBumps [tower-http](https://github.com/tower-rs/tower-http) from 0.6.10 to 0.7.0.\n- [Release notes](https://github.com/tower-rs/tower-http/releases)\n- [Commits](https://github.com/tower-rs/tower-http/compare/tower-http-0.6.10...tower-http-0.7.0)\n\n---\nupdated-dependencies:\n- dependency-name: tower-http\n  dependency-version: 0.7.0\n  dependency-type: direct:production\n  update-type: version-update:semver-minor\n...\n\nSigned-off-by: dependabot[bot] <support@github.com>",
          "timestamp": "2026-07-28T15:44:38+01:00",
          "tree_id": "9d332f15026de39fd2020f7f5d50ebfe9ebe4135",
          "url": "https://github.com/holochain/kitsune2/commit/833671627488559f42c3c897ace70826b7427914"
        },
        "date": 1785250146724,
        "tool": "cargo",
        "benches": [
          {
            "name": "local_relay/throughput/payload/1KiB",
            "value": 49712,
            "range": "± 1783",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/8KiB",
            "value": 52368,
            "range": "± 2046",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/32KiB",
            "value": 64221,
            "range": "± 1355",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/roundtrip/1KiB/localhost",
            "value": 41318076,
            "range": "± 167805",
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
          "id": "b9e23e57e82212ef038305dda42983eb3e4c0759",
          "message": "build(deps): bump schemars from 0.9.0 to 1.2.2\n\nBumps [schemars](https://github.com/GREsau/schemars) from 0.9.0 to 1.2.2.\n- [Release notes](https://github.com/GREsau/schemars/releases)\n- [Changelog](https://github.com/GREsau/schemars/blob/master/CHANGELOG.md)\n- [Commits](https://github.com/GREsau/schemars/compare/v0.9.0...v1.2.2)\n\n---\nupdated-dependencies:\n- dependency-name: schemars\n  dependency-version: 1.2.1\n  dependency-type: direct:production\n  update-type: version-update:semver-major\n...\n\nSigned-off-by: dependabot[bot] <support@github.com>",
          "timestamp": "2026-07-28T16:11:01+01:00",
          "tree_id": "c065db81217d6b85dfe888285abcabfd278fb8be",
          "url": "https://github.com/holochain/kitsune2/commit/b9e23e57e82212ef038305dda42983eb3e4c0759"
        },
        "date": 1785252086339,
        "tool": "cargo",
        "benches": [
          {
            "name": "local_relay/throughput/payload/1KiB",
            "value": 81054,
            "range": "± 4228",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/8KiB",
            "value": 82968,
            "range": "± 4467",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/32KiB",
            "value": 96982,
            "range": "± 5995",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/roundtrip/1KiB/localhost",
            "value": 42000672,
            "range": "± 5917",
            "unit": "ns/iter"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "6267702+ThetaSinner@users.noreply.github.com",
            "name": "ThetaSinner",
            "username": "ThetaSinner"
          },
          "committer": {
            "email": "ThetaSinner@users.noreply.github.com",
            "name": "ThetaSinner",
            "username": "ThetaSinner"
          },
          "distinct": true,
          "id": "2e496b7b4282482b68632bb05f4d3874789e0b2b",
          "message": "chore: Prepare next release",
          "timestamp": "2026-07-28T16:56:22+01:00",
          "tree_id": "3146ba493c5260333d55b146240da9e1e1734b09",
          "url": "https://github.com/holochain/kitsune2/commit/2e496b7b4282482b68632bb05f4d3874789e0b2b"
        },
        "date": 1785254706012,
        "tool": "cargo",
        "benches": [
          {
            "name": "local_relay/throughput/payload/1KiB",
            "value": 55015,
            "range": "± 2638",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/8KiB",
            "value": 64414,
            "range": "± 3119",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/throughput/payload/32KiB",
            "value": 88823,
            "range": "± 2209",
            "unit": "ns/iter"
          },
          {
            "name": "local_relay/roundtrip/1KiB/localhost",
            "value": 42000199,
            "range": "± 27367",
            "unit": "ns/iter"
          }
        ]
      }
    ]
  }
}