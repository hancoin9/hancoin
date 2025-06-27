use warp::Filter;
use futures::{SinkExt, StreamExt};

pub fn chat_routes() -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    warp::path("ws")
        .and(warp::ws())
        .map(|ws: warp::ws::Ws| {
            ws.on_upgrade(handle_ws)
        })
}

async fn handle_ws(ws: warp::ws::WebSocket) {
    let (mut tx, mut rx) = ws.split();
    while let Some(Ok(msg)) = rx.next().await {
        if let Ok(text) = msg.to_str() {
            println!("WebSocket Received: {}", text);
            tx.send(warp::ws::Message::text(format!("echo: {}", text))).await.ok();
        }
    }
}