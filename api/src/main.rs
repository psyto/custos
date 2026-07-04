//! Custos HTTP API — serves the wallet-approval demo UI and exposes the engine.
//!
//!   GET  /             → the wallet-approval UI (static)
//!   GET  /api/demo     → the 3 built-in scenarios, evaluated fresh by the engine
//!   GET  /api/build    → build an unsigned "hidden delegate" tx for a real account
//!   POST /api/scan     → scan an unsigned base64 tx (pre-sign) → verdict
//!
//! Run: cargo run  (in api/), then open http://127.0.0.1:8787

use std::collections::HashMap;

use axum::{extract::Query, response::Html, routing::{get, post}, Json, Router};
use custos_engine::{loader, scenarios};
use serde::Deserialize;
use serde_json::json;

// A real mainnet USDC token account + its owner wallet (sourced from a live
// Jupiter tx); used as the default target for the "build a real tx" demo.
const DEMO_ATA: &str = "8N3vV8bPQJMWfM3kHnU79Q3Q9zfWjQtirPo11RN4ytnY";
const DEMO_OWNER: &str = "9C6A9o9VR8NZdpJRnLhP2EYV2vEJsDYAaE6D3yHXUiLd";

async fn index() -> Html<&'static str> {
    Html(include_str!("../../web/index.html"))
}

async fn demo() -> Json<serde_json::Value> {
    Json(serde_json::to_value(scenarios::builtin()).unwrap())
}

/// Build an unsigned drainer tx targeting a real token account, as a malicious
/// dApp would hand it to a connected wallet.
async fn build(Query(q): Query<HashMap<String, String>>) -> Json<serde_json::Value> {
    let account = q.get("account").cloned().unwrap_or_else(|| DEMO_ATA.into());
    let owner = q.get("owner").or_else(|| q.get("user")).cloned().unwrap_or_else(|| DEMO_OWNER.into());
    let tx = loader::build_hidden_approve_b64(&account, &owner);
    Json(json!({ "tx_base64": tx, "account": account, "owner": owner,
        "summary": "memo \"Claim your airdrop\" + hidden Approve(u64::MAX)" }))
}

#[derive(Deserialize)]
struct ScanBody {
    tx_base64: String,
    user: Option<String>,
}

/// The pre-sign check: simulate the exact tx the wallet is about to sign.
async fn scan(Json(b): Json<ScanBody>) -> Json<serde_json::Value> {
    let rpc = loader::default_rpc();
    let result = tokio::task::spawn_blocking(move || {
        std::panic::catch_unwind(|| loader::scan_b64(&b.tx_base64, b.user.as_deref(), &rpc))
    })
    .await
    .unwrap();
    match result {
        Ok(report) => Json(serde_json::to_value(report).unwrap()),
        Err(_) => Json(json!({ "error": "scan failed (RPC unavailable or rate-limited)" })),
    }
}

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/", get(index))
        .route("/api/demo", get(demo))
        .route("/api/build", get(build))
        .route("/api/scan", post(scan));

    let addr = "127.0.0.1:8787";
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    println!("Custos UI  →  http://{addr}");
    axum::serve(listener, app).await.unwrap();
}
