use libp2p::{
    core::upgrade,
    gossipsub::{self, ConfigBuilder, IdentTopic, MessageAuthenticity, Behaviour as Gossipsub, Event as GossipsubEvent},
    swarm::SwarmBuilder,
    identity::{self, Keypair, PublicKey},
    noise::{self, Config as NoiseConfig, Keypair as NoiseKeypair, X25519Spec},
    swarm::{Swarm, SwarmEvent, Config as SwarmConfig, NetworkBehaviour},
    tcp::tokio::Transport as TokioTcpTransport,
    yamux::Config as YamuxConfig,
    PeerId, Transport,
};
use crate::tor::{TorConfig, TorConnector};
use ed25519_dalek::{Signature, Signer, Verifier};
use std::time::{SystemTime, UNIX_EPOCH};
use std::error::Error;
use std::time::{Duration, Instant};
use log::{info, warn, error, debug};
use serde::{Serialize, Deserialize};
use bincode::{serialize, deserialize};
use parking_lot::Mutex;
use std::sync::Arc;
use std::collections::HashMap;
use governor::{Quota, RateLimiter};
use nonzero_ext::nonzero;

/// 优化的P2P网络配置
#[derive(Clone)]
pub struct P2PConfig {
    pub max_message_size: usize,
    pub max_connections: u32,
    pub message_rate_limit: u32, // 消息/秒
    pub peer_timeout: Duration,
    /// Tor网络配置
    pub tor_config: TorConfig,
}

impl Default for P2PConfig {
    fn default() -> Self {
        Self {
            max_message_size: 1024,
            max_connections: 100,
            message_rate_limit: 10,
            peer_timeout: Duration::from_secs(30),
            tor_config: TorConfig::default(),
        }
    }
}

/// 优化的P2P网络状态
#[derive(Default)]
struct P2PState {
    active_peers: HashMap<PeerId, Instant>,
    message_count: usize,
    last_message_time: Option<Instant>,
}

/// 启动优化的P2P网络
pub async fn start_p2p(config: Option<P2PConfig>) -> Result<(), Box<dyn Error>> {
    let config = config.unwrap_or_default();
    
    // 1. 生成本地密钥和PeerId
    let id_keys = identity::Keypair::generate_ed25519();
    let peer_id = PeerId::from(id_keys.public());
    info!("Starting P2P node with ID: {:?}", peer_id);
    
    // 初始化速率限制器
    let rate_limiter = Arc::new(RateLimiter::direct(Quota::per_second(nonzero!(config.message_rate_limit))));

    // 2. 生成Noise密钥
    let noise_keys = NoiseKeypair::<X25519Spec>::new()
        .into_authenticated(&id_keys)
        .map_err(|e| format!("Failed to generate Noise keys: {:?}", e))?;
    
    // 初始化P2P状态
    let state = Arc::new(Mutex::new(P2PState::default()));

    // 3. 构建优化的传输层，支持Tor
    let transport = {
        // 创建Tor连接器
        let tor_connector = if config.tor_config.enabled {
            info!("启用Tor网络连接，代理地址: {}", config.tor_config.proxy_addr);
            Some(Arc::new(TorConnector::new(config.tor_config.clone())))
        } else {
            None
        };
        
        // 创建TCP传输
        let tcp_config = libp2p::tcp::Config::default()
            .nodelay(true) // 启用TCP_NODELAY减少延迟
            .reuse_address(true) // 允许地址重用
            .listen_backlog(128); // 增加监听队列大小
            
        let tcp = if let Some(tor) = tor_connector {
            // 使用自定义的TCP传输，通过Tor连接
            let tor_clone = tor.clone();
            TokioTcpTransport::new(tcp_config)
                .map(move |socket, addr| {
                    let addr_str = match addr {
                        libp2p::core::ConnectedPoint::Dialer { address, .. } => address.to_string(),
                        libp2p::core::ConnectedPoint::Listener { .. } => "listener".to_string(),
                    };
                    
                    if TorConnector::is_onion_address(&addr_str) {
                        debug!("通过Tor连接到.onion地址: {:?}", addr);
                        Box::pin(async move {
                            let stream = tor_clone.connect(&addr_str).await?;
                            Ok((stream, addr))
                        }) as std::pin::Pin<Box<dyn std::future::Future<Output = Result<(tokio::net::TcpStream, std::net::SocketAddr), std::io::Error>> + Send>>
                    } else if tor_clone.is_enabled() {
                        // 如果启用了Tor，所有连接都通过Tor
                        debug!("通过Tor连接到地址: {:?}", addr);
                        Box::pin(async move {
                            let stream = tor_clone.connect(&addr_str).await?;
                            Ok((stream, addr))
                        }) as std::pin::Pin<Box<dyn std::future::Future<Output = Result<(tokio::net::TcpStream, std::net::SocketAddr), std::io::Error>> + Send>>
                    } else {
                        // 直接连接
                        Box::pin(async move {
                            let stream = tokio::net::TcpStream::connect(addr).await?;
                            Ok((stream, addr))
                        }) as std::pin::Pin<Box<dyn std::future::Future<Output = Result<(tokio::net::TcpStream, std::net::SocketAddr), std::io::Error>> + Send>>
                    }
                })
        } else {
            // 标准TCP传输
            TokioTcpTransport::new(tcp_config)
        };
        
        tcp.upgrade(upgrade::Version::V1)
            .authenticate(
                NoiseConfig::new(noise_keys)
                    .into_authenticated()
                    .with_remote_peer_id_verification(true) // 强制验证远程PeerId
            )
            .multiplex(YamuxConfig::default())
            .timeout(Duration::from_secs(10)) // 添加超时
            .boxed()
    };

    // 4. 配置优化的gossipsub
    let gossipsub_config = ConfigBuilder::default()
        .max_transmit_size(config.max_message_size)
        .validation_mode(gossipsub::ValidationMode::Strict) // 使用Strict验证模式
        .peer_score_params(Default::default()) // 启用对等节点评分
        .flood_publish(true)
        .message_id_fn(|message| {
            // 使用更安全的消息ID生成
            let mut hasher = blake3::Hasher::new();
            hasher.update(&message.source.as_ref().map(|p| p.to_bytes()).unwrap_or_default());
            hasher.update(&message.data);
            hasher.update(&message.sequence_number.unwrap_or_default().to_be_bytes());
            gossipsub::MessageId::from(hasher.finalize().as_bytes()[..32].to_vec())
        })
        .build()
        .expect("Failed to build Gossipsub config");

    let mut gossipsub = Gossipsub::new_with_message_authenticity(
        MessageAuthenticity::Signed(id_keys.clone()),
        gossipsub_config,
    )
    .expect("Failed to create Gossipsub");

    // 订阅主题
    let topic = IdentTopic::new("hancoin-topic-v2"); // 使用版本化主题
    gossipsub.subscribe(&topic).expect("Failed to subscribe to topic");
    
    // 添加消息验证回调
    gossipsub.set_message_validator(|_, message| {
        // 检查消息大小
        if message.data.len() > config.max_message_size {
            warn!("Rejected oversized message: {} bytes", message.data.len());
            return false;
        }
        
        // 检查消息速率
        if rate_limiter.check().is_err() {
            warn!("Message rate limit exceeded");
            return false;
        }
        
        true
    });

    // 5. 构建优化的Swarm
    let mut swarm = {
        let behaviour = gossipsub;
        let swarm_config = libp2p::swarm::Config::with_tokio_executor()
            .with_idle_connection_timeout(config.peer_timeout)
            .with_max_established_incoming_connections(config.max_connections)
            .with_max_established_outgoing_connections(config.max_connections)
            .with_dial_concurrency_factor(4)  // 增加并发拨号数
            .with_notification_buffer_size(32)  // 增加通知缓冲区大小
            .with_pending_connection_timeout(Duration::from_secs(10))  // 减少挂起连接的超时时间
            .with_max_negotiating_inbound_streams(8)  // 增加最大协商入站流
            .with_max_negotiating_outbound_streams(8)  // 增加最大协商出站流
            .with_connection_event_buffer_size(64);  // 增加连接事件缓冲区大小
        
        SwarmBuilder::with_tokio_executor(transport, behaviour, peer_id)
            .config(swarm_config)
            .build()
    };

    // 监听多个地址
    swarm.listen_on("/ip4/0.0.0.0/tcp/4001".parse()?)?;
    swarm.listen_on("/ip6/::/tcp/4001".parse()?)?;  // 添加IPv6支持

    // 6. 优化的事件循环
    tokio::spawn(async move {
        let state_clone = state.clone();
        
        loop {
            match swarm.next().await {
                Some(SwarmEvent::Behaviour(GossipsubEvent::Message { 
                    propagation_source: _,
                    message_id: _,
                    message,
                })) => {
                    // 更新状态
                    let mut state = state_clone.lock();
                    state.message_count += 1;
                    state.last_message_time = Some(Instant::now());
                    
                    // 处理消息
                    if let Ok(msg) = deserialize::<P2PMessage>(&message.data) {
                        debug!("Received valid P2P message: {:?}", msg);
                        // 这里添加消息处理逻辑
                    } else {
                        warn!("Received invalid P2P message");
                    }
                },
                Some(SwarmEvent::NewListenAddr { address, .. }) => {
                    info!("Listening on {:?}", address);
                },
                Some(SwarmEvent::ConnectionEstablished { peer_id, .. }) => {
                    info!("Connected to peer: {:?}", peer_id);
                    state_clone.lock().active_peers.insert(peer_id, Instant::now());
                },
                Some(SwarmEvent::ConnectionClosed { peer_id, cause, .. }) => {
                    info!("Disconnected from peer: {:?}, cause: {:?}", peer_id, cause);
                    state_clone.lock().active_peers.remove(&peer_id);
                },
                Some(SwarmEvent::OutgoingConnectionError { peer_id, error, .. }) => {
                    warn!("Failed to connect to peer {:?}: {:?}", peer_id, error);
                },
                Some(SwarmEvent::IncomingConnectionError { error, .. }) => {
                    warn!("Incoming connection error: {:?}", error);
                },
                Some(_) => {},
                None => {
                    // 处理None情况，可能是连接已关闭
                    warn!("Swarm stream returned None, connection may be closed");
                }
            }
        }
    });
    
    // 添加定期清理任务
    tokio::spawn(async move {
        let state = state.clone();
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        
        loop {
            interval.tick().await;
            
            // 清理不活跃的对等节点
            let mut state = state.lock();
            let now = Instant::now();
            state.active_peers.retain(|_, last_seen| {
                now.duration_since(*last_seen) < config.peer_timeout
            });
            
            debug!("Active peers: {}, Total messages: {}", 
                  state.active_peers.len(), state.message_count);
        }
    });

    Ok(())
}

/// 优化的P2P消息结构
#[derive(Serialize, Deserialize, Debug)]
pub struct P2PMessage {
    pub version: u8,
    pub timestamp: u64,
    pub payload: Vec<u8>,
    pub signature: Vec<u8>,
}

impl P2PMessage {
    pub fn new(payload: Vec<u8>) -> Self {
        Self {
            version: 1,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            payload,
            signature: Vec::new(),
        }
    }
    
    pub fn sign(&mut self, keypair: &Keypair) -> Result<(), Box<dyn Error>> {
        let mut data = serialize(&self.payload)?;
        data.extend(self.timestamp.to_be_bytes());
        
        // 使用libp2p内置方法进行签名
        let signature = keypair.sign(&data);
        self.signature = signature;
        Ok(())
    }
    
    pub fn verify(&self, public_key: &PublicKey) -> Result<(), Box<dyn Error>> {
        let mut data = serialize(&self.payload)?;
        data.extend(self.timestamp.to_be_bytes());
        
        if !public_key.verify(&data, &self.signature) {
            return Err("Signature verification failed".into());
        }
        Ok(())
    }
}