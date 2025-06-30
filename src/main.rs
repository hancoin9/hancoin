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

    let ledger = Arc::new(Ledger {        accounts: DashMap::new(),        issued: AtomicU64::new(0),    });

    // 检查发行量是否超过总量
    if ledger.issued.load(Ordering::SeqCst) >= HAN_TOTAL_SUPPLY {
        eprintln!("Total supply has reached the limit!");
        return;
    }

    // P2P Swarm（可选）
    let _ = build_swarm().await.ok();

    // WebSocket 路由
    let ws_routes = ws::chat_routes();

    // Faucet 路由（示例）
    let faucet_route = warp::path("faucet")
        .and(warp::post())
        .and(warp::body::json())
        .and_then(move |req: serde_json::Value| {
            let ledger = ledger.clone();
            async move {
                if let Some(account_id) = req.get("account_id").and_then(|v| v.as_str()) {
                    let mut account = ledger.accounts.entry(account_id.to_string()).or_insert(Account::default());                    let new_issued = ledger.issued.load(Ordering::SeqCst) + 100;
                    if new_issued > HAN_TOTAL_SUPPLY {
                        return Ok(warp::reply::json(&serde_json::json!({"status": "error", "message": "Total supply limit reached"})));
                    }
                    account.balance += 100; // 示例领取 100 个币                    ledger.issued.fetch_add(100, Ordering::SeqCst);                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({"status": "ok"})))
                } else {
                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({"status": "error", "message": "Missing account_id"})))
                }
            }
        });

    let routes = ws_routes.or(faucet_route);

    println!("Server running at http://0.0.0.0:3030/");
    warp::serve(routes).run(([0, 0, 0, 0], 3030)).await;
}