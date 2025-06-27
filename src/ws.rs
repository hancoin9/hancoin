use warp::Filter;
use futures::{SinkExt, StreamExt};
use std::sync::Arc;
use dashmap::DashMap;

pub type ChatClients = Arc<DashMap<String, warp::ws::WebSocket>>;

pub fn chat_routes(clients: ChatClients) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    warp::path("ws")
        .and(warp::ws())
        .and(with_clients(clients))
        .map(|ws: warp::ws::Ws, clients| {
            ws.on_upgrade(move |socket| client_connected(socket, clients))
        })
}

fn with_clients(
    clients: ChatClients,
) -> impl Filter<Extract = (ChatClients,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || clients.clone())
}

async fn client_connected(ws: warp::ws::WebSocket, _clients: ChatClients) {
    let (mut tx, mut rx) = ws.split();
    while let Some(Ok(msg)) = rx.next().await {
        if let Ok(text) = msg.to_str() {
            println!("WebSocket Received: {}", text);
            // 这里可以广播消息或业务处理
            tx.send(warp::ws::Message::text(format!("echo: {}", text))).await.ok();
        }
    }
}