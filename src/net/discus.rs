//! Discus
//!
//! A discovery manager wrapping a discovery service.




/// Discus wraps the discovery protocol.
/// It provides a simple interface to work with discovered nodes.
#[derive(Debug)]
pub struct Discus {
    /// All nodes discovered via discovery protocol.
    ///
    /// These nodes can be ephemeral and are updated via the discovery protocol.
    discovered_nodes: HashMap<PeerId, SocketAddr>,
    /// Local ENR of the discovery service.
    local_enr: NodeRecord,
    /// Handler to interact with the Discovery v4 service
    discv4: Option<Discv4>,
    /// All KAD table updates from the discv4 service.
    discv4_updates: Option<ReceiverStream<DiscoveryUpdate>>,
    /// The handle to the spawned discv4 service
    _discv4_service: Option<JoinHandle<()>>,
    /// Handler to interact with the DNS discovery service
    _dns_discovery: Option<DnsDiscoveryHandle>,
    /// Updates from the DNS discovery service.
    dns_discovery_updates: Option<ReceiverStream<DnsNodeRecordUpdate>>,
    /// The handle to the spawned DNS discovery service
    _dns_disc_service: Option<JoinHandle<()>>,
    /// Events buffered until polled.
    queued_events: VecDeque<DiscoveryEvent>,
}

impl Discus {
    /// Spawns an asynchronous [`reth_discv4::Discv4Service`],
    /// listening to all discovered nodes through a channel.
    pub async fn new(
        discovery_addr: SocketAddr,
        sk: SecretKey,
        discv4_config: Option<Discv4Config>,
        dns_discovery_config: Option<DnsDiscoveryConfig>,
    ) -> Result<Self, NetworkError> {
        // Discv4 Setup
        let local_enr = NodeRecord::from_secret_key(discovery_addr, &sk);
        let (discv4, discv4_updates, _discv4_service) = if let Some(disc_config) = discv4_config {
            let (discv4, mut discv4_service) =
                Discv4::bind(discovery_addr, local_enr, sk, disc_config)
                    .await
                    .map_err(NetworkError::Discovery)?;
            let discv4_updates = discv4_service.update_stream();
            // spawn the service
            let _discv4_service = discv4_service.spawn();
            (Some(discv4), Some(discv4_updates), Some(_discv4_service))
        } else {
            (None, None, None)
        };

        // setup DNS discovery
        let (_dns_discovery, dns_discovery_updates, _dns_disc_service) =
            if let Some(dns_config) = dns_discovery_config {
                let (mut service, dns_disc) = DnsDiscoveryService::new_pair(
                    Arc::new(DnsResolver::from_system_conf()?),
                    dns_config,
                );
                let dns_discovery_updates = service.node_record_stream();
                let dns_disc_service = service.spawn();
                (Some(dns_disc), Some(dns_discovery_updates), Some(dns_disc_service))
            } else {
                (None, None, None)
            };

        Ok(Self {
            local_enr,
            discv4,
            discv4_updates,
            _discv4_service,
            discovered_nodes: Default::default(),
            queued_events: Default::default(),
            _dns_disc_service,
            _dns_discovery,
            dns_discovery_updates,
        })
    }

    /// Bans the [`IpAddr`] in the discovery service.
    pub(crate) fn ban_ip(&self, ip: IpAddr) {
        if let Some(discv4) = &self.discv4 {
            discv4.ban_ip(ip)
        }
    }

    /// Bans the [`PeerId`] and [`IpAddr`] in the discovery service.
    pub(crate) fn ban(&self, peer_id: PeerId, ip: IpAddr) {
        if let Some(discv4) = &self.discv4 {
            discv4.ban(peer_id, ip)
        }
    }

    /// Returns the id with which the local identifies itself in the network
    pub(crate) fn local_id(&self) -> PeerId {
        self.local_enr.id
    }

}