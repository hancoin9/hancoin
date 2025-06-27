mod types;
mod crypto;
mod p2p;
mod ws;

use crate::types::*;
use crate::crypto::*;
use crate::p2p::*;
use crate::ws::*;

use std::sync::Arc;
use dashmap::DashMap;
use std::sync::atomic::AtomicU64;
use warp::Filter;

#[tokio::main]
async fn main() {
    // 初始化日志
    env_logger::init();

    // 初始化账本
    let ledger = Arc::new(Ledger {
        accounts: DashMap::new(),
        issued: AtomicU64::new(0),
    });

    // 初始化聊天客户端管理
    let chat_clients = Arc::new(DashMap::new());

    // P2P Swarm（可选，若未用P2P可不调用）
    let _p2p_swarm = build_swarm().await.ok();

    // WebSocket 路由
    let ws_routes = ws::chat_routes(chat_clients.clone());

    // Faucet 路由（示例，可根据业务调整）
    let ledger_clone = ledger.clone();
    let faucet_route = warp::path("faucet")
        .and(warp::post())
        .and(warp::body::json())
        .and_then(move |req: serde_json::Value| {
            let _ledger = ledger_clone.clone();
            async move {
                // 这里可根据 req 结构和业务实现水龙头发币逻辑
                Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({"status": "ok"})))
            }
        });

    // 合并所有路由
    let routes = ws_routes.or(faucet_route);

    // 启动 warp 服务
    println!("Server running at http://0.0.0.0:3030/");
    warp::serve(routes).run(([0, 0, 0, 0], 3030)).await;
}