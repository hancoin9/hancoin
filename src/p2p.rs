use libp2p::{
    identity, PeerId, gossipsub::{self, Gossipsub, MessageAuthenticity, GossipsubEvent, IdentTopic}, 
    Multiaddr, Swarm, swarm::SwarmEvent, futures::StreamExt,
};
use std::error::Error;
use tokio::sync::mpsc::{Sender, Receiver};

// 极速极简P2P同步
pub async fn run_p2p_node(topic: String, tx: Sender<Vec<u8>>, mut rx: Receiver<Vec<u8>>) -> Result<(), Box<dyn Error>> {
    let id_keys = identity::Keypair::generate_ed25519();
    let peer_id = PeerId::from(id_keys.public());
    let mut gossipsub = Gossipsub::new(
        MessageAuthenticity::Signed(id_keys.clone()),
        gossipsub::Config::default(),
    )?;
    let topic_obj = IdentTopic::new(topic.clone());
    gossipsub.subscribe(&topic_obj)?;

    let transport = libp2p::development_transport(id_keys.clone()).await?;
    let mut swarm = Swarm::with_tokio_executor(transport, gossipsub, peer_id);

    Swarm::listen_on(&mut swarm, "/ip4/0.0.0.0/tcp/0".parse::<Multiaddr>()?)?;

    loop {
        tokio::select! {
            Some(b) = rx.recv() => {
                swarm.behaviour_mut().publish(topic_obj.clone(), b)?;
            }
            ev = swarm.select_next_some() => {
                if let SwarmEvent::Behaviour(GossipsubEvent::Message { message, .. }) = ev {
                    let _ = tx.send(message.data).await;
                }
            }
        }
    }
}