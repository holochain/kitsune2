use base64::prelude::*;
use rand::Rng;

#[tokio::main]
async fn main() {
    const USAGE: &str = "usage: test-auth-hook-server <port>";
    let mut arg_iter = std::env::args();
    arg_iter.next().expect(USAGE);
    let port: u16 = arg_iter.next().expect(USAGE).parse().expect(USAGE);

    println!("#STARTUP_PORT#{port}#");

    async fn handle_auth(body: bytes::Bytes) -> axum::response::Response {
        if &body[..] != b"valid" {
            return axum::response::IntoResponse::into_response((
                axum::http::StatusCode::UNAUTHORIZED,
                "Unauthorized",
            ));
        }
        let mut token = [0; 32];
        rand::thread_rng().fill(&mut token);
        let token = BASE64_URL_SAFE_NO_PAD.encode(&token[..]);
        axum::response::IntoResponse::into_response(axum::Json(
            serde_json::json!({
                "authToken": token,
            }),
        ))
    }

    let app: axum::Router<()> = axum::Router::new()
        .route("/authenticate", axum::routing::put(handle_auth));

    let h = axum_server::Handle::default();
    let h2 = h.clone();

    let task = tokio::task::spawn(async move {
        axum_server::bind(([127, 0, 0, 1], port).into())
            .handle(h2)
            .serve(app.into_make_service_with_connect_info::<std::net::SocketAddr>())
            .await
            .unwrap();
    });

    let hook_addr = h.listening().await.unwrap();

    println!("#SERVER_RUNNING#http://{hook_addr:?}/authenticate#");

    task.await.unwrap();
}
