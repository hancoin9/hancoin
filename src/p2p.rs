use libp2p::{
    core::upgrade,
    gossipsub::{Gossipsub, GossipsubConfig, GossipsubEvent, IdentTopic, MessageAuthenticity},
    identity,
    mdns::{Mdns, MdnsConfig, MdnsEvent},
    noise::{Keypair as NoiseKeypair, NoiseConfig, X25519Spec, AuthenticKeypair},
    swarm::{Swarm, SwarmEvent, Config as SwarmConfig},
    tcp::TokioTcpConfig,
    yamux::YamuxConfig,
    PeerId, Transport,
};
use std::error::Error;
use std::time::Duration;
use log::{info, warn, error};

/// 启动P2P网络
pub async fn start_p2p() -> Result<(), Box<dyn Error>> {
    // 1. 生成本地密钥和PeerId
    let id_keys = identity::Keypair::generate_ed25519();
    let peer_id = PeerId::from(id_keys.public());
    info!("Local peer ID: {:?}", peer_id);

    // 2. 生成Noise密钥用于加密
    // 将
    let noise_keys: AuthenticKeypair<_> = NoiseKeypair::<X25519Spec>::new()
        .into_authentic(&id_keys)
        .expect("Failed to generate Noise keys");
    // 替换为
    let noise_keys: AuthenticKeypair<_> = NoiseKeypair::<X25519Spec>::new()
        .into_authentic(&id_keys)
        .map_err(|e| format!("Failed to generate Noise keys: {:?}"))?;

    // 3. 构建传输层
    let transport = TokioTcpConfig::new()
        .upgrade(upgrade::Version::V1)
        .authenticate(NoiseConfig::xx(noise_keys).into_authenticated())
        .multiplex(YamuxConfig::default())
        .boxed();

    // 4. 配置gossipsub
    let gossipsub_config = GossipsubConfig::default()
        .with_max_transmit_size(1024)  // 消息大小限制
        .with_flood_publish(true);
    let mut gossipsub = Gossipsub::new(
        MessageAuthenticity::Signed(id_keys.clone()),
        gossipsub_config,
    )
    .expect("Failed to create Gossipsub");

    // 订阅主题
    let topic = IdentTopic::new("hancoin-topic");
    gossipsub.subscribe(&topic).expect("Failed to subscribe to topic");

    // 5. 配置Swarm
    let swarm_config = SwarmConfig::with_tokio_executor()
        .with_idle_connection_timeout(Duration::from_secs(30))
        .with_max_established_incoming_connections(50)
        .with_max_established_outgoing_connections(50);

    let mut swarm = Swarm::new(transport, gossipsub, peer_id, swarm_config);

    // 监听地址
    swarm.listen_on("/ip4/0.0.0.0/tcp/4001".parse()?)?;

    // 6. 事件循环
    tokio::spawn(async move {
        loop {
            match swarm.select_next_some().await {
                SwarmEvent::Behaviour(GossipsubEvent::Message { message, .. }) => {
                    // 验证消息大小
                    if message.data.len() > 1024 {
                        warn!("Received oversized message: {} bytes", message.data.len());
                        continue;
                    }
                    info!("Received P2P message: {:?}", String::from_utf8_lossy(&message.data));
                    // 这里添加消息处理逻辑
                },
                SwarmEvent::NewListenAddr { address, .. } => {
                    info!("Listening on {:?}", address);
                },
                SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                    info!("Connected to peer: {:?}", peer_id);
                },
                SwarmEvent::ConnectionClosed { peer_id, cause, .. } => {
                    info!("Disconnected from peer: {:?}, cause: {:?}", peer_id, cause);
                },
                SwarmEvent::OutgoingConnectionError { peer_id, error, .. } => {
                    warn!("Failed to connect to peer {:?}: {:?}", peer_id, error);
                },
                _ => {}
            }
        }
    });

    Ok(())
}