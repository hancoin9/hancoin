use libp2p::{
    core::upgrade,
    gossipsub::{self, Gossipsub, GossipsubConfig, GossipsubEvent, IdentTopic, MessageAuthenticity},
    identity, noise, tcp, yamux, PeerId, Swarm, Transport,
};
use libp2p::swarm::SwarmEvent;
use std::error::Error;

pub async fn build_swarm() -> Result<Swarm<Gossipsub>, Box<dyn Error>> {
    // 生成密钥
    let id_keys = identity::Keypair::generate_ed25519();
    let peer_id = PeerId::from(id_keys.public());

    // 构建加密Transport
    let transport = tcp::tokio::Transport::new(tcp::Config::default().nodelay(true))
        .upgrade(upgrade::Version::V1)
        .authenticate(noise::Config::new(&id_keys)?)
        .multiplex(yamux::Config::default())
        .boxed();

    // Gossipsub 配置
    let gossipsub_config = GossipsubConfig::default();
    let mut gossipsub = Gossipsub::new(
        MessageAuthenticity::Signed(id_keys.clone()),
        gossipsub_config,
    )?;

    // 订阅主题
    let topic = IdentTopic::new("hancoin-megagroup");
    gossipsub.subscribe(&topic)?;

    // 构建 Swarm
    let mut swarm = Swarm::with_tokio_executor(transport, gossipsub, peer_id);

    // 事件监听（后台任务）
    tokio::spawn(async move {
        loop {
            match swarm.select_next_some().await {
                SwarmEvent::Behaviour(GossipsubEvent::Message { message, .. }) => {
                    println!("P2P Received: {:?}", message.data);
                }
                _ => {}
            }
        }
    });

    Ok(swarm)
}