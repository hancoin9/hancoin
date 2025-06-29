use warp::Filter;
use futures::{SinkExt, StreamExt};

pub fn chat_routes() -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    warp::path("ws")
        .and(warp::ws())
        .map(|ws: warp::ws::Ws| {
            ws.on_upgrade(handle_ws)
        })
}

async fn handle_ws(mut ws: warp::ws::WebSocket) {
    while let Some(Ok(msg)) = ws.next().await {
        if let Ok(text) = msg.to_str() {
            println!("WebSocket Received: {}", text);
            if let Err(e) = ws.send(warp::ws::Message::text(format!("echo: {}", text))).await {
                eprintln!("WebSocket send error: {:?}", e);
            }
        }
    }
}