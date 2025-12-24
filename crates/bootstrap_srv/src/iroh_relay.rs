use axum::{
    extract::Request,
    response::{IntoResponse, Response},
};
use http::{HeaderMap, Method, StatusCode, Uri};
use iroh::EndpointId;
use iroh_relay::server::{
    AccessConfig, ClientRateLimit, RelayConfig, Server, ServerConfig,
};
use std::num::NonZeroU32;
use tracing::{error, info, warn};

pub async fn handle_iroh_relay() -> impl IntoResponse {
    println!("hitting this method");
    (StatusCode::OK, b"iroh-relay")
}

pub async fn start_secure_relay_server() -> Server {
    Server::spawn(ServerConfig::<(), ()> {
        relay: Some(RelayConfig {
            // ✅ SECURE: Bind only to localhost
            http_bind_addr: ([127, 0, 0, 1], 0).into(),
            tls: None, // HTTP is fine for localhost
            limits: iroh_relay::server::Limits {
                // Reasonable limits for internal use
                accept_conn_limit: Some(20.0),
                accept_conn_burst: Some(100),
                client_rx: Some(ClientRateLimit {
                    bytes_per_second: NonZeroU32::new(2 * 1024 * 1024).unwrap(), // 2MB/s
                    max_burst_bytes: Some(NonZeroU32::new(256 * 1024).unwrap()), // 256KB burst
                }),
            },
            key_cache_capacity: Some(2048),
            access: create_secure_access_config(),
        }),
        quic: None,         // Disable QUIC for internal proxy
        metrics_addr: None, // Don't expose metrics port either
    })
    .await
    .expect("Failed to start secure relay server")
}

fn create_secure_access_config() -> AccessConfig {
    AccessConfig::Restricted(Box::new(|endpoint_id| {
        Box::pin(async move {
            // Your secure authentication logic
            match authenticate_endpoint(endpoint_id).await {
                Ok(_) => {
                    info!("✅ Granted relay access to: {}", endpoint_id);
                    iroh_relay::server::Access::Allow
                }
                Err(e) => {
                    warn!("❌ Denied relay access to {}: {}", endpoint_id, e);
                    iroh_relay::server::Access::Deny
                }
            }
        })
    }))
}

pub async fn proxy_to_internal_relay(
    req: Request,
    relay_port: u16,
) -> Result<Response, StatusCode> {
    println!("incoming method");
    let method = req.method().clone();
    let uri = req.uri();
    let headers = req.headers().clone();

    // Additional security: Validate the request
    if !is_valid_relay_request(&method, uri, &headers) {
        warn!("🚫 Blocked invalid relay request: {} {}", method, uri);
        return Err(StatusCode::BAD_REQUEST);
    }

    // Construct internal URL
    let internal_url = format!(
        "http://127.0.0.1:{}{}",
        relay_port,
        uri.path_and_query().map_or("", |x| x.as_str())
    );
    println!("internal url {internal_url:?}");

    // Forward request to internal relay server
    let client = reqwest::Client::new();
    let body = axum::body::to_bytes(req.into_body(), usize::MAX)
        .await
        .map_err(|_| StatusCode::BAD_REQUEST)?;

    match client
        .request(method, &internal_url)
        .headers(headers)
        .body(body)
        .send()
        .await
    {
        Ok(response) => {
            println!("got response from relay {response:?}");
            // Convert back to axum response
            let status = response.status();
            let headers = response.headers().clone();
            let body = response
                .bytes()
                .await
                .map_err(|_| StatusCode::BAD_GATEWAY)?;

            let mut resp = Response::new(axum::body::Body::from(body));
            *resp.status_mut() = status;
            *resp.headers_mut() = headers;

            Ok(resp)
        }
        Err(e) => {
            error!("Failed to proxy to relay: {}", e);
            println!("Failed to proxy to relay: {}", e);
            Err(StatusCode::BAD_GATEWAY)
        }
    }
}
fn is_valid_relay_request(
    method: &Method,
    uri: &Uri,
    headers: &HeaderMap,
) -> bool {
    // Only allow GET requests to specific paths
    if method != Method::GET {
        return false;
    }

    // Only allow known relay endpoints
    match uri.path() {
        "/relay" | "/ping" => true,
        _ => false,
    }

    // Could add more validation here:
    // - Check for required headers
    // - Validate User-Agent
    // - Rate limiting headers
    // etc.
}

async fn authenticate_endpoint(endpoint_id: EndpointId) -> Result<(), String> {
    // Your authentication logic here
    // This could check JWT tokens, API keys, database lookups, etc.

    // Example: Simple allowlist (replace with your logic)
    // if ALLOWED_ENDPOINTS.contains(&endpoint_id) {
    //     Ok(())
    // } else {
    //     Err("Endpoint not authorized".to_string())
    // }

    Ok(()) // Allow all for now
}

// Graceful shutdown
// pub async fn shutdown_relay() {
//     if let Some(server) = RELAY_SERVER.get() {
//         info!("Shutting down internal relay server...");
//         // Note: We can't call server.shutdown() here because it consumes self
//         // The server will be dropped when the process exits
//     }
// }
