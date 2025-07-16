use dashmap::DashSet;
use warp::Filter;
use futures::{SinkExt, StreamExt};
use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation, TokenData};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use log::{info, warn, error, debug};
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::{Instant, Duration};
use governor::{Quota, RateLimiter};
use nonzero_ext::nonzero;
use tokio::time::interval;
use once_cell::sync::Lazy;

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
            
            // 如果没有token或验证失败，返回未授权状态
            ws.on_upgrade(move |socket| {
                async move {
                    let mut socket = socket;
                    let _ = socket.send(warp::ws::Message::text("Unauthorized")).await;
                    // 关闭连接
                }
            })
        })
}

/// 优化的JWT Claims结构
#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    iss: String,      // 签发者
    sub: String,      // 用户ID
    exp: u64,         // 过期时间
    iat: u64,         // 签发时间
    jti: String,      // JWT ID
    #[serde(default)]
    roles: Vec<String>, // 用户角色
}

/// 增强的JWT验证器
struct JwtValidator {
    current_secret: String,
    previous_secrets: Vec<String>,
    revoked_tokens: DashSet<String>,
    allowed_issuers: HashSet<String>,
}

impl JwtValidator {
    fn new() -> Self {
        // 获取当前密钥，如果环境变量不存在则使用默认值（仅用于开发环境）
        let current_secret = std::env::var("JWT_CURRENT_SECRET")
            .unwrap_or_else(|_| {
                warn!("JWT_CURRENT_SECRET not set, using default (INSECURE)");
                "development_secret_key_do_not_use_in_production".to_string()
            });
            
        // 获取之前的密钥列表
        let previous_secrets = std::env::var("JWT_PREVIOUS_SECRETS")
            .unwrap_or_default()
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
            
        // 设置允许的签发者
        let mut allowed_issuers = HashSet::new();
        allowed_issuers.insert("hancoin-server".to_string());
        
        Self {
            current_secret,
            previous_secrets,
            revoked_tokens: DashSet::new(),
            allowed_issuers,
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
            Ok(token_data) => {
                // 手动验证iss
                if !self.allowed_issuers.contains(&token_data.claims.iss) {
                    warn!("Invalid issuer in token: {}", token_data.claims.iss);
                    return false;
                }

                // 增强claim验证
                if !token_data.claims.sub.starts_with("user-") {
                    warn!("Invalid user ID format: {}", token_data.claims.sub);
                    return false;
                }
                
                info!("User {} authenticated via WebSocket", token_data.claims.sub);
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
    
    /// 清理过期的撤销令牌
    fn cleanup_revoked(&self) {
        // 实际实现中，我们应该解析令牌并检查过期时间
        // 这里简化处理，假设所有撤销令牌在24小时后可以从集合中移除
        // 在生产环境中，应该使用更复杂的逻辑
        warn!("Token revocation cleanup not implemented");
    }
}

// 使用once_cell替代lazy_static
static JWT_VALIDATOR: Lazy<JwtValidator> = Lazy::new(|| JwtValidator::new());

/// 增强的JWT验证入口
fn validate_token(token: &str) -> bool {
    JWT_VALIDATOR.validate_token(token)
}

/// 优化的WebSocket消息处理
const MAX_MESSAGE_SIZE: usize = 1024;
const MAX_CONNECTIONS: usize = 1000;

async fn handle_ws(
    ws: warp::ws::WebSocket,
    state: Arc<Mutex<WsState>>,
    rate_limiter: Arc<RateLimiter>,
) {
    // 检查连接限制
    {
        let mut state = state.lock();
        if state.active_connections >= MAX_CONNECTIONS {
            drop(state); // 提前释放锁
            let mut ws = ws;
            let _ = ws.send(warp::ws::Message::text("Error: Too many connections")).await;
            return;
        }
        state.active_connections += 1;
        state.last_connection_time = Some(Instant::now());
    }
    
    info!("New WebSocket connection established");
    
    // 发送心跳
    let (mut ws_sink, mut ws_stream) = ws.split();
    let mut heartbeat = interval(Duration::from_secs(30));
    
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
                        if msg.as_bytes().len() > MAX_MESSAGE_SIZE {
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
                        } else if msg.is_binary() {
                            // 处理二进制消息
                            let _ = ws_sink.send(warp::ws::Message::text("Error: Binary messages not supported")).await;
                        } else if msg.is_close() {
                            // 客户端请求关闭连接
                            break;
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

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_jwt_validator() {
        let validator = JwtValidator::new();
        
        // 测试无效令牌
        assert!(!validator.validate_token("invalid.token.here"));
        
        // 测试撤销功能
        validator.revoke_token("some.token.here");
        assert!(!validator.validate_token("some.token.here"));
    }
}