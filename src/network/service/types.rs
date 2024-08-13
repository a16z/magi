use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use discv5::enr::{CombinedKey, Enr};
use eyre::Result;
use libp2p::{multiaddr::Protocol, Multiaddr};

/// An [Ipv4Addr] and port.
#[derive(Debug, Clone, Copy)]
pub struct NetworkAddress {
    /// An [Ipv4Addr]
    pub ip: Ipv4Addr,
    /// A port
    pub port: u16,
}

/// A wrapper around a peer's Network Address.
#[derive(Debug)]
pub struct Peer {
    /// The peer's [Ipv4Addr] and port
    pub addr: NetworkAddress,
}

impl TryFrom<&Enr<CombinedKey>> for NetworkAddress {
    type Error = eyre::Report;

    /// Convert an [Enr] to a Network Address.
    fn try_from(value: &Enr<CombinedKey>) -> Result<Self> {
        let ip = value.ip4().ok_or(eyre::eyre!("missing ip"))?;
        let port = value.tcp4().ok_or(eyre::eyre!("missing port"))?;

        Ok(Self { ip, port })
    }
}

impl From<NetworkAddress> for Multiaddr {
    /// Converts a Network Address to a [Multiaddr]
    fn from(value: NetworkAddress) -> Self {
        let mut multiaddr = Multiaddr::empty();
        multiaddr.push(Protocol::Ip4(value.ip));
        multiaddr.push(Protocol::Tcp(value.port));

        multiaddr
    }
}

impl From<NetworkAddress> for SocketAddr {
    /// Converts a Network Address to a [SocketAddr].
    fn from(value: NetworkAddress) -> Self {
        SocketAddr::new(IpAddr::V4(value.ip), value.port)
    }
}

impl TryFrom<SocketAddr> for NetworkAddress {
    type Error = eyre::Report;

    /// Converts a [SocketAddr] to a Network Address.
    fn try_from(value: SocketAddr) -> Result<Self> {
        let ip = match value.ip() {
            IpAddr::V4(ip) => ip,
            IpAddr::V6(_) => eyre::bail!("ipv6 not supported"),
        };

        Ok(Self {
            ip,
            port: value.port(),
        })
    }
}

impl TryFrom<&Enr<CombinedKey>> for Peer {
    type Error = eyre::Report;

    /// Converts an [Enr] to a Peer
    fn try_from(value: &Enr<CombinedKey>) -> Result<Self> {
        let addr = NetworkAddress::try_from(value)?;
        Ok(Peer { addr })
    }
}

impl From<Peer> for Multiaddr {
    /// Converts a Peer to a [Multiaddr]
    fn from(value: Peer) -> Self {
        value.addr.into()
    }
}
