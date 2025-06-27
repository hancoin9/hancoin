mod types;
mod crypto;
mod p2p;
mod ws;

use crate::types::*;
use crate::p2p::*;
use crate::ws::*;

use std::sync::Arc;
use dashmap::DashMap;
use std::sync::atomic::AtomicU64;
use warp::Filter;

#[tokio::main]
async fn main() {
    env_logger::init();

    let ledger = Arc::new(Ledger {
        accounts: DashMap::new(),
        issued: AtomicU64::new(0),
    });

    // P2P Swarm（可选）
    let _p2p_swarm = build_swarm().await.ok();

    // WebSocket 路由
    let ws_routes = ws::chat_routes();

    // Faucet 路由（示例）
    let ledger_clone = ledger.clone();
    let faucet_route = warp::path("faucet")
        .and(warp::post())
        .and(warp::body::json())
        .and_then(move |req: serde_json::Value| {
            let _ledger = ledger_clone.clone();
            async move {
                Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({"status": "ok"})))
            }
        });

    let routes = ws_routes.or(faucet_route);

    println!("Server running at http://0.0.0.0:3030/");
    warp::serve(routes).run(([0, 0, 0, 0], 3030)).await;
}