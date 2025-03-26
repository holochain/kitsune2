use kitsune2_bootstrap_srv::{BootstrapSrv, Config};
use kitsune2_core::{Ed25519LocalAgent, Ed25519Verifier};
use kitsune2_test_utils::enable_tracing;
use std::sync::Arc;
use url::Url;

#[test]
fn connect_with_client() {
    enable_tracing();

    let s = BootstrapSrv::new(Config::testing()).unwrap();

    // Create a test agent
    let local_agent: kitsune2_api::DynLocalAgent =
        Arc::new(Ed25519LocalAgent::default());
    let info =
        kitsune2_test_utils::agent::AgentBuilder::default().build(local_agent);

    // Build the server URL to connect to the test bootstrap server
    let server_url =
        Url::parse(&format!("http://{:?}", s.listen_addrs()[0])).unwrap();

    // Put the agent info to the server
    kitsune2_bootstrap_client::blocking_put(server_url.clone(), &info).unwrap();

    // Get all agent infos from the server, should just be ours
    let infos = kitsune2_bootstrap_client::blocking_get(
        server_url.clone(),
        info.space.clone(),
        Arc::new(Ed25519Verifier),
    )
    .unwrap();

    assert_eq!(1, infos.len());
    assert_eq!(info, infos[0]);
}
