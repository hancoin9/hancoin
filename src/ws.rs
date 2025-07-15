use dashmap::DashSet;
use warp::Filter;
use futures::{SinkExt, StreamExt};
use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation, Header};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use log::{info, warn, error, debug};
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::{Instant, Duration};
use governor::{Quota, RateLimiter};
use nonzero_ext::nonzero;
use tokio::time::interval;
use hmac::{Hmac, Mac};
use sha2::Sha256;

/// WebSocket连接状态
#[derive(Default)]
struct WsState {
    active_connections: usize,
    last_connection_time: Option<Instant>,
    message_count: usize,
}

/// WebSocket路由配置
pub fn chat_routes() -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    let state = Arc::new(Mutex::new(WsState::default()));
    let rate_limiter = Arc::new(RateLimiter::direct(Quota::per_second(nonzero!(10)))); // 10 msg/s
    
    warp::path("ws")
        .and(warp::query::<HashMap<String, String>>())
        .and(warp::ws())
        .map(move |params: HashMap<String, String>, ws: warp::ws::Ws| {
            let state = state.clone();
            let rate_limiter = rate_limiter.clone();
            
            // 验证token
            if let Some(token) = params.get("token") {
                if validate_token(token) {
                    return ws.on_upgrade(move |socket| {
                        handle_ws(socket, state, rate_limiter)
                    });
                }
            }
            warp::reply::with_status("Unauthorized", warp::http::StatusCode::UNAUTHORIZED)
        })
}

/// 优化的JWT Claims结构
#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    iss: Option<HashSet<String>>, // 签发者集合
    sub: String, // 用户ID
    exp: u64,    // 过期时间
    iat: u64,    // 签发时间
    jti: String, // JWT ID
}

/// 增强的JWT验证器
struct JwtValidator {
    current_secret: String,
    previous_secrets: Vec<String>,
    revoked_tokens: DashSet<String>,
}

impl JwtValidator {
    fn new() -> Self {
        Self {
            current_secret: std::env::var("JWT_CURRENT_SECRET")
                .expect("JWT_CURRENT_SECRET must be set"),
            previous_secrets: std::env::var("JWT_PREVIOUS_SECRETS")
                .unwrap_or_default()
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect(),
            revoked_tokens: DashSet::new(),
        }
    }

    /// 验证JWT令牌
    fn validate_token(&self, token: &str) -> bool {
        // 检查令牌是否被撤销
        if self.revoked_tokens.contains(token) {
            warn!("Attempt to use revoked token");
            return false;
        }

        // 尝试用当前密钥验证
        if self.try_validate(token, &self.current_secret) {
            return true;
        }

        // 尝试用旧密钥验证(支持密钥轮换)
        for secret in &self.previous_secrets {
            if self.try_validate(token, secret) {
                return true;
            }
        }

        false
    }

    /// 实际验证逻辑
    fn try_validate(&self, token: &str, secret: &str) -> bool {
        let validation = Validation {
            algorithms: vec![Algorithm::HS256],
            validate_exp: true,
            validate_nbf: true,
            iss: None, // 禁用自动iss验证，改为手动验证
            ..Default::default()
        };

        match decode::<Claims>(
            token,
            &DecodingKey::from_secret(secret.as_bytes()),
            &validation,
        ) {
            Ok(claims) => {
                // 手动验证iss
                if let Some(issuers) = &claims.claims.iss {
                    if !issuers.contains("hancoin-server") {
                        warn!("Invalid issuer in token");
                        return false;
                    }
                } else {
                    warn!("Missing issuer in token");
                    return false;
                }

                // 增强claim验证
                if !claims.claims.sub.starts_with("user-") {
                    warn!("Invalid user ID format");
                    return false;
                }
                
                info!("User {} authenticated via WebSocket", claims.claims.sub);
                true
            },
            Err(e) => {
                debug!("JWT validation failed: {}", e);
                false
            }
        }
    }

    /// 撤销令牌
    fn revoke_token(&self, token: &str) {
        self.revoked_tokens.insert(token.to_string());
    }
}

lazy_static::lazy_static! {
    static ref JWT_VALIDATOR: JwtValidator = JwtValidator::new();
}

/// 增强的JWT验证入口
fn validate_token(token: &str) -> bool {
    JWT_VALIDATOR.validate_token(token)
}

/// 优化的WebSocket消息处理
const MAX_MESSAGE_SIZE: usize = 1024;
const MAX_CONNECTIONS: usize = 1000;

async fn handle_ws(
    mut ws: warp::ws::WebSocket,
    state: Arc<Mutex<WsState>>,
    rate_limiter: Arc<RateLimiter>,
) {
    // 检查连接限制
    {
        let mut state = state.lock();
        if state.active_connections >= MAX_CONNECTIONS {
            let _ = ws.send(warp::ws::Message::text("Error: Too many connections")).await;
            return;
        }
        state.active_connections += 1;
        state.last_connection_time = Some(Instant::now());
    }
    
    info!("New WebSocket connection established");
    
    // 发送心跳
    let (mut ws_sink, mut ws_stream) = ws.split();
    let heartbeat = interval(Duration::from_secs(30));
    
    tokio::pin!(heartbeat);
    
    // 使用固定大小的缓冲区
    let mut buf = [0u8; MAX_MESSAGE_SIZE];
    
    loop {
        tokio::select! {
            _ = heartbeat.tick() => {
                if let Err(e) = ws_sink.send(warp::ws::Message::ping(&[])).await {
                    warn!("Heartbeat failed: {:?}", e);
                    break;
                }
            },
            result = ws_stream.next() => {
                match result {
                    Some(Ok(msg)) => {
                        // 检查消息大小
                        if msg.len() > MAX_MESSAGE_SIZE {
                            let _ = ws_sink.send(warp::ws::Message::text("Error: Message too large")).await;
                            continue;
                        }
                        
                        // 检查速率限制
                        if rate_limiter.check().is_err() {
                            let _ = ws_sink.send(warp::ws::Message::text("Error: Rate limit exceeded")).await;
                            continue;
                        }
                        
                        // 处理文本消息
                        if let Ok(text) = msg.to_str() {
                            debug!("Received WebSocket message: {}", text);
                            
                            // 更新状态
                            state.lock().message_count += 1;
                            
                            // 构造响应
                            let response = format!("echo: {}", text);
                            if let Err(e) = ws_sink.send(warp::ws::Message::text(response)).await {
                                warn!("Failed to send WebSocket message: {:?}", e);
                                break;
                            }
                        } else {
                            let _ = ws_sink.send(warp::ws::Message::text("Error: Binary messages not supported")).await;
                        }
                    },
                    Some(Err(e)) => {
                        warn!("WebSocket error: {:?}", e);
                        break;
                    },
                    None => break,
                }
            }
        }
    }
    
    // 更新连接状态
    state.lock().active_connections -= 1;
    info!("WebSocket connection closed");
}