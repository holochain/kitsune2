window.BENCHMARK_DATA = {
  "lastUpdate": 1781796061780,
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
      }
    ]
  }
}