use kitsune2_bootstrap_srv::{BootstrapSrv, Config};
use kitsune2_core::{Ed25519LocalAgent, Ed25519Verifier};
use kitsune2_test_utils::{auth::AuthHookServer, enable_tracing};
use std::sync::Arc;
use url::Url;

use kitsune2_bootstrap_client::*;

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
    blocking_put(server_url.clone(), &info).unwrap();

    // Get all agent infos from the server, should just be ours
    let infos = blocking_get(
        server_url.clone(),
        info.space.clone(),
        Arc::new(Ed25519Verifier),
    )
    .unwrap();

    assert_eq!(1, infos.len());
    assert_eq!(info, infos[0]);
}

#[test]
fn connect_with_auth() {
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

    let auth = AuthMaterial::new(b"hello".to_vec());

    // Put the agent info to the server
    blocking_put_auth(server_url.clone(), &info, Some(&auth)).unwrap();

    // Get all agent infos from the server, should just be ours
    let infos = blocking_get_auth(
        server_url.clone(),
        info.space.clone(),
        Arc::new(Ed25519Verifier),
        Some(&auth),
    )
    .unwrap();

    assert_eq!(1, infos.len());
    assert_eq!(info, infos[0]);
}

#[test]
fn connect_with_bad_auth_retries() {
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

    let auth = AuthMaterial::new(b"hello".to_vec());
    *auth.danger_access_token().lock().unwrap() = Some("bob".into());
    assert_eq!(
        "bob",
        auth.danger_access_token().lock().unwrap().as_ref().unwrap()
    );

    // This call is okay despite the bad token because it retries
    // getting a new token.
    blocking_put_auth(server_url.clone(), &info, Some(&auth)).unwrap();

    assert_ne!(
        "bob",
        auth.danger_access_token().lock().unwrap().as_ref().unwrap()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn auth_with_real_token_provider() {
    enable_tracing();

    let auth_hook = AuthHookServer::spawn(Some(b"hello".to_vec()));

    let mut config = Config::testing();
    config.auth.authentication_hook_server = Some(auth_hook.url());

    config.allowed_origins = Some(vec!["http://localhost".into()]);

    let s =
        tokio::task::block_in_place(move || BootstrapSrv::new(config).unwrap());

    // Build the server URL to connect to the test bootstrap server
    let server_url =
        Url::parse(&format!("http://{:?}", s.listen_addrs()[0])).unwrap();

    // First, the happy path
    let local_agent: kitsune2_api::DynLocalAgent =
        Arc::new(Ed25519LocalAgent::default());
    let info =
        kitsune2_test_utils::agent::AgentBuilder::default().build(local_agent);
    let auth1 = AuthMaterial::new(b"hello".to_vec());
    tokio::task::block_in_place(|| {
        blocking_put_auth(server_url.clone(), &info, Some(&auth1)).unwrap();
    });

    // Now, with bad auth material
    let local_agent: kitsune2_api::DynLocalAgent =
        Arc::new(Ed25519LocalAgent::default());
    let info =
        kitsune2_test_utils::agent::AgentBuilder::default().build(local_agent);
    let auth2 = AuthMaterial::new(b"bad".to_vec());
    tokio::task::block_in_place(|| {
        blocking_put_auth(server_url.clone(), &info, Some(&auth2)).unwrap_err();
    });
}

/// Test that authentication works using only the relay-independent auth module.
/// This test disables the relay server entirely to ensure we're testing only
/// the HTTP bootstrap auth code path.
#[tokio::test(flavor = "multi_thread")]
async fn auth_feature_independent() {
    enable_tracing();

    let auth_hook = AuthHookServer::spawn(Some(b"secret".to_vec()));

    let mut config = Config::testing();
    // Configure the relay-independent auth module via config.auth.
    config.auth.authentication_hook_server = Some(auth_hook.url());
    // Disable the relay server entirely - we're only testing HTTP bootstrap endpoints
    config.no_relay_server = true;
    config.allowed_origins = Some(vec!["http://localhost".into()]);

    let s =
        tokio::task::block_in_place(move || BootstrapSrv::new(config).unwrap());

    let server_url =
        Url::parse(&format!("http://{:?}", s.listen_addrs()[0])).unwrap();

    // Test 1: Valid auth material should succeed
    let local_agent: kitsune2_api::DynLocalAgent =
        Arc::new(Ed25519LocalAgent::default());
    let info =
        kitsune2_test_utils::agent::AgentBuilder::default().build(local_agent);
    let valid_auth = AuthMaterial::new(b"secret".to_vec());

    tokio::task::block_in_place(|| {
        // Authenticate with valid_auth material and put agent info into bootstrap
        blocking_put_auth(server_url.clone(), &info, Some(&valid_auth))
            .unwrap();

        // Get should also succeed with the same auth (token is reused)
        let infos = blocking_get_auth(
            server_url.clone(),
            info.space.clone(),
            Arc::new(Ed25519Verifier),
            Some(&valid_auth),
        )
        .unwrap();

        assert_eq!(1, infos.len());
        assert_eq!(info, infos[0]);
    });

    // Test 2: Invalid auth material should fail
    let local_agent2: kitsune2_api::DynLocalAgent =
        Arc::new(Ed25519LocalAgent::default());
    let info2 =
        kitsune2_test_utils::agent::AgentBuilder::default().build(local_agent2);
    let invalid_auth = AuthMaterial::new(b"wrong-secret".to_vec());

    tokio::task::block_in_place(|| {
        // Put should fail with invalid auth
        blocking_put_auth(server_url.clone(), &info2, Some(&invalid_auth))
            .unwrap_err();
    });

    // Test 3: No auth when auth is required should fail
    let local_agent3: kitsune2_api::DynLocalAgent =
        Arc::new(Ed25519LocalAgent::default());
    let info3 =
        kitsune2_test_utils::agent::AgentBuilder::default().build(local_agent3);

    tokio::task::block_in_place(|| {
        // Put without auth should fail when auth is configured
        blocking_put(server_url.clone(), &info3).unwrap_err();

        // Get without auth should also fail
        blocking_get(
            server_url.clone(),
            info3.space.clone(),
            Arc::new(Ed25519Verifier),
        )
        .unwrap_err();
    });
}

/// Test that the client re-authenticates when the server returns 401
/// due to token expiration.
#[tokio::test(flavor = "multi_thread")]
async fn reauth_on_token_expiration() {
    enable_tracing();

    let auth_hook = AuthHookServer::spawn(Some(b"secret".to_vec()));

    let mut config = Config::testing();
    config.auth.authentication_hook_server = Some(auth_hook.url());
    // Set a very short token timeout so we can test expiration
    config.auth.auth_token_idle_timeout = std::time::Duration::from_millis(200);
    config.no_relay_server = true;
    config.allowed_origins = Some(vec!["http://localhost".into()]);

    let s =
        tokio::task::block_in_place(move || BootstrapSrv::new(config).unwrap());

    let server_url =
        Url::parse(&format!("http://{:?}", s.listen_addrs()[0])).unwrap();

    let local_agent: kitsune2_api::DynLocalAgent =
        Arc::new(Ed25519LocalAgent::default());
    let info =
        kitsune2_test_utils::agent::AgentBuilder::default().build(local_agent);
    let auth = AuthMaterial::new(b"secret".to_vec());

    // First request should succeed and register the token
    tokio::task::block_in_place(|| {
        blocking_put_auth(server_url.clone(), &info, Some(&auth)).unwrap();
    });

    // Wait for the token to expire on the server
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    // Second request should get 401, re-authenticate, and succeed
    tokio::task::block_in_place(|| {
        // This should succeed because the client will re-authenticate
        // when it receives 401 from the expired token
        let infos = blocking_get_auth(
            server_url.clone(),
            info.space.clone(),
            Arc::new(Ed25519Verifier),
            Some(&auth),
        )
        .unwrap();

        assert_eq!(1, infos.len());
        assert_eq!(info, infos[0]);
    });
}

#[test]
fn fetch_relay_token_authenticates_and_caches() {
    enable_tracing();

    let auth_hook = AuthHookServer::spawn(Some(b"material".to_vec()));
    let mut config = Config::testing();
    config.auth.authentication_hook_server = Some(auth_hook.url());
    config.no_relay_server = true;
    let server =
        BootstrapSrv::new(config).expect("bootstrap server should start");
    let server_url =
        Url::parse(&format!("http://{}", server.listen_addrs()[0]))
            .expect("bootstrap server URL should parse");

    let auth = AuthMaterial::new(b"material".to_vec());

    let tok1 = blocking_fetch_relay_token(server_url.clone(), &auth)
        .expect("first fetch should succeed");
    assert_eq!(tok1, "test-auth-token-1");
    assert_eq!(auth.token().as_deref(), Some("test-auth-token-1"));

    let tok2 = blocking_fetch_relay_token(server_url, &auth)
        .expect("cached fetch should succeed");
    assert_eq!(tok2, "test-auth-token-1");
    assert_eq!(auth_hook.request_count(), 1);
}
