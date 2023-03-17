use std::net::IpAddr;
use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::Arc;

use clap::Parser;
use clap::Subcommand;
use discv5::IpMode;
use ethers_core::types::H256;
use eyre::Result;

use discv5::{enr::*, Discv5, Discv5ConfigBuilder, Discv5Event};

use magi::net::bootnodes;
use magi::{
    net::{keys, stats},
    telemetry,
};

#[tokio::main]
async fn main() -> Result<()> {
    telemetry::init(true, Some("peers".to_string()))?;
    telemetry::register_shutdown();

    let server = Cli::parse();
    let addr = server
        .listen_address
        .parse::<IpAddr>()
        .map_err(|_| eyre::eyre!("Invalid listening address"))?;
    let port = server.listen_port;
    tracing::info!(target: "peers", "Starting peers server on {}:{}", addr, port);

    // The number of nodes required to come to consensus before our external IP is updated.
    let peer_update_min = server.peer_update_min;
    tracing::info!(target: "peers", "Peer update minimum: {}", peer_update_min);

    // Build the ENR
    let enr_key = keys::generate(server.static_key, server.secp256k1_key.clone())?;
    let enr = server.build(&enr_key)?;
    tracing::info!(target: "peers", "Constructed enr: {:?}", enr);

    let connect_enr = server
        .enr
        .as_ref()
        .map(|enr| {
            enr.parse::<Enr<CombinedKey>>()
                .map_err(|_| eyre::eyre!("Invalid base64 encoded ENR"))
        })
        .transpose()?;
    tracing::info!(target: "peers", "Connect ENR: {:?}", connect_enr);

    // Build the discv5 server using a default config
    let peer_update_min = peer_update_min
        .try_into()
        .map_err(|_| eyre::eyre!("Invalid peer update min"))?;
    let config = Discv5ConfigBuilder::new()
        .enr_peer_update_min(peer_update_min)
        .ip_mode(IpMode::Ip4)
        .build();
    tracing::info!(target: "peers", "Built Discv5 Config: {:?}", config);
    let mut discv5 = Discv5::new(enr, enr_key, config)
        .map_err(|_| eyre::eyre!("Failed to construct discv5 server"))?;

    // Connect to an ENR if allowed to search for p2p connections
    if !server.no_search {
        if let Some(connect_enr) = connect_enr {
            tracing::info!(
                "Connecting to ENR. ip: {:?}, udp_port: {:?},  tcp_port: {:?}",
                connect_enr.ip4(),
                connect_enr.udp4(),
                connect_enr.tcp4()
            );
            if let Err(e) = discv5.add_enr(connect_enr) {
                tracing::warn!("ENR not added: {:?}", e);
            }
        }
    }

    let op_nodes = bootnodes::optimism_mainnet_nodes();
    tracing::debug!(target: "peers", "Bootstrapping nodes: {:?}", op_nodes);

    // Use predefined bootnodes to bootstrap the peernet
    let op_enrs = bootnodes::optimism_mainnet_enrs();
    tracing::debug!(target: "peers", "Bootstrapping with {} nodes", op_nodes.len());
    for enr in op_enrs {
        match discv5.add_enr(enr) {
            Ok(_) => tracing::debug!(target: "peers", "Bootstrapped node"),
            Err(e) => tracing::warn!(target: "peers", "Failed to bootstrap node: {:?}", e),
        }
    }

    // Start the discv5 server
    discv5
        .start(SocketAddr::new(addr, port))
        .await
        .map_err(|_| eyre::eyre!("Should be able to start the server"))?;
    tracing::info!(target: "peers", "Server listening on {addr}:{port}");

    let server_ref = Arc::new(discv5);
    if server.stats > 0 {
        stats::run(Arc::clone(&server_ref), None, server.stats);
    }

    if server.no_search {
        tracing::info!(target: "peers", "Running without query service, press CTRL-C to exit.");
        let _ = tokio::signal::ctrl_c().await;
        std::process::exit(0);
    }

    tracing::info!(target: "peers", "Subcommand: {:?}", server.subcommand);

    match server.subcommand {
        Some(SubCommand::Blocks { block_hash }) => {
            let block_hash = if let "latest" = block_hash.as_str() {
                // TODO: Call client for latest block hash
                H256::zero()
            } else {
                H256::from_str(&block_hash)?
            };
            tracing::debug!(target: "peers", "Block hash: {:?}", block_hash);

            tracing::debug!(target: "peers", "TODO: Implement blocks");
            Ok(())
        }
        None => server.events(server_ref).await,
    }
}

impl Cli {
    /// Starts the server and listens for events.
    pub async fn events(&self, disc: Arc<Discv5>) -> Result<()> {
        // Listen to all incoming events
        tracing::info!(target: "peers", "Listening to all incoming events...");
        let mut event_stream = disc.event_stream().await.unwrap();
        loop {
            match event_stream.recv().await {
                Some(Discv5Event::SocketUpdated(addr)) => {
                    tracing::info!(target: "peers", "Nodes ENR socket address has been updated to: {:?}", addr);
                }
                Some(Discv5Event::Discovered(enr)) => {
                    tracing::info!(target: "peers", "A peer has been discovered: {}", enr.node_id());
                }
                Some(discv5::Discv5Event::EnrAdded { enr, .. }) => {
                    tracing::info!(
                        target: "peers",
                        "A peer has been added to the routing table with enr: {}",
                        enr
                    );
                }
                Some(discv5::Discv5Event::NodeInserted { node_id, .. }) => {
                    tracing::info!(
                        target: "peers",
                        "A peer has been added to the routing table with node_id: {}",
                        node_id
                    );
                }
                Some(discv5::Discv5Event::SessionEstablished(enr, addr)) => {
                    tracing::info!(
                        target: "peers",
                        "A session has been established with peer: {} at address: {}",
                        enr,
                        addr
                    );
                }
                Some(discv5::Discv5Event::TalkRequest(talk_request)) => {
                    tracing::info!(
                        target: "peers",
                        "A talk request has been received from peer: {}",
                        talk_request.node_id()
                    );
                }
                _ => {}
            }
        }
    }

    /// Builds an [`discv5::enr::Enr`] from the peers CLI arguments and a provided [`discv5::enr::CombinedKey`].
    pub fn build(&self, enr_key: &CombinedKey) -> Result<Enr<CombinedKey>> {
        let mut builder = EnrBuilder::new("v4");
        let addr = self
            .listen_address
            .parse::<IpAddr>()
            .map_err(|_| eyre::eyre!("Invalid listening address"))?;
        let port = self.listen_port;

        // if the -w switch is used, use the listen_address and port for the ENR
        match &self.enr_address {
            Some(address_string) => {
                let enr_address = address_string
                    .parse::<IpAddr>()
                    .map_err(|_| eyre::eyre!("Invalid enr-address"))?;
                let _ = builder.ip(enr_address);
            }
            None => {
                let _ = builder.ip(addr);
            }
        }
        match self.enr_port {
            Some(enr_port) => builder.udp4(enr_port),
            None => builder.udp4(port),
        };
        tracing::debug!(target: "peers", "ENR address: {}:{}", addr, port);

        // Set the server sequence number.
        if let Some(seq_no_string) = &self.enr_seq_no {
            let seq_no = seq_no_string
                .parse::<u64>()
                .map_err(|_| eyre::eyre!("Invalid sequence number, must be a uint"))?;
            builder.seq(seq_no);
        }
        tracing::debug!(target: "peers", "Sequence Number: {:?}", self.enr_seq_no);

        // Set the eth2 enr field.
        if let Some(eth2_string) = &self.enr_eth2 {
            let ssz_bytes =
                hex::decode(eth2_string).map_err(|_| eyre::eyre!("Invalid eth2 hex bytes"))?;
            builder.add_value("eth2", &ssz_bytes);
        }

        // Build
        let enr = builder.build(enr_key)?;
        tracing::info!(target: "peers", "Built ENR: {}", enr.to_base64());

        // If the ENR is useful print it
        tracing::info!("Node Id: {:?}", enr.node_id());
        if enr.udp4_socket().is_some() {
            tracing::info!(target: "peers", "Base64 ENR: {}", enr.to_base64());
            tracing::info!(
                target: "peers",
                "ip: {}, udp port:{}",
                enr.ip4().ok_or(eyre::eyre!("Missing ipv4 address"))?,
                enr.udp4().ok_or(eyre::eyre!("Missing udp4 address"))?
            );
        } else {
            tracing::warn!(target: "peers", "ENR is not printed as no IP:PORT was specified");
        }

        Ok(enr)
    }
}

/// The CLI for magi peering
#[derive(Parser, Debug, Clone)]
pub struct Cli {
    /// Specifies the listening address of the server.
    #[clap(
        short = 'l',
        long = "listen-address",
        help = "Specifies the listening address of the server.",
        default_value = "0.0.0.0"
    )]
    pub listen_address: String,
    /// Specifies the listening UDP port of the server.
    #[clap(
        short = 'p',
        long = "listen-port",
        help = "Specifies the listening UDP port of the server.",
        default_value = "9000"
    )]
    pub listen_port: u16,
    /// Specifies the IP address of the ENR record. Not specifying this results in an ENR with no IP field, unless the -w switch is used.
    #[clap(
        short = 'i',
        long = "enr-address",
        help = "Specifies the IP address of the ENR record. Not specifying this results in an ENR with no IP field, unless the -w switch is used."
    )]
    pub enr_address: Option<String>,
    /// Specifies the UDP port of the ENR record. Not specifying this results in an ENR with no UDP field, unless the -w switch is used.
    #[clap(
        short = 'u',
        long = "enr-port",
        help = "Specifies the UDP port of the ENR record. Not specifying this results in an ENR with no UDP field, unless the -w switch is used."
    )]
    pub enr_port: Option<u16>,
    /// Specifies the ENR sequence number when creating the ENR.
    #[clap(
        short = 'q',
        long = "enr-seq-no",
        help = "Specifies the ENR sequence number when creating the ENR."
    )]
    pub enr_seq_no: Option<String>,
    /// Specifies the Eth2 field as ssz encoded hex bytes.
    #[clap(
        short = 'd',
        long = "enr-eth2",
        help = "Specifies the Eth2 field as ssz encoded hex bytes."
    )]
    pub enr_eth2: Option<String>,
    /// The Enr IP address and port will be the same as the specified listening address and port.
    #[clap(
        short = 'w',
        long = "enr-default",
        help = "The Enr IP address and port will be the same as the specified listening address and port."
    )]
    pub enr_default: bool,
    /// Use a fixed static key (hard-coded). This is primarily for debugging.
    #[clap(
        short = 'k',
        long = "static-key",
        help = "Use a fixed static key (hard-coded). This is primarily for debugging."
    )]
    pub static_key: bool,
    /// Specify a secp256k1 private key (hex encoded) to use for the nodes identity.
    #[clap(
        short = 't',
        long = "secp256k1-key",
        help = "Specify a secp256k1 private key (hex encoded) to use for the nodes identity."
    )]
    pub secp256k1_key: Option<String>,
    /// A base64 ENR that this node will initially connect to.
    #[clap(
        short = 'e',
        long = "enr",
        allow_hyphen_values = true,
        help = "A base64 ENR that this node will initially connect to."
    )]
    pub enr: Option<String>,
    /// The minimum number of peers required to update the IP address. Cannot be less than 2.
    #[clap(
        short = 'n',
        long = "peer-update-min",
        help = "The minimum number of peers required to update the IP address. Cannot be less than 2.",
        default_value = "2"
    )]
    pub peer_update_min: u64,
    /// The time to wait between successive searches. Default is 10 seconds.
    #[clap(
        short = 'b',
        long = "break-time",
        help = "The time to wait between successive searches. Default is 10 seconds.",
        default_value = "10"
    )]
    pub break_time: u64,
    /// Displays statistics on the local routing table.
    #[clap(
        short = 's',
        long = "stats",
        help = "Displays statistics on the local routing table.",
        default_value = "10"
    )]
    pub stats: u64,
    /// Prevents the server from doing any peer searches.
    #[clap(
        short = 'x',
        long = "no-search",
        help = "Prevents the server from doing any peer searches."
    )]
    pub no_search: bool,
    /// Bootstraps the server peers
    #[clap(
        short = 'o',
        long = "bootstrap",
        help = "Bootstraps the server peers from a specified file."
    )]
    pub bootstrap: Option<String>,

    /// Peer Subcommands
    #[clap(subcommand)]
    pub subcommand: Option<SubCommand>,
}

/// The subcommands for the CLI
#[derive(Subcommand, Debug, Clone)]
pub enum SubCommand {
    /// Fetch Blocks from through the p2p network
    #[clap(name = "blocks")]
    Blocks {
        /// Block Hash
        #[clap(short = 'a', long = "block-hash", default_value = "latest")]
        block_hash: String,
    },
}
