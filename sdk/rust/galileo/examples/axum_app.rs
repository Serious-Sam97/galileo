//! GALILEO_ENDPOINT / GALILEO_API_KEY / OTEL_SERVICE_NAME then `cargo run --example axum_app`; GET :3007/orders/42
use axum::{extract::Path, routing::get, Json, Router};
use galileo::identity::Identity;

async fn order(Path(id): Path<u32>) -> Json<serde_json_value::Value> {
    galileo::identity::set(Identity { user_id: Some("u-7".into()), tenant: Some("acme".into()), ..Default::default() });
    let total = galileo::traced!("price_order", { id as f64 * 9.9 });
    let row = galileo::sql!("SELECT id, total FROM orders WHERE id = $1", "postgresql", async { tokio::time::sleep(std::time::Duration::from_millis(12)).await; (id, total) });
    if id == 13 { let e = std::io::Error::other("cart is null"); galileo::capture_error(&e); }
    Json(serde_json_value::json!({ "id": row.0, "total": row.1 }))
}

mod serde_json_value { pub use serde_json::{json, Value}; }

#[tokio::main]
async fn main() {
    let _g = galileo::init(galileo::Config::from_env());
    let app = Router::new().route("/orders/{id}", get(order)).layer(axum::middleware::from_fn(galileo::axum::middleware));
    let l = tokio::net::TcpListener::bind("127.0.0.1:3007").await.unwrap();
    println!("example on :3007");
    axum::serve(l, app).await.unwrap();
}
