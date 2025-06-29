use warp::Filter;
use futures::{SinkExt, StreamExt};

pub fn chat_routes() -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    warp::path("ws")
        .and(warp::ws())
        .map(|ws: warp::ws::Ws| {
            ws.on_upgrade(handle_ws)
        })
}

const MAX_MESSAGE_SIZE: usize = 1024;

async fn handle_ws(mut ws: warp::ws::WebSocket) {
    while let Some(Ok(msg)) = ws.next().await {
        if msg.len() > MAX_MESSAGE_SIZE {
            if let Err(e) = ws.send(warp::ws::Message::text("Message too large")).await {
                eprintln!("WebSocket send error: {:?}", e);
            }
            continue;
        }
        if let Ok(text) = msg.to_str() {
            println!("WebSocket Received: {}", text);
            if let Err(e) = ws.send(warp::ws::Message::text(format!("echo: {}", text))).await {
                eprintln!("WebSocket send error: {:?}", e);
            }
        }
    }
}