//! Custos HTTP API — serves the wallet-approval demo UI and exposes the engine.
//!
//!   GET /            → the wallet-approval UI (static)
//!   GET /api/demo    → the 3 built-in scenarios, evaluated fresh by the engine
//!
//! Run: cargo run   (in api/), then open http://127.0.0.1:8787

use axum::{response::Html, routing::get, Json, Router};
use custos_engine::scenarios;

async fn index() -> Html<&'static str> {
    Html(include_str!("../../web/index.html"))
}

async fn demo() -> Json<serde_json::Value> {
    // CPU-bound LiteSVM work; fast enough to run inline for a demo.
    let reports = scenarios::builtin();
    Json(serde_json::to_value(reports).unwrap())
}

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/", get(index))
        .route("/api/demo", get(demo));

    let addr = "127.0.0.1:8787";
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    println!("Custos UI  →  http://{addr}");
    axum::serve(listener, app).await.unwrap();
}
