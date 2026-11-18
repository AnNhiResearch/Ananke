//! Codegen của giao thức (AD-10): sinh `openapi.json` ở gốc repo.
//!
//! Dùng: `cargo run -p ananke-protocol -- gen openapi`
//!
//! File sinh ra được **commit**; SDK (`packages/sdk-*`) sinh tiếp từ nó.

use std::path::Path;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if !args.iter().any(|arg| arg == "openapi") {
        eprintln!("cách dùng: cargo run -p ananke-protocol -- gen openapi");
        std::process::exit(2);
    }

    let document = ananke_protocol::openapi::document();
    let mut json = serde_json::to_string_pretty(&document).expect("serialize OpenAPI");
    json.push('\n');

    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("repo root");
    let path = root.join("openapi.json");
    std::fs::write(&path, json).expect("ghi openapi.json");

    println!("đã ghi {}", path.display());
}