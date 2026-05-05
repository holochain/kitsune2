//! Criterion benchmarks for relay message delivery.
//!
//! Benchmarks use the `iroh_relay::client::ClientBuilder` API directly
//! (not `iroh::Endpoint`) to measure raw relay throughput without the
//! overhead of QUIC negotiation, holepunching, or QAD.
//!
//! **Local relay** benchmarks always run against an in-process axum relay
//! server — no network access required.
//!
//! **External relay** benchmarks are gated behind the
//! `KITSUNE2_IROH_RELAY_BENCH_URLS` environment variable (comma-separated
//! list of relay URLs). If unset, only local benchmarks run.
//!
//! # Examples
//!
//! ```sh
//! # Local only:
//! cargo bench -p kitsune2_bootstrap_srv
//!
//! # Compare against public and deployed relays:
//! KITSUNE2_IROH_RELAY_BENCH_URLS="https://use1-1.relay.n0.iroh-canary.iroh.link./,https://dev-test-bootstrap2-iroh.holochain.org" \
//!   cargo bench -p kitsune2_bootstrap_srv
//! ```

use std::{net::Ipv4Addr, time::Duration};

use axum::{Router, routing::get};
use criterion::{
    BenchmarkId, Criterion, Throughput, criterion_group, criterion_main,
};
use futures::{SinkExt, StreamExt};
use iroh_base::{RelayUrl, SecretKey};
use iroh_relay::{
    client::ClientBuilder,
    dns::DnsResolver,
    protos::relay::{ClientToRelayMsg, Datagrams, RelayToClientMsg},
};
use kitsune2_bootstrap_srv::iroh_relay_axum::{
    create_relay_state, relay_handler,
};
use tokio::{net::TcpListener, runtime::Runtime};

/// Spawn an in-process axum relay server on a dedicated thread.
fn spawn_local_relay() -> RelayUrl {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let rt = Runtime::new().unwrap();
        rt.block_on(async {
            let state = create_relay_state(None);
            let app = Router::new()
                .route("/relay", get(relay_handler))
                .with_state(state);

            let listener =
                TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
            let addr = listener.local_addr().unwrap();
            tx.send(addr).unwrap();
            axum::serve(listener, app).await.expect("server error");
        });
    });
    let addr = rx.recv().unwrap();
    format!("http://{}/relay", addr).parse().unwrap()
}

type ClientPair = (
    iroh_relay::client::Client,
    iroh_base::EndpointId,
    iroh_relay::client::Client,
    iroh_base::EndpointId,
);

/// Connect two relay clients to the given relay URL.
async fn connect_client_pair(relay_url: &RelayUrl) -> ClientPair {
    let resolver = DnsResolver::new();

    let a_secret = SecretKey::generate();
    let a_key = a_secret.public();
    let client_a =
        ClientBuilder::new(relay_url.clone(), a_secret, resolver.clone())
            .connect()
            .await
            .unwrap();

    let b_secret = SecretKey::generate();
    let b_key = b_secret.public();
    let client_b = ClientBuilder::new(relay_url.clone(), b_secret, resolver)
        .connect()
        .await
        .unwrap();

    (client_a, a_key, client_b, b_key)
}

fn local_relay_benchmarks(c: &mut Criterion) {
    let relay_url = spawn_local_relay();

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    // Payload throughput at various sizes. The relay protocol has a max
    // frame size of 64 KiB, so we stay below that limit. Fresh clients
    // are created per size to avoid connection exhaustion across many
    // Criterion iterations.
    let mut group = c.benchmark_group("local_relay/throughput");
    group.measurement_time(Duration::from_secs(15));
    group.sample_size(20);

    for (label, size) in [
        ("1KiB", 1024usize),
        ("8KiB", 8 * 1024),
        ("32KiB", 32 * 1024),
    ] {
        let (mut client_a, a_key, mut client_b, b_key) =
            rt.block_on(connect_client_pair(&relay_url));

        let payload = vec![42u8; size];
        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(
            BenchmarkId::new("payload", label),
            &payload,
            |b, payload| {
                b.iter(|| {
                    rt.block_on(async {
                        let msg = Datagrams::from(payload.as_slice());
                        client_a
                            .send(ClientToRelayMsg::Datagrams {
                                dst_endpoint_id: b_key,
                                datagrams: msg,
                            })
                            .await
                            .unwrap();

                        // Read until we get the datagram, skipping pings.
                        loop {
                            let received = tokio::time::timeout(
                                Duration::from_secs(5),
                                client_b.next(),
                            )
                            .await
                            .expect("timeout")
                            .expect("stream ended")
                            .unwrap();

                            match received {
                                RelayToClientMsg::Datagrams {
                                    remote_endpoint_id,
                                    ..
                                } => {
                                    assert_eq!(remote_endpoint_id, a_key);
                                    break;
                                }
                                RelayToClientMsg::Ping { .. } => continue,
                                other => {
                                    panic!(
                                        "unexpected message type: {:?}",
                                        other
                                    )
                                }
                            }
                        }
                    });
                });
            },
        );
    }

    group.finish();
}

/// Roundtrip benchmark for the local relay: connect + handshake + 1 KiB
/// send, same methodology as external_relay_benchmarks so numbers are
/// directly comparable.
fn local_relay_roundtrip(c: &mut Criterion) {
    let relay_url = spawn_local_relay();

    let mut group = c.benchmark_group("local_relay/roundtrip");
    // Each iteration creates fresh TCP connections that linger in
    // TIME_WAIT (~30s on macOS). Keep timing short and sample count
    // low to stay within ephemeral port limits.
    group.warm_up_time(Duration::from_millis(500));
    group.measurement_time(Duration::from_secs(5));
    group.sample_size(10);

    group.bench_function(BenchmarkId::new("1KiB", "localhost"), |b| {
        b.iter(|| {
            relay_roundtrip(&relay_url);
        });
    });

    group.finish();
}

/// Send a single 1 KiB datagram through a relay, including client
/// connect + handshake time. Each call creates fresh clients to avoid
/// hitting upstream rate limits on long-lived connections.
fn relay_roundtrip(relay_url: &RelayUrl) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    rt.block_on(async {
        let resolver = DnsResolver::new();

        let a_secret = SecretKey::generate();
        let a_key = a_secret.public();
        let mut client_a =
            ClientBuilder::new(relay_url.clone(), a_secret, resolver.clone())
                .connect()
                .await
                .unwrap();

        let b_secret = SecretKey::generate();
        let b_key = b_secret.public();
        let mut client_b =
            ClientBuilder::new(relay_url.clone(), b_secret, resolver)
                .connect()
                .await
                .unwrap();

        let payload = vec![42u8; 1024];
        let msg = Datagrams::from(payload.as_slice());
        client_a
            .send(ClientToRelayMsg::Datagrams {
                dst_endpoint_id: b_key,
                datagrams: msg,
            })
            .await
            .unwrap();

        loop {
            let received =
                tokio::time::timeout(Duration::from_secs(10), client_b.next())
                    .await
                    .expect("timeout")
                    .expect("stream ended")
                    .unwrap();

            match received {
                RelayToClientMsg::Datagrams {
                    remote_endpoint_id, ..
                } => {
                    assert_eq!(remote_endpoint_id, a_key);
                    break;
                }
                RelayToClientMsg::Ping { .. } => continue,
                other => panic!("unexpected message type: {:?}", other),
            }
        }
    });
}

fn external_relay_benchmarks(c: &mut Criterion) {
    let urls = match std::env::var("KITSUNE2_IROH_RELAY_BENCH_URLS") {
        Ok(val) => {
            let urls = val
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>();
            if urls.is_empty() {
                return;
            }
            urls
        }
        Err(_) => return,
    };

    let mut group = c.benchmark_group("external_relay/roundtrip");
    group.measurement_time(Duration::from_secs(60));
    group.sample_size(10);

    for url_str in &urls {
        let relay_url: RelayUrl = url_str
            .parse()
            .unwrap_or_else(|e| panic!("invalid relay URL {url_str}: {e}"));

        group.bench_function(BenchmarkId::new("1KiB", url_str), |b| {
            b.iter(|| {
                relay_roundtrip(&relay_url);
            });
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    local_relay_benchmarks,
    local_relay_roundtrip,
    external_relay_benchmarks
);
criterion_main!(benches);
