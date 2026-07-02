//! Dev-only tool: generate the PeterFan signing keypair and issue license keys.
//!
//! Never ships to customers and is excluded from the workspace (see the root
//! `Cargo.toml`'s `exclude`). The private key it prints must be stored
//! offline (a password manager or hardware key) and never committed — anyone
//! who has it can mint valid PeterFan licenses.
//!
//! Usage:
//!   cargo run -- genkey
//!   cargo run -- issue --private <b64> --email buyer@example.com [--days 365] [--seats 1]

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use serde::Serialize;

#[derive(Serialize)]
struct LicensePayload {
    email: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    exp: Option<u64>,
    seats: u8,
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("genkey") => genkey(),
        Some("issue") => issue(&args[2..]),
        _ => {
            eprintln!("usage:\n  genkey\n  issue --private <b64> --email <email> [--days <n>] [--seats <n>]");
            std::process::exit(1);
        }
    }
}

fn genkey() {
    let mut csprng = rand::rngs::OsRng;
    let signing_key = SigningKey::generate(&mut csprng);
    let verifying_key: VerifyingKey = signing_key.verifying_key();

    println!("Public key  (embed in packages/core/src/license.rs PUBLIC_KEY_B64):");
    println!("  {}", URL_SAFE_NO_PAD.encode(verifying_key.to_bytes()));
    println!();
    println!("Private key (store OFFLINE — password manager / hardware key; NEVER commit):");
    println!("  {}", URL_SAFE_NO_PAD.encode(signing_key.to_bytes()));
    println!();
    println!("Losing the private key means you can never issue new licenses with this public");
    println!("key again. Leaking it means anyone can mint valid PeterFan licenses.");
}

fn issue(args: &[String]) {
    let mut private_b64 = None;
    let mut email = None;
    let mut days: Option<u64> = None;
    let mut seats: u8 = 1;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--private" => {
                private_b64 = args.get(i + 1).cloned();
                i += 2;
            }
            "--email" => {
                email = args.get(i + 1).cloned();
                i += 2;
            }
            "--days" => {
                days = args.get(i + 1).and_then(|s| s.parse().ok());
                i += 2;
            }
            "--seats" => {
                seats = args.get(i + 1).and_then(|s| s.parse().ok()).unwrap_or(1);
                i += 2;
            }
            _ => i += 1,
        }
    }

    let (Some(private_b64), Some(email)) = (private_b64, email) else {
        eprintln!("issue requires --private <b64> --email <email>");
        std::process::exit(1);
    };

    let Ok(sk_bytes) = URL_SAFE_NO_PAD.decode(&private_b64) else {
        eprintln!("--private is not valid base64url");
        std::process::exit(1);
    };
    let Ok(sk_arr) = <[u8; 32]>::try_from(sk_bytes.as_slice()) else {
        eprintln!("--private must decode to 32 bytes");
        std::process::exit(1);
    };
    let signing_key = SigningKey::from_bytes(&sk_arr);

    let exp = days.map(|d| {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock before 1970")
            .as_secs();
        now + d * 86_400
    });

    let payload = LicensePayload { email, exp, seats };
    let json = serde_json::to_vec(&payload).expect("payload always serializes");
    let signature = signing_key.sign(&json);

    let key = format!(
        "PFAN1-{}.{}",
        URL_SAFE_NO_PAD.encode(&json),
        URL_SAFE_NO_PAD.encode(signature.to_bytes())
    );
    println!("{key}");
}
