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

    // 4. 配置 gossipsub
    let gossipsub_config = GossipsubConfig::default();
    let mut gossipsub = Gossipsub::new(
        MessageAuthenticity::Signed(id_keys.clone()),
        gossipsub_config,
    )
    .expect("正确创建 Gossipsub");
    let topic = IdentTopic::new("hancoin-topic");
    gossipsub.subscribe(&topic).unwrap();

    // 5. 构建 mdns
    let mdns = Mdns::new(MdnsConfig::default()).await?;

    // 6. 组合行为体
    let mut swarm = Swarm::new(transport, gossipsub, peer_id, Default::default());

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
            }
        }
    }
}