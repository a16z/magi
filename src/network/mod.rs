use std::{time::Duration, net::SocketAddr};

use eyre::Result;
use libp2p::{
    mplex::MplexConfig,
    gossipsub::{self, IdentTopic, Event, MessageAcceptance, Message, MessageId},
    noise, ping,
    swarm::{keep_alive, NetworkBehaviour, SwarmBuilder, SwarmEvent},
    tcp, Multiaddr, PeerId, Transport,
};
use libp2p_identity::Keypair;
use futures::{prelude::*, select};
use openssl::sha::sha256;

pub mod discovery;

pub async fn run() -> Result<()> {
    let key = Keypair::generate_secp256k1();
    let transport = tcp::tokio::Transport::new(tcp::Config::default())
        .upgrade(libp2p::core::upgrade::Version::V1Lazy)
        .authenticate(noise::Config::new(&key).unwrap())
        .multiplex(MplexConfig::default())
        .boxed();

    let behaviour = {
        let keep_alive = keep_alive::Behaviour::default();
        let ping = ping::Behaviour::default();
        
        let gossipsub_config = gossipsub::ConfigBuilder::default()
            .mesh_n(8)
            .mesh_n_low(6)
            .mesh_n_high(12)
            .gossip_lazy(6)
            .heartbeat_interval(Duration::from_millis(500))
            .fanout_ttl(Duration::from_secs(24))
            .history_length(12)
            .history_gossip(3)
            .duplicate_cache_time(Duration::from_secs(65))
            .validation_mode(gossipsub::ValidationMode::None)
            .validate_messages()
            .message_id_fn(message_id)
            .build()
            .unwrap();

        println!("{:?}", gossipsub_config.protocol_id());

        let mut gossipsub = gossipsub::Behaviour::new(gossipsub::MessageAuthenticity::Anonymous, gossipsub_config).unwrap();

        let topic = IdentTopic::new("/optimism/420/0/blocks");
        gossipsub.subscribe(&topic).unwrap();

        Behaviour {
            ping,
            keep_alive,
            gossipsub,
        }
    };

    let mut swarm =
        SwarmBuilder::with_tokio_executor(transport, behaviour, PeerId::from(key.public())).build();

    swarm.listen_on("/ip4/0.0.0.0/tcp/9000".parse()?).unwrap();

    let addr = "0.0.0.0:9001".parse::<SocketAddr>()?;
    let chain_id = 420;

    let mut peer_recv = discovery::start(addr, chain_id)?;

    loop {
        select! {
            peer = peer_recv.recv().fuse() => {
                if let Some(peer) = peer {
                    let peer: Multiaddr = format!("/ip4/{}/tcp/{}", peer.ip, peer.port).parse()?;
                    swarm.dial(peer).unwrap();
                }
            },
            event = swarm.select_next_some() => {
                match event {
                    SwarmEvent::Behaviour(BehaviourEvent::Gossipsub(event)) => {
                        match event {
                            Event::Message { message_id, propagation_source, message } => {
                                swarm.behaviour_mut().gossipsub.report_message_validation_result(
                                    &message_id,
                                    &propagation_source,
                                    MessageAcceptance::Accept,
                                ).unwrap();

                                tracing::info!("data: {}", hex::encode(message.data));
                            },

                            default => tracing::info!("{:?}", default),
                        }
                    },
                    SwarmEvent::ConnectionClosed { peer_id, cause, .. } => {
                        cause.unwrap();

                    },
                    SwarmEvent::Behaviour(BehaviourEvent::Ping(_)) => {

                    },
                    default => tracing::info!("{:?}", default),
                }
            },
        }
    }

    // Ok(())
}

fn message_id(msg: &Message) -> MessageId {
    let mut decoder = snap::raw::Decoder::new();
    let id = match decoder.decompress_vec(&msg.data) {
        Ok(data) => {
            let domain_valid_snappy: Vec<u8> = vec![0x1, 0x0, 0x0, 0x0];
            sha256([domain_valid_snappy.as_slice(), data.as_slice()].concat().as_slice())[..20].to_vec()
        },
        Err(_) => {
            let domain_invalid_snappy: Vec<u8> = vec![0x0, 0x0, 0x0, 0x0];
            sha256([domain_invalid_snappy.as_slice(), msg.data.as_slice()].concat().as_slice())[..20].to_vec()
        },
    };

    MessageId(id)
}

#[derive(NetworkBehaviour)]
struct Behaviour {
    keep_alive: keep_alive::Behaviour,
    ping: ping::Behaviour,
    gossipsub: gossipsub::Behaviour,
}
