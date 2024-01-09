use discv5::Enr;
use std::{net::SocketAddr, time::Duration};

use eyre::Result;
use futures::prelude::*;
use libp2p::{
    gossipsub::{self, IdentTopic, Message, MessageId},
    mplex::MplexConfig,
    noise, ping,
    swarm::{NetworkBehaviour, SwarmBuilder, SwarmEvent},
    tcp, Multiaddr, PeerId, Swarm, Transport,
};
use libp2p_identity::{secp256k1, secp256k1::SecretKey, Keypair};
use openssl::sha::sha256;
use tokio::select;

use super::{handlers::Handler, service::types::NetworkAddress};
use crate::network::signer::Signer;

use crate::{engine::ExecutionPayload, network::handlers::block_handler::encode_block_msg};

use ethers::core::k256::ecdsa::SigningKey;

mod discovery;
mod types;

pub struct Service {
    handlers: Vec<Box<dyn Handler>>,
    addr: SocketAddr,
    chain_id: u64,
    keypair: Keypair,
    bootnodes: Option<Vec<Enr>>,
    signer: Option<Signer>,
    block_topic: IdentTopic,
}

impl Service {
    pub fn new(
        addr: SocketAddr,
        chain_id: u64,
        bootnodes: Option<Vec<Enr>>,
        keypair: Option<SecretKey>,
        sequencer_secret_key: Option<SecretKey>,
        block_topic: IdentTopic,
    ) -> Self {
        let keypair = keypair
            .map(|secp_secret_key| secp256k1::Keypair::from(secp_secret_key).into())
            .unwrap_or_else(Keypair::generate_secp256k1);

        let signer = sequencer_secret_key
            .and_then(|pk| SigningKey::from_slice(&pk.to_bytes()).ok())
            .map(|signing_key| {
                Signer::new(chain_id, signing_key, None).expect("Failed to create Signer")
            });

        Self {
            handlers: Vec::new(),
            addr,
            chain_id,
            keypair,
            bootnodes,
            signer,
            block_topic,
        }
    }

    pub fn add_handler(mut self, handler: Box<dyn Handler>) -> Self {
        self.handlers.push(handler);
        self
    }

    pub fn set_keypair(mut self, keypair: Keypair) -> Self {
        self.keypair = keypair;
        self
    }

    pub fn start(
        mut self,
        mut receiver_new_block: tokio::sync::mpsc::Receiver<ExecutionPayload>,
    ) -> Result<()> {
        let addr = NetworkAddress::try_from(self.addr)?;

        let mut swarm = create_swarm(self.keypair.clone(), &self.handlers)?;

        let mut peer_recv =
            discovery::start(addr, self.keypair.clone(), self.chain_id, self.bootnodes)?;

        let multiaddr = Multiaddr::from(addr);
        swarm
            .listen_on(multiaddr)
            .map_err(|_| eyre::eyre!("swarm listen failed"))?;

        let mut handlers = Vec::new();
        handlers.append(&mut self.handlers);

        // for p2p
        let p2p_data = self.signer.take();

        // Listening to peers
        tokio::spawn(async move {
            loop {
                select! {
                    // new peer
                    peer = peer_recv.recv().fuse() => {
                        if let Some(peer) = peer {
                            let peer = Multiaddr::from(peer);
                            _ = swarm.dial(peer);
                        }
                    },
                    // incoming blocks from peers
                    event = swarm.select_next_some() => {
                        if let SwarmEvent::Behaviour(event) = event {
                            event.handle(&mut swarm, &handlers);
                        }
                    },
                    // publish a block for peers
                    Some(payload) = receiver_new_block.recv() => {
                        if let Some(signer) = p2p_data.as_ref() {
                            match encode_block_msg(payload, signer)
                                .map_err(|err| err.to_string())
                                .and_then(|tx|{
                                    swarm.behaviour_mut().gossipsub
                                        .publish(self.block_topic.clone(),tx)
                                        .map_err(|err| err.to_string())
                                }){
                                    Ok(_message_id) => {},
                                    Err(err) => tracing::debug!("P2P block broadcast error: {err:?}"),
                                };
                        } else {
                            tracing::warn!("missed signer p2p private key; skip payload broadcast");
                        }
                    },
                    _ = tokio::signal::ctrl_c() => {
                        break;
                    }
                }
            }
        });

        Ok(())
    }
}

fn compute_message_id(msg: &Message) -> MessageId {
    let mut decoder = snap::raw::Decoder::new();
    let id = match decoder.decompress_vec(&msg.data) {
        Ok(data) => {
            let domain_valid_snappy: Vec<u8> = vec![0x1, 0x0, 0x0, 0x0];
            sha256(
                [domain_valid_snappy.as_slice(), data.as_slice()]
                    .concat()
                    .as_slice(),
            )[..20]
                .to_vec()
        }
        Err(_) => {
            let domain_invalid_snappy: Vec<u8> = vec![0x0, 0x0, 0x0, 0x0];
            sha256(
                [domain_invalid_snappy.as_slice(), msg.data.as_slice()]
                    .concat()
                    .as_slice(),
            )[..20]
                .to_vec()
        }
    };

    MessageId(id)
}

fn create_swarm(keypair: Keypair, handlers: &[Box<dyn Handler>]) -> Result<Swarm<Behaviour>> {
    let transport = tcp::tokio::Transport::new(tcp::Config::default())
        .upgrade(libp2p::core::upgrade::Version::V1Lazy)
        .authenticate(noise::Config::new(&keypair)?)
        .multiplex(MplexConfig::default())
        .boxed();

    let behaviour = Behaviour::new(handlers)?;

    Ok(
        SwarmBuilder::with_tokio_executor(transport, behaviour, PeerId::from(keypair.public()))
            .build(),
    )
}

#[derive(NetworkBehaviour)]
#[behaviour(out_event = "Event")]
struct Behaviour {
    ping: ping::Behaviour,
    gossipsub: gossipsub::Behaviour,
}

impl Behaviour {
    fn new(handlers: &[Box<dyn Handler>]) -> Result<Self> {
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
            .message_id_fn(compute_message_id)
            .build()
            .map_err(|_| eyre::eyre!("gossipsub config creation failed"))?;

        let mut gossipsub =
            gossipsub::Behaviour::new(gossipsub::MessageAuthenticity::Anonymous, gossipsub_config)
                .map_err(|_| eyre::eyre!("gossipsub behaviour creation failed"))?;

        handlers
            .iter()
            .flat_map(|handler| {
                handler
                    .topics()
                    .iter()
                    .map(|topic| {
                        let topic = IdentTopic::new(topic.to_string());
                        gossipsub
                            .subscribe(&topic)
                            .map_err(|_| eyre::eyre!("subscription failed"))
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Result<Vec<bool>>>()?;

        Ok(Self { ping, gossipsub })
    }
}

enum Event {
    Ping(ping::Event),
    Gossipsub(gossipsub::Event),
}

impl Event {
    fn handle(self, swarm: &mut Swarm<Behaviour>, handlers: &[Box<dyn Handler>]) {
        if let Self::Gossipsub(gossipsub::Event::Message {
            propagation_source,
            message_id,
            message,
        }) = self
        {
            let handler = handlers
                .iter()
                .find(|h| h.topics().contains(&message.topic));
            if let Some(handler) = handler {
                let status = handler.handle(message);

                _ = swarm
                    .behaviour_mut()
                    .gossipsub
                    .report_message_validation_result(&message_id, &propagation_source, status);
            }
        }
    }
}

impl From<ping::Event> for Event {
    fn from(value: ping::Event) -> Self {
        Event::Ping(value)
    }
}

impl From<gossipsub::Event> for Event {
    fn from(value: gossipsub::Event) -> Self {
        Event::Gossipsub(value)
    }
}
