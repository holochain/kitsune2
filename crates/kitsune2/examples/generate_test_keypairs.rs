//! Generate two ed25519 keypairs and register them with the Holochain auth
//! server so they can be approved for relay access.
//!
//! Usage:
//! ```sh
//! cargo run -p kitsune2 --no-default-features --features transport-iroh \
//!     --example generate_test_keypairs -- https://hc-auth-iroh-unyt.holochain.org
//! ```
//!
//! The tool will:
//!   1. Generate (or load) two ed25519 keypairs, saved to disk as
//!      `test-keypair-1.secret` / `test-keypair-2.secret`.
//!   2. Register each public key with the auth server via
//!      `PUT /request-auth/{base64url_key}`.
//!   3. Print the public keys so you can approve them on the ops dashboard.
//!
//! After approving the keys, the relay stability test can be run — it will
//! generate fresh auth material (signed `/now` challenges) automatically.

use base64::Engine as _;
use ed25519_dalek::SigningKey;
use std::fs;
use std::path::Path;

fn load_or_generate_key(path: &Path) -> SigningKey {
    if path.exists() {
        let hex = fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("Failed to read {}: {e}", path.display()));
        let hex = hex.trim();
        let mut seed = [0u8; 32];
        for (i, chunk) in hex.as_bytes().chunks(2).enumerate() {
            seed[i] = u8::from_str_radix(
                std::str::from_utf8(chunk).unwrap(),
                16,
            )
            .unwrap_or_else(|e| {
                panic!("Invalid hex in {}: {e}", path.display())
            });
        }
        println!("Loaded existing key from {}", path.display());
        SigningKey::from_bytes(&seed)
    } else {
        let key = SigningKey::generate(&mut rand::thread_rng());
        let hex: String =
            key.to_bytes().iter().map(|b| format!("{b:02x}")).collect();
        fs::write(path, &hex).unwrap_or_else(|e| {
            panic!("Failed to write {}: {e}", path.display())
        });
        println!("Generated new key → {}", path.display());
        key
    }
}

fn register_key(auth_url: &str, pub_key_b64: &str) {
    let url = format!("{auth_url}/request-auth/{pub_key_b64}");
    println!("  PUT {url}");

    match ureq::put(&url).send_empty() {
        Ok(response) => {
            let status = response.status();
            let body = response
                .into_body()
                .read_to_string()
                .unwrap_or_default();
            println!("  → {status} {body}");
        }
        Err(e) => {
            println!("  → Error: {e}");
        }
    }
}

fn main() {
    let auth_url = std::env::args().nth(1).unwrap_or_else(|| {
        eprintln!(
            "Usage: generate_test_keypairs <AUTH_SERVER_URL>\n\
             Example: generate_test_keypairs https://hc-auth-iroh-unyt.holochain.org"
        );
        std::process::exit(1);
    });
    // Strip trailing slash if present.
    let auth_url = auth_url.trim_end_matches('/');

    for i in 1..=2 {
        let secret_path = Path::new(&format!("test-keypair-{i}.secret"))
            .to_path_buf();
        let signing_key = load_or_generate_key(&secret_path);
        let pub_key_bytes = signing_key.verifying_key().as_bytes().to_vec();

        let pub_key_b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(&pub_key_bytes);
        let pub_key_hex: String = pub_key_bytes
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();

        println!();
        println!("========================================");
        println!("  Keypair {i}");
        println!("========================================");
        println!();
        println!("  Public Key (hex):    {pub_key_hex}");
        println!("  Public Key (base64): {pub_key_b64}");
        println!();

        println!("  Registering with auth server...");
        register_key(auth_url, &pub_key_b64);
    }

    println!();
    println!("========================================");
    println!("  Next steps");
    println!("========================================");
    println!();
    println!("  1. Approve both keys on the auth dashboard:");
    println!("     {auth_url}/ops/auth");
    println!();
    println!("  2. Run the relay stability test:");
    println!("     KITSUNE2_TEST_AUTH_URL={auth_url} \\");
    println!(
        "     KITSUNE2_TEST_BOOTSTRAP_URL={auth_url}/bootstrap \\"
    );
    println!("     KITSUNE2_TEST_RELAY_URL={auth_url}/relay \\");
    println!("     cargo test -p kitsune2 --no-default-features --features transport-iroh \\");
    println!("         --test relay_stability -- --ignored --nocapture");
}
