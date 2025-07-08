mod types;
mod crypto;
mod p2p;
mod ws;

use crate::types::*;
use crate::p2p::start_p2p;
use crate::ws::chat_routes;
use crate::crypto::{verify_signature, PublicKey, Signature};

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::SystemTime;
use warp::Filter;
use log::{info, error};
use hex::decode;

#[tokio::main]
async fn main() {
    // 初始化日志系统
    env_logger::init();
    info!("Starting HANCOIN node...");

    // 创建账本实例
    let ledger = Arc::new(Ledger::default());

    // 检查总供应量
    if ledger.issued.load(Ordering::SeqCst) >= HAN_TOTAL_SUPPLY {
        error!("Total supply limit reached!");
        return;
    }

    // 启动P2P网络
    if let Err(e) = start_p2p().await {
        error!("Failed to start P2P network: {:?}", e);
    }

    // WebSocket路由
    let ws_routes = chat_routes();

    // Faucet路由（优化版）
    let faucet_route = warp::path("faucet")
        .and(warp::post())
        .and(warp::body::json())
        .and_then(move |req: serde_json::Value| {
            let ledger = ledger.clone();
            async move {
                // 提取并验证account_id
                let account_id = req.get("account_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| error_response("Missing account_id"))?;

                // 提取并验证signature
                let signature = req.get("signature")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| error_response("Missing signature"))?;

                // 验证公钥格式
                let public_key_bytes = decode(account_id)
                    .map_err(|_| error_response("Invalid account_id format"))?;
                let public_key = PublicKey::from_bytes(&public_key_bytes)
                    .map_err(|_| error_response("Invalid public key"))?;

                // 验证签名
                let message = b"claim_faucet";
                let signature_bytes = decode(signature)
                    .map_err(|_| error_response("Invalid signature format"))?;
                let signature = Signature::from_bytes(&signature_bytes);
                if !verify_signature(&public_key, message, &signature) {
                    return error_response("Invalid signature");
                }

                // 获取当前时间
                let now = SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .map_err(|_| error_response("System time error"))?
                    .as_secs();

                // 使用DashMap原子操作检查领取频率
                // 使用entry API减少一次哈希查找
                let entry = ledger.accounts.entry(account_id.to_string()).or_insert(Account::default());
                let mut account = entry.value_mut();

                // 检查领取频率
                if now - account.last_claim < FAUCET_COOLDOWN {
                    // 错误处理
                }

                // 更新账户
                account.balance += FAUCET_DAILY_LIMIT;
                account.last_claim = now;

                Ok(warp::reply::json(&serde_json::json!({
                    "status": "ok",
                    "balance": account.balance,
                    "issued": new_issued
                })))
            }
        });

    // 组合路由
    // 替换CORS配置
    let routes = ws_routes.or(faucet_route)
        .with(warp::cors()
            .allow_origin(warp::cors::Origin::exact("http://localhost:8080".parse().unwrap())
            .or(warp::cors::Origin::exact("https://yourdomain.com".parse().unwrap())))
            .allow_headers(vec!["Content-Type"])
            .allow_methods(vec![warp::http::Method::GET, warp::http::Method::POST]));

    // 启动服务器
    info!("Server running at http://0.0.0.0:3030/");
    warp::serve(routes).run(([0, 0, 0, 0], 3030)).await;
}

// 统一错误响应处理
fn error_response(message: &str) -> Result<impl warp::Reply, warp::Rejection> {
    Ok(warp::reply::with_status(
        warp::reply::json(&serde_json::json!({"status": "error", "message": message})),
        warp::http::StatusCode::BAD_REQUEST,
    ))
}

// Faucet领取成功时
info!("Faucet claimed by user {} (amount: {})", account_id, FAUCET_DAILY_LIMIT);
// 不记录完整的交易信息或密钥