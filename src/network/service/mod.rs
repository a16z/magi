use std::{net::SocketAddr, time::Duration};

use eyre::Result;
use futures::{prelude::*, select};
use libp2p::{
    gossipsub::{self, IdentTopic, Message, MessageId},
    mplex::MplexConfig,
    noise, ping,
    swarm::{NetworkBehaviour, SwarmBuilder, SwarmEvent},
    tcp, Multiaddr, PeerId, Swarm, Transport,
};
use libp2p_identity::Keypair;
use openssl::sha::sha256;

use super::{handlers::Handler, service::types::NetworkAddress};

mod discovery;
mod enr;
mod types;

/// Responsible for management of the `Discv5` & `libp2p` services.
pub struct Service {
    /// Handles validation & processing of inbound messages
    handlers: Vec<Box<dyn Handler>>,
    /// The socket address that the service is listening on.
    addr: SocketAddr,
    /// The chain ID of the network
    chain_id: u64,
    /// A unique keypair to validate the node's identity
    keypair: Option<Keypair>,
}

impl Service {
    /// Creates a new [Service]
    pub fn new(addr: SocketAddr, chain_id: u64) -> Self {
        Self {
            handlers: Vec::new(),
            addr,
            chain_id,
            keypair: None,
        }
    }

    /// Adds a handler to [Service]
    pub fn add_handler(mut self, handler: Box<dyn Handler>) -> Self {
        self.handlers.push(handler);
        self
    }

    /// Sets the keypair for [Service]
    pub fn set_keypair(mut self, keypair: Keypair) -> Self {
        self.keypair = Some(keypair);
        self
    }

    /// Starts the Discv5 peer discovery & libp2p services
    /// and continually listens for new peers and messages to handle
    pub fn start(mut self) -> Result<()> {
        let addr = NetworkAddress::try_from(self.addr)?;
        let keypair = self.keypair.unwrap_or_else(Keypair::generate_secp256k1);

        let mut swarm = create_swarm(keypair, &self.handlers)?;
        let mut peer_recv = discovery::start(addr, self.chain_id)?;

        let multiaddr = Multiaddr::from(addr);
        swarm
            .listen_on(multiaddr)
            .map_err(|_| eyre::eyre!("swarm listen failed"))?;

        let mut handlers = Vec::new();
        handlers.append(&mut self.handlers);

        tokio::spawn(async move {
            loop {
                select! {
                    peer = peer_recv.recv().fuse() => {
                        if let Some(peer) = peer {
                            let peer = Multiaddr::from(peer);
                            _ = swarm.dial(peer);
                        }
                    },
                    event = swarm.select_next_some() => {
                        if let SwarmEvent::Behaviour(event) = event {
                            event.handle(&mut swarm, &handlers);
                        }
                    },
                }
            }
        });

        Ok(())
    }
}

/// Computes the message ID of a `gossipsub` message
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

/// Creates the libp2p [Swarm]
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

/// Specifies the [NetworkBehaviour] of the node
#[derive(NetworkBehaviour)]
#[behaviour(out_event = "Event")]
struct Behaviour {
    /// Adds [libp2p::ping] to respond to inbound pings, and send periodic outbound pings
    ping: ping::Behaviour,
    /// Adds [libp2p::gossipsub] to enable gossipsub as the routing layer
    gossipsub: gossipsub::Behaviour,
}

impl Behaviour {
    /// Configures the swarm behaviors, subscribes to the gossip topics, and returns a new [Behaviour]
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

/// The type of message received
enum Event {
    /// Represents a [ping::Event]
    #[allow(dead_code)]
    Ping(ping::Event),
    /// Represents a [gossipsub::Event]
    Gossipsub(gossipsub::Event),
}

impl Event {
    /// Handles received gossipsub messages. Ping messages are ignored.
    /// Reports back to [libp2p::gossipsub] to apply peer scoring and forward the message to other peers if accepted.
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
    /// Converts [ping::Event] to [Event]
    fn from(value: ping::Event) -> Self {
        Event::Ping(value)
    }
}

impl From<gossipsub::Event> for Event {
    /// Converts [gossipsub::Event] to [Event]
    fn from(value: gossipsub::Event) -> Self {
        Event::Gossipsub(value)
    }
}
