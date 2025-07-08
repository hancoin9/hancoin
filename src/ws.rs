use warp::Filter;
use futures::{SinkExt, StreamExt};
use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use log::{info, warn};

/// WebSocket路由配置
pub fn chat_routes() -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    warp::path("ws")
        .and(warp::query::<HashMap<String, String>>())
        .and(warp::ws())
        .map(|params: HashMap<String, String>, ws: warp::ws::Ws| {
            // 验证token
            if let Some(token) = params.get("token") {
                if validate_token(token) {
                    return ws.on_upgrade(handle_ws);
                }
            }
            warp::reply::with_status("Unauthorized", warp::http::StatusCode::UNAUTHORIZED)
        })
}

/// JWT Claims结构
#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    sub: String,
    exp: u64,
}

/// 验证JWT令牌
fn validate_token(token: &str) -> bool {
    // 生产环境必须设置JWT_SECRET环境变量
    let secret = std::env::var("JWT_SECRET")
        .expect("JWT_SECRET environment variable not set");
    match decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::new(Algorithm::HS256),
    ) {
        Ok(claims) => {
            info!("User {} authenticated via WebSocket", claims.claims.sub);
            true
        },
        Err(e) => {
            warn!("WebSocket authentication failed"); // 不记录详细错误以防信息泄露
            false
        }
    }
}

/// WebSocket消息处理
const MAX_MESSAGE_SIZE: usize = 1024;
// 使用缓冲区减少分配
async fn handle_ws(mut ws: warp::ws::WebSocket) {
    info!("New WebSocket connection established");
    let mut buf = String::with_capacity(MAX_MESSAGE_SIZE);

    while let Some(result) = ws.next().await {
        match result {
            Ok(msg) => {
                if msg.len() > MAX_MESSAGE_SIZE {
                    let _ = ws.send(warp::ws::Message::text("Error: Message too large")).await;
                    continue;
                }

                if let Ok(text) = msg.to_str() {
                    info!("Received WebSocket message: {}", text);
                    buf.clear();
                    buf.push_str("echo: ");
                    buf.push_str(text);
                    if let Err(e) = ws.send(warp::ws::Message::text(buf.clone())).await {
                        warn!("Failed to send WebSocket message: {:?}", e);
                        break;
                    }
                } else {
                    let _ = ws.send(warp::ws::Message::text("Error: Binary messages not supported")).await;
                }
            },
            Err(e) => {
                warn!("WebSocket error: {:?}", e);
                break;
            }
        }
    }

    info!("WebSocket connection closed");
}