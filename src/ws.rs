use std::collections::{VecDeque, HashSet};
use std::sync::{Arc, Mutex};
use warp::ws::{Message, WebSocket};
use futures::{SinkExt, StreamExt};
use serde::{Serialize, Deserialize};
use tokio::sync::mpsc::UnboundedSender;

// 极速超级群聊，极简本地环形缓存
const MAX_MEGA_MSGS: usize = 1000;
const MAX_MSG_ID_SET: usize = 3000;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MegaGroupMsg {
    pub id: u64,
    pub from: String,
    pub text: String,
    pub timestamp: u64,
    pub signature: String,
}

#[derive(Default)]
pub struct MegaMsgRing {
    pub msgs: VecDeque<MegaGroupMsg>,
    pub seen_ids: HashSet<u64>,
}
impl MegaMsgRing {
    pub fn push(&mut self, msg: MegaGroupMsg) -> bool {
        if self.seen_ids.contains(&msg.id) {
            return false;
        }
        if self.msgs.len() >= MAX_MEGA_MSGS {
            if let Some(old) = self.msgs.pop_front() {
                self.seen_ids.remove(&old.id);
            }
        }
        self.seen_ids.insert(msg.id);
        if self.seen_ids.len() > MAX_MSG_ID_SET {
            if let Some(old) = self.msgs.front() {
                self.seen_ids.remove(&old.id);
            }
        }
        self.msgs.push_back(msg);
        true
    }
}

pub type MegaGroupClients = Arc<Mutex<Vec<UnboundedSender<Message>>>>;
pub type MegaGroupRing = Arc<Mutex<MegaMsgRing>>;

#[derive(Serialize, Deserialize, Debug)]
pub struct ChatMsg {
    pub from: String,
    pub text: String,
    pub timestamp: u64,
    pub signature: String,
    pub burn_after: Option<u64>,
    pub to: Option<String>,
    pub encrypted: Option<bool>,
    pub nonce: Option<String>,
    pub r#type: Option<String>,
}

pub type ChatClients = Arc<Mutex<Vec<UnboundedSender<Message>>>>;

pub fn wschat_filter(chat_clients: ChatClients) -> impl warp::Filter<Extract = (ChatClients,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || chat_clients.clone())
}

pub async fn wschat_handler(ws: WebSocket, chat_clients: ChatClients, _pubkey: String) {
    let (mut tx, mut rx) = ws.split();
    let (client_tx, mut client_rx) = tokio::sync::mpsc::unbounded_channel();
    {
        chat_clients.lock().unwrap().push(client_tx);
    }
    tokio::spawn(async move {
        while let Some(Ok(msg)) = rx.next().await {
            if let Ok(txt) = msg.to_str() {
                if let Ok(chat) = serde_json::from_str::<ChatMsg>(txt) {
                    if chat.text.len() > 256 { continue; }
                    let sendtxt = if chat.encrypted.unwrap_or(false) && chat.to.is_some() {
                        serde_json::to_string(&serde_json::json!({
                            "type":"chat",
                            "from":chat.from,
                            "to":chat.to.as_ref().unwrap(),
                            "text":chat.text,
                            "timestamp":chat.timestamp,
                            "burn_after":chat.burn_after.unwrap_or(0),
                            "encrypted":true,
                            "nonce":chat.nonce,
                        })).unwrap()
                    } else {
                        serde_json::to_string(&serde_json::json!({
                            "type":"chat",
                            "from":chat.from,
                            "text":chat.text,
                            "timestamp":chat.timestamp,
                            "burn_after":chat.burn_after.unwrap_or(0),
                            "encrypted":false
                        })).unwrap()
                    };
                    for c in chat_clients.lock().unwrap().iter() {
                        let _ = c.send(Message::text(&sendtxt));
                    }
                }
            }
        }
    });
    while let Some(msg) = client_rx.recv().await {
        let _ = tx.send(msg).await;
    }
}

pub fn wsmegagroup_filter(clients: MegaGroupClients) -> impl warp::Filter<Extract = (MegaGroupClients,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || clients.clone())
}

pub fn wsmegagroup_ring(ring: MegaGroupRing) -> impl warp::Filter<Extract = (MegaGroupRing,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || ring.clone())
}

pub async fn wsmegagroup_handler(
    ws: WebSocket,
    clients: MegaGroupClients,
    msg_ring: MegaGroupRing,
    _pubkey: String,
) {
    let (mut tx, mut rx) = ws.split();
    let (client_tx, mut client_rx) = tokio::sync::mpsc::unbounded_channel();
    {
        clients.lock().unwrap().push(client_tx);
    }
    {
        let msgs = msg_ring.lock().unwrap().msgs.clone();
        for m in msgs {
            let sendtxt = serde_json::to_string(&serde_json::json!({
                "type":"megagroup",
                "from":m.from,
                "text":m.text,
                "timestamp":m.timestamp
            })).unwrap();
            let _ = tx.send(Message::text(sendtxt)).await;
        }
    }
    let clients2 = clients.clone();
    let msg_ring2 = msg_ring.clone();
    tokio::spawn(async move {
        while let Some(Ok(msg)) = rx.next().await {
            if let Ok(txt) = msg.to_str() {
                if let Ok(mg) = serde_json::from_str::<MegaGroupMsg>(txt) {
                    if mg.text.len() > 256 { continue; }
                    let mut ring = msg_ring2.lock().unwrap();
                    if ring.push(mg.clone()) {
                        let sendtxt = serde_json::to_string(&serde_json::json!({
                            "type":"megagroup",
                            "from":mg.from,
                            "text":mg.text,
                            "timestamp":mg.timestamp
                        })).unwrap();
                        for c in clients2.lock().unwrap().iter() {
                            let _ = c.send(Message::text(&sendtxt));
                        }
                    }
                }
            }
        }
    });
    while let Some(msg) = client_rx.recv().await {
        let _ = tx.send(msg).await;
    }
}