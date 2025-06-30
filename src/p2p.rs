use libp2p::{
    core::upgrade,
    gossipsub::{
        Gossipsub, GossipsubConfig, GossipsubEvent, IdentTopic, MessageAuthenticity,
    },
    identity,
    mdns::{Mdns, MdnsConfig, MdnsEvent},
    noise::{Keypair as NoiseKeypair, NoiseConfig, X25519Spec, AuthenticKeypair},
    swarm::{Swarm, SwarmEvent},
    tcp::TokioTcpConfig,
    yamux::YamuxConfig,
    PeerId, Transport,
};
use std::error::Error;
use tokio::io::{self, AsyncBufReadExt};

pub async fn start_p2p() -> Result<(), Box<dyn Error>> {
    // 1. 生成本地密钥和 PeerId
    let id_keys = identity::Keypair::generate_ed25519();
    let peer_id = PeerId::from(id_keys.public());
    println!("本地节点 PeerId: {:?}", peer_id);

    // 2. 生成 Noise 密钥用于加密
    let noise_keys: AuthenticKeypair<_> = NoiseKeypair::<X25519Spec>::new().into_authentic(&id_keys).expect("Noise key generation failed");

    // 3. 构建传输层
    let transport = TokioTcpConfig::new()
        .upgrade(upgrade::Version::V1)
        .authenticate(NoiseConfig::xx(noise_keys).into_authenticated())
        .multiplex(YamuxConfig::default())
        .boxed();

<<<<<<< HEAD
    // 4. 配置 gossipsub
    let gossipsub_config = GossipsubConfig::default();
    let mut gossipsub = Gossipsub::new(
        MessageAuthenticity::Signed(id_keys.clone()),
        gossipsub_config,
    )
    .expect("正确创建 Gossipsub");
    let topic = IdentTopic::new("hancoin-topic");
    gossipsub.subscribe(&topic).unwrap();
=======
    // Gossipsub 配置
    let gossipsub = Gossipsub::new(
        MessageAuthenticity::Signed(id_keys.clone()),
        GossipsubConfig::default(),
    )?;
>>>>>>> 52136f2c8f82a31b56616d5e1c024b79a2512196

    // 5. 构建 mdns
    let mdns = Mdns::new(MdnsConfig::default()).await?;

    // 6. 组合行为体
    let mut swarm = Swarm::new(transport, gossipsub, peer_id, Default::default());

<<<<<<< HEAD
    // 7. 事件循环
    loop {
        tokio::select! {
            _ = io::stdin().lines().next_line() => {
                // 用户输入处理
            }
            event = swarm.select_next_some() => {
                match event {
                    SwarmEvent::Behaviour(GossipsubEvent::Message { message, .. }) => {
                        println!("收到消息: {:?}", String::from_utf8_lossy(&message.data));
                    }
                    _ => {}
                }
=======
    tokio::spawn(async move {
        loop {
            match swarm.select_next_some().await {
                SwarmEvent::Behaviour(GossipsubEvent::Message { message, .. }) => {
                    // 添加消息大小限制，避免内存占用过高
                    if message.data.len() > 1024 {
                        eprintln!("Received message too large: {} bytes", message.data.len());
                        continue;
                    }
                    println!("P2P Received: {:?}", message.data);
                }
                SwarmEvent::NewListenAddr { address, .. } => {
                    println!("Listening on {:?}", address);
                }
                SwarmEvent::ConnectionClosed { peer_id, cause, .. } => {
                    println!("Connection closed with peer {:?}, cause: {:?}", peer_id, cause);
                }
                err @ SwarmEvent::Behaviour(_) => {
                    eprintln!("P2P Swarm error: {:?}", err);
                }
                _ => {}
>>>>>>> 52136f2c8f82a31b56616d5e1c024b79a2512196
            }
        }
    }
}