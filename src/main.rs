mod types;
mod crypto;
mod p2p;
mod ws;
mod tor;
mod coinjoin;

use crate::types::*;
use crate::p2p::{start_p2p, P2PConfig};
use crate::ws::chat_routes;
use crate::crypto::{init_crypto, generate_keypair, sign_message};
use crate::tor::TorConfig;
use crate::coinjoin::{CoinJoinManager, CoinJoinSession, CoinJoinRequest, CoinJoinStatus};

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use std::net::IpAddr;
use std::collections::HashMap;
use std::convert::TryFrom;
use warp::Filter;
use log::{info, error, warn, debug};
use hex::decode;
use tokio::sync::RwLock;
use std::time::Duration;
use uuid::Uuid;
use ed25519_dalek::{Verifier, Signature, VerifyingKey};

// API版本常量
const API_VERSION: &str = "v1";

#[tokio::main]
async fn main() {
    // 初始化日志系统
    env_logger::init();
    info!("Starting HANCOIN node v0.3.0...");

    // 初始化加密子系统
    init_crypto();

    // 创建账本实例
    let ledger = Arc::new(Ledger::new());

    // 检查总供应量
    if ledger.issued.load(Ordering::SeqCst) >= HAN_TOTAL_SUPPLY {
        error!("Total supply limit reached!");
        return;
    }

    // 创建CoinJoin会话管理器
    let coinjoin_manager = Arc::new(tokio::sync::Mutex::new(CoinJoinManager::new(3600))); // 1小时超时
    
    // 创建P2P配置
    let mut p2p_config = p2p::P2PConfig::default();

    // 配置Tor
    let tor_enabled = std::env::var("ENABLE_TOR").unwrap_or_else(|_| "false".to_string()) == "true";
    if tor_enabled {
        p2p_config.tor_config.enabled = true;
        p2p_config.tor_config.proxy_addr = std::env::var("TOR_PROXY")
            .unwrap_or_else(|_| "127.0.0.1:9050".to_string());
        info!("Tor已启用，代理地址: {}", p2p_config.tor_config.proxy_addr);
    } else {
        info!("Tor未启用，使用标准网络连接");
    }
    
    // 启动P2P网络
    if let Err(e) = p2p::start_p2p(p2p_config).await {
        error!("Failed to start P2P network: {:?}", e);
    }

    // WebSocket路由
    let ws_routes = chat_routes();

    // 创建API路由
    let api_routes = create_api_routes(ledger.clone());
    
    // 创建CoinJoin API路由
    let coinjoin_routes = create_coinjoin_routes(coinjoin_manager.clone());

    // CORS配置
    let cors = warp::cors()
        .allow_any_origin()
        .allow_headers(vec!["Content-Type", "Authorization"])
        .allow_methods(vec!["GET", "POST", "PUT", "DELETE"]);
        
    // 组合所有路由
    let routes = ws_routes
        .or(api_routes)
        .or(coinjoin_routes)
        .with(cors)
        .recover(handle_rejection);

    // 启动服务器
    info!("Server running at http://0.0.0.0:3030/");
    warp::serve(routes).run(([0, 0, 0, 0], 3030)).await;
}

/// 创建API路由
fn create_api_routes(
    ledger: Arc<Ledger>,
) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    // 水龙头路由
    let faucet_route = warp::path(API_VERSION)
        .and(warp::path("faucet"))
        .and(warp::post())
        .and(warp::body::json())
        .and(with_ledger(ledger.clone()))
        .and_then(handle_faucet);

    // 查询余额路由
    let balance_route = warp::path(API_VERSION)
        .and(warp::path("balance"))
        .and(warp::path::param::<String>())
        .and(warp::get())
        .and(with_ledger(ledger.clone()))
        .and_then(handle_balance);

    // 转账路由
    let transfer_route = warp::path(API_VERSION)
        .and(warp::path("transfer"))
        .and(warp::post())
        .and(warp::body::json())
        .and(with_ledger(ledger.clone()))
        .and_then(handle_transfer);

    // 查询交易历史路由
    let transactions_route = warp::path(API_VERSION)
        .and(warp::path("transactions"))
        .and(warp::path::param::<String>())
        .and(warp::get())
        .and(with_ledger(ledger.clone()))
        .and_then(handle_transactions);

    // 发布动态消息路由
    let post_moment_route = warp::path(API_VERSION)
        .and(warp::path("moments"))
        .and(warp::post())
        .and(warp::body::json())
        .and(with_ledger(ledger.clone()))
        .and_then(handle_post_moment);

    // 查询动态消息路由
    let get_moments_route = warp::path(API_VERSION)
        .and(warp::path("moments"))
        .and(warp::get())
        .and(warp::query::<HashMap<String, String>>())
        .and(with_ledger(ledger.clone()))
        .and_then(handle_get_moments);

    // 系统状态路由
    let status_route = warp::path(API_VERSION)
        .and(warp::path("status"))
        .and(warp::get())
        .and(with_ledger(ledger.clone()))
        .and_then(handle_status);

    // 组合所有API路由
    faucet_route
        .or(balance_route)
        .or(transfer_route)
        .or(transactions_route)
        .or(post_moment_route)
        .or(get_moments_route)
        .or(status_route)
}

/// 将Ledger注入到处理程序中
fn with_ledger(
    ledger: Arc<Ledger>,
) -> impl Filter<Extract = (Arc<Ledger>,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || ledger.clone())
}

/// 处理水龙头请求
async fn handle_faucet(
    req: serde_json::Value,
    ledger: Arc<Ledger>,
) -> Result<impl warp::Reply, warp::Rejection> {
    // 提取并验证account_id
    let account_id = req.get("account_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| warp::reject::custom(HancoinError::MissingAccountId))?;

    // 提取并验证signature
    let signature = req.get("signature")
        .and_then(|v| v.as_str())
        .ok_or_else(|| warp::reject::custom(HancoinError::MissingSignature))?;

    // 验证账户ID格式
    if !is_valid_account_id(account_id) {
        return Err(warp::reject::custom(HancoinError::InvalidAccountIdFormat));
    }

    // 验证公钥格式
    let public_key_bytes = decode(account_id)
        .map_err(|_| warp::reject::custom(HancoinError::InvalidAccountIdFormat))?;
    let public_key = VerifyingKey::from_bytes(&public_key_bytes.try_into().unwrap())
        .map_err(|_| warp::reject::custom(HancoinError::InvalidPublicKey))?;

    // 验证签名
    let message = b"claim_faucet";
    let signature_bytes = decode(signature)
        .map_err(|_| warp::reject::custom(HancoinError::InvalidSignatureFormat))?;
    let signature = Signature::from_bytes(&signature_bytes.try_into().unwrap())
        .map_err(|_| warp::reject::custom(HancoinError::InvalidSignatureData))?;
    
    if public_key.verify(message, &signature).is_err() {
        return Err(warp::reject::custom(HancoinError::InvalidSignature));
    }

    // 获取当前时间
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_err(|_| warp::reject::custom(HancoinError::SystemTimeError))?
        .as_secs();

    // 获取或创建账户
    let mut account = match ledger.accounts.get(account_id) {
        Some(account) => account.clone(),
        None => {
            let new_account = Account::default();
            ledger.accounts.insert(account_id.to_string(), new_account.clone());
            new_account
        }
    };
    
    // 严格检查领取频率(24小时冷却)
    if now - account.last_claim < FAUCET_COOLDOWN {
        return Err(warp::reject::custom(HancoinError::FaucetCooldownNotOver));
    }

    // 检查总发行量(防止溢出)
    let new_issued = ledger.issued.load(Ordering::SeqCst) + FAUCET_DAILY_LIMIT;
    if new_issued > HAN_TOTAL_SUPPLY {
        return Err(warp::reject::custom(HancoinError::TotalSupplyLimitReached));
    }

    // 原子更新账户和总发行量
    account.balance = account.balance.saturating_add(FAUCET_DAILY_LIMIT);
    account.last_claim = now;
    ledger.accounts.insert(account_id.to_string(), account.clone());
    ledger.issued.store(new_issued, Ordering::SeqCst);
    
    // 记录审计日志
    debug!("Faucet claimed - account: {}, amount: {}, new balance: {}, total issued: {}",
        account_id, FAUCET_DAILY_LIMIT, account.balance, new_issued);
    
    // 记录日志
    info!("Faucet claimed by user {} (amount: {})", account_id, FAUCET_DAILY_LIMIT);

    Ok(warp::reply::json(&serde_json::json!({
        "status": "ok",
        "balance": account.balance,
        "issued": FAUCET_DAILY_LIMIT
    })))
}

/// 处理余额查询请求
async fn handle_balance(
    account_id: String,
    ledger: Arc<Ledger>,
) -> Result<impl warp::Reply, warp::Rejection> {
    let account = ledger.accounts.get(&account_id)
        .ok_or_else(|| warp::reject::custom(HancoinError::AccountNotFound))?;

    Ok(warp::reply::json(&serde_json::json!({
        "status": "ok",
        "balance": account.balance,
        "last_active": account.last_active
    })))
}

/// 处理转账请求
async fn handle_transfer(
    tx_req: serde_json::Value,
    ledger: Arc<Ledger>,
) -> Result<impl warp::Reply, warp::Rejection> {
    // 提取交易信息
    let from = tx_req.get("from")
        .and_then(|v| v.as_str())
        .ok_or_else(|| warp::reject::custom(HancoinError::InvalidTransaction))?;
    
    let to = tx_req.get("to")
        .and_then(|v| v.as_str())
        .ok_or_else(|| warp::reject::custom(HancoinError::InvalidTransaction))?;
    
    let amount = tx_req.get("amount")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| warp::reject::custom(HancoinError::InvalidTransaction))?;
    
    let signature = tx_req.get("signature")
        .and_then(|v| v.as_str())
        .ok_or_else(|| warp::reject::custom(HancoinError::InvalidTransaction))?;
    
    // 验证发送方账户存在
    let mut from_account = ledger.accounts.get(from)
        .ok_or_else(|| warp::reject::custom(HancoinError::AccountNotFound))?
        .clone();
    
    // 验证余额充足
    if from_account.balance < amount {
        return Err(warp::reject::custom(HancoinError::InvalidTransaction));
    }
    
    // 验证签名
    let public_key_bytes = decode(from)
        .map_err(|_| warp::reject::custom(HancoinError::InvalidAccountIdFormat))?;
    let public_key = VerifyingKey::from_bytes(&public_key_bytes.try_into().unwrap())
        .map_err(|_| warp::reject::custom(HancoinError::InvalidPublicKey))?;
    
    let message =