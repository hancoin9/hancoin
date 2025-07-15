mod types;
mod crypto;
mod p2p;
mod ws;
mod tor;
mod coinjoin;

use crate::types::*;
use crate::p2p::{start_p2p, P2PConfig};
use crate::ws::chat_routes;
use crate::crypto::{verify_signature, PublicKey, Signature};
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

// API版本常量
const API_VERSION: &str = "v1";

#[tokio::main]
async fn main() {
    // 初始化日志系统
    env_logger::init();
    info!("Starting HANCOIN node v0.3.0...");

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
        .ok_or_else(|| warp::reject::custom(Error("Missing account_id".to_string())))?;

    // 提取并验证signature
    let signature = req.get("signature")
        .and_then(|v| v.as_str())
        .ok_or_else(|| warp::reject::custom(Error("Missing signature".to_string())))?;

    // 验证公钥格式
    let public_key_bytes = decode(account_id)
        .map_err(|_| warp::reject::custom(Error("Invalid account_id format".to_string())))?;
    let public_key = PublicKey::from_bytes(&public_key_bytes)
        .map_err(|_| warp::reject::custom(Error("Invalid public key".to_string())))?;

    // 验证签名
    let message = b"claim_faucet";
    let signature_bytes = decode(signature)
        .map_err(|_| warp::reject::custom(Error("Invalid signature format".to_string())))?;
    let signature = Signature::from_bytes(&signature_bytes)
        .map_err(|_| warp::reject::custom(Error("Invalid signature data".to_string())))?;
    if !verify_signature(&public_key, message, &signature) {
        return Err(warp::reject::custom(Error("Invalid signature".to_string())));
    }

    // 获取当前时间
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_err(|_| warp::reject::custom(Error("System time error".to_string())))?
        .as_secs();

    // 使用DashMap原子操作检查领取频率
    let mut account = ledger.get_or_create_account(account_id);
    
    // 严格检查领取频率(24小时冷却)
    if now - account.last_claim < 86400 {
        return Err(warp::reject::custom(Error("Faucet cooldown period not over".to_string())));
    }

    // 检查总发行量(防止溢出)
    let new_issued = ledger.issued.load(Ordering::SeqCst) + FAUCET_DAILY_LIMIT;
    if new_issued > HAN_TOTAL_SUPPLY {
        return Err(warp::reject::custom(Error("Total supply limit reached".to_string())));
    }

    // 原子更新账户和总发行量
    account.balance = account.balance.saturating_add(FAUCET_DAILY_LIMIT);
    account.last_claim = now;
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
        .ok_or_else(|| warp::reject::custom(Error("Account not found".to_string())))?;

    Ok(warp::reply::json(&serde_json::json!({
        "status": "ok",
        "balance": account.balance,
        "last_active": account.last_active
    })))
}

/// 处理转账请求
async fn handle_transfer(
    tx: Tx,
    ledger: Arc<Ledger>,
) -> Result<impl warp::Reply, warp::Rejection> {
    // 验证交易
    if !tx.is_valid() {
        return Err(warp::reject::custom(Error("Invalid transaction".to_string())));
    }

    // 执行转账
    ledger.transfer(&tx)
        .map_err(|e| warp::reject::custom(Error(e)))?;

    Ok(warp::reply::json(&serde_json::json!({
        "status": "ok",
        "tx_id": format!("tx_{}", Uuid::new_v4())
    })))
}

/// 处理交易历史查询请求
async fn handle_transactions(
    account_id: String,
    ledger: Arc<Ledger>,
) -> Result<impl warp::Reply, warp::Rejection> {
    let account = ledger.accounts.get(&account_id)
        .ok_or_else(|| warp::reject::custom(Error("Account not found".to_string())))?;

    Ok(warp::reply::json(&serde_json::json!({
        "status": "ok",
        "transactions": account.transactions
    })))
}

/// 处理发布动态消息请求
async fn handle_post_moment(
    req: serde_json::Value,
    ledger: Arc<Ledger>,
) -> Result<impl warp::Reply, warp::Rejection> {
    let from = req.get("from")
        .and_then(|v| v.as_str())
        .ok_or_else(|| warp::reject::custom(Error("Missing from".to_string())))?;

    let text = req.get("text")
        .and_then(|v| v.as_str())
        .ok_or_else(|| warp::reject::custom(Error("Missing text".to_string())))?;

    let signature = req.get("signature")
        .and_then(|v| v.as_str())
        .ok_or_else(|| warp::reject::custom(Error("Missing signature".to_string())))?;

    // 验证签名
    let public_key_bytes = decode(from)
        .map_err(|_| warp::reject::custom(Error("Invalid from format".to_string())))?;
    let public_key = PublicKey::from_bytes(&public_key_bytes)
        .map_err(|_| warp::reject::custom(Error("Invalid public key".to_string())))?;

    let signature_bytes = decode(signature)
        .map_err(|_| warp::reject::custom(Error("Invalid signature format".to_string())))?;
    let signature = Signature::from_bytes(&signature_bytes)
        .map_err(|_| warp::reject::custom(Error("Invalid signature data".to_string())))?;
    
    if !verify_signature(&public_key, text.as_bytes(), &signature) {
        return Err(warp::reject::custom(Error("Invalid signature".to_string())));
    }

    let moment = Moment::new(from.to_string(), text.to_string(), signature.to_string());
    let moment_id = format!("moment_{}", Uuid::new_v4());
    ledger.moments.insert(moment_id.clone(), moment);

    Ok(warp::reply::json(&serde_json::json!({
        "status": "ok",
        "moment_id": moment_id
    })))
}

/// 处理查询动态消息请求
async fn handle_get_moments(
    query: HashMap<String, String>,
    ledger: Arc<Ledger>,
) -> Result<impl warp::Reply, warp::Rejection> {
    let limit = query.get("limit")
        .and_then(|v| v.parse::<usize>().ok())
        .map(|n| std::cmp::min(n, 100)) // 限制最大100条
        .unwrap_or(20);
    let offset = query.get("offset")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(0);
        
    // 防止offset过大导致性能问题
    let offset = std::cmp::min(offset, ledger.moments.len().saturating_sub(1));

    let moments: Vec<_> = ledger.moments.iter()
        .skip(offset)
        .take(limit)
        .map(|entry| {
            let (id, moment) = entry.pair();
            serde_json::json!({
                "id": id,
                "from": moment.from,
                "text": moment.text,
                "timestamp": moment.timestamp,
                "likes": moment.likes
            })
        })
        .collect();

    Ok(warp::reply::json(&serde_json::json!({
        "status": "ok",
        "moments": moments,
        "count": moments.len()
    })))
}

/// 处理系统状态查询请求
async fn handle_status(
    ledger: Arc<Ledger>,
) -> Result<impl warp::Reply, warp::Rejection> {
    Ok(warp::reply::json(&serde_json::json!({
        "status": "ok",
        "total_accounts": ledger.accounts.len(),
        "total_moments": ledger.moments.len(),
        "total_issued": ledger.get_issued(),
        "total_supply": HAN_TOTAL_SUPPLY
    })))
}

// 自定义错误类型
#[derive(Debug)]
struct Error(String);

impl warp::reject::Reject for Error {}

#[derive(Debug)]
struct RateLimitError;

impl warp::reject::Reject for RateLimitError {}

/// 处理拒绝请求
async fn handle_rejection(err: warp::Rejection) -> Result<impl warp::Reply, std::convert::Infallible> {
    let code;
    let message;

    if err.is_not_found() {
        code = warp::http::StatusCode::NOT_FOUND;
        message = "Not Found";
    } else if let Some(e) = err.find::<Error>() {
        code = warp::http::StatusCode::BAD_REQUEST;
        message = &e.0;
    } else if let Some(_) = err.find::<RateLimitError>() {
        code = warp::http::StatusCode::TOO_MANY_REQUESTS;
        message = "Rate limit exceeded. Please try again later.";
    } else if let Some(_) = err.find::<warp::filters::body::BodyDeserializeError>() {
        code = warp::http::StatusCode::BAD_REQUEST;
        message = "Invalid JSON data";
    } else if let Some(_) = err.find::<warp::reject::MethodNotAllowed>() {
        code = warp::http::StatusCode::METHOD_NOT_ALLOWED;
        message = "Method not allowed";
    } else {
        // 未处理的错误
        code = warp::http::StatusCode::INTERNAL_SERVER_ERROR;
        message = "Internal Server Error";
        error!("Unhandled rejection: {:?}", err);
    }

    let json = warp::reply::json(&serde_json::json!({
        "status": "error",
        "message": message
    }));

    Ok(warp::reply::with_status(json, code))
}

/// 创建CoinJoin API路由
fn create_coinjoin_routes(
    manager: Arc<tokio::sync::Mutex<CoinJoinManager>>,
) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    // 创建CoinJoin会话路由
    let create_session_route = warp::path(API_VERSION)
        .and(warp::path("coinjoin"))
        .and(warp::path("sessions"))
        .and(warp::post())
        .and(warp::body::json())
        .and(with_coinjoin_manager(manager.clone()))
        .and_then(handle_create_coinjoin_session);

    // 获取CoinJoin会话路由
    let get_session_route = warp::path(API_VERSION)
        .and(warp::path("coinjoin"))
        .and(warp::path("sessions"))
        .and(warp::path::param::<String>())
        .and(warp::get())
        .and(with_coinjoin_manager(manager.clone()))
        .and_then(handle_get_coinjoin_session);

    // 添加输入路由
    let add_input_route = warp::path(API_VERSION)
        .and(warp::path("coinjoin"))
        .and(warp::path("sessions"))
        .and(warp::path::param::<String>())
        .and(warp::path("inputs"))
        .and(warp::post())
        .and(warp::body::json())
        .and(with_coinjoin_manager(manager.clone()))
        .and_then(handle_add_coinjoin_input);

    // 添加输出路由
    let add_output_route = warp::path(API_VERSION)
        .and(warp::path("coinjoin"))
        .and(warp::path("sessions"))
        .and(warp::path::param::<String>())
        .and(warp::path("outputs"))
        .and(warp::post())
        .and(warp::body::json())
        .and(with_coinjoin_manager(manager.clone()))
        .and_then(handle_add_coinjoin_output);

    // 添加签名路由
    let add_signature_route = warp::path(API_VERSION)
        .and(warp::path("coinjoin"))
        .and(warp::path("sessions"))
        .and(warp::path::param::<String>())
        .and(warp::path("signatures"))
        .and(warp::post())
        .and(warp::body::json())
        .and(with_coinjoin_manager(manager.clone()))
        .and_then(handle_add_coinjoin_signature);

    // 完成会话路由
    let finalize_session_route = warp::path(API_VERSION)
        .and(warp::path("coinjoin"))
        .and(warp::path("sessions"))
        .and(warp::path::param::<String>())
        .and(warp::path("finalize"))
        .and(warp::post())
        .and(warp::body::json())
        .and(with_coinjoin_manager(manager.clone()))
        .and_then(handle_finalize_coinjoin_session);

    // 组合所有CoinJoin路由
    create_session_route
        .or(get_session_route)
        .or(add_input_route)
        .or(add_output_route)
        .or(add_signature_route)
        .or(finalize_session_route)
}

/// 将CoinJoin管理器注入到处理程序中
fn with_coinjoin_manager(
    manager: Arc<tokio::sync::Mutex<CoinJoinManager>>,
) -> impl Filter<Extract = (Arc<tokio::sync::Mutex<CoinJoinManager>>,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || manager.clone())
}

/// 处理创建CoinJoin会话请求
async fn handle_create_coinjoin_session(
    req: CoinJoinRequest,
    manager: Arc<tokio::sync::Mutex<CoinJoinManager>>,
) -> Result<impl warp::Reply, warp::Rejection> {
    let session_info = {
        let mut manager = manager.lock().await;
        manager.create_session(&req)
    };

    Ok(warp::reply::with_status(
        warp::reply::json(&session_info),
        warp::http::StatusCode::CREATED,
    ))
}

/// 处理获取CoinJoin会话请求
async fn handle_get_coinjoin_session(
    id: String,
    manager: Arc<tokio::sync::Mutex<CoinJoinManager>>,
) -> Result<impl warp::Reply, warp::Rejection> {
    let session = {
        let manager = manager.lock().await;
        manager.get_session(&id)
    };

    match session {
        Some(session) => Ok(warp::reply::json(&session.get_info())),
        None => Err(warp::reject::custom(Error(format!("会话不存在: {}", id)))),
    }
}

/// 处理添加CoinJoin输入请求
async fn handle_add_coinjoin_input(
    id: String,
    req: coinjoin::InputRequest,
    manager: Arc<tokio::sync::Mutex<CoinJoinManager>>,
) -> Result<impl warp::Reply, warp::Rejection> {
    let result = {
        let mut manager = manager.lock().await;
        manager.add_input(&id, &req)
    };

    match result {
        Ok(info) => Ok(warp::reply::json(&info)),
        Err(err) => Err(warp::reject::custom(Error(err))),
    }
}

/// 处理添加CoinJoin输出请求
async fn handle_add_coinjoin_output(
    id: String,
    req: coinjoin::OutputRequest,
    manager: Arc<tokio::sync::Mutex<CoinJoinManager>>,
) -> Result<impl warp::Reply, warp::Rejection> {
    let result = {
        let mut manager = manager.lock().await;
        manager.add_output(&id, &req)
    };

    match result {
        Ok(info) => Ok(warp::reply::json(&info)),
        Err(err) => Err(warp::reject::custom(Error(err))),
    }
}

/// 处理添加CoinJoin签名请求
async fn handle_add_coinjoin_signature(
    id: String,
    req: coinjoin::SignatureRequest,
    manager: Arc<tokio::sync::Mutex<CoinJoinManager>>,
) -> Result<impl warp::Reply, warp::Rejection> {
    let result = {
        let mut manager = manager.lock().await;
        manager.add_signature(&id, &req)
    };

    match result {
        Ok(info) => Ok(warp::reply::json(&info)),
        Err(err) => Err(warp::reject::custom(Error(err))),
    }
}

/// 处理完成CoinJoin会话请求
async fn handle_finalize_coinjoin_session(
    id: String,
    req: coinjoin::FinalizeRequest,
    manager: Arc<tokio::sync::Mutex<CoinJoinManager>>,
) -> Result<impl warp::Reply, warp::Rejection> {
    let result = {
        let mut manager = manager.lock().await;
        manager.finalize_session(&id, &req)
    };

    match result {
        Ok(info) => Ok(warp::reply::json(&info)),
        Err(err) => Err(warp::reject::custom(Error(err))),
    }
}