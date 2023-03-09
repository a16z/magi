//! NodeRecord is a discv4 ENR.
//!
//! This is adapted from [reth](https://github.com/paradigmxyz/reth).
//!
//! We need to hoist this out of the reth crate due to compile issues.
//! In the future, this can be replaced with [reth] types.

use crate::net::peers::PeerId;
use secp256k1::{SecretKey, SECP256K1};
use std::{
    fmt,
    fmt::Write,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    num::ParseIntError,
    str::FromStr,
};
use url::{Host, Url};

/// NodeRecord is a discv4 ENR
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct NodeRecord {
    /// The Node Address
    pub address: IpAddr,
    /// TCP port of the port that accepts connections
    pub tcp_port: u16,
    /// UDP discovery port
    pub udp_port: u16,
    /// Public key of the discovery service
    pub id: PeerId,
}

impl NodeRecord {
    /// Derive the [`NodeRecord`] from the secret key and addr
    pub fn from_secret_key(addr: SocketAddr, sk: &SecretKey) -> Self {
        let pk = secp256k1::PublicKey::from_secret_key(SECP256K1, sk);
        let id = PeerId::from_slice(&pk.serialize_uncompressed()[1..]);
        Self::new(addr, id)
    }

    /// Converts the `address` into an [`Ipv4Addr`] if the `address` is a mapped [Ipv6Addr](std::net::Ipv6Addr).
    ///
    /// Returns `true` if the address was converted.
    ///
    /// See also [std::net::Ipv6Addr::to_ipv4_mapped]
    pub fn convert_ipv4_mapped(&mut self) -> bool {
        // convert IPv4 mapped IPv6 address
        if let IpAddr::V6(v6) = self.address {
            if let Some(v4) = v6.to_ipv4_mapped() {
                self.address = v4.into();
                return true
            }
        }
        false
    }

    /// Same as [Self::convert_ipv4_mapped] but consumes the type
    pub fn into_ipv4_mapped(mut self) -> Self {
        self.convert_ipv4_mapped();
        self
    }

    /// Creates a new record from a socket addr and peer id.
    #[allow(unused)]
    pub fn new(addr: SocketAddr, id: PeerId) -> Self {
        Self { address: addr.ip(), tcp_port: addr.port(), udp_port: addr.port(), id }
    }

    /// The TCP socket address of this node
    #[must_use]
    pub fn tcp_addr(&self) -> SocketAddr {
        SocketAddr::new(self.address, self.tcp_port)
    }

    /// The UDP socket address of this node
    #[must_use]
    pub fn udp_addr(&self) -> SocketAddr {
        SocketAddr::new(self.address, self.udp_port)
    }
}

impl fmt::Display for NodeRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("enode://")?;
        hex::encode(self.id.as_bytes()).fmt(f)?;
        f.write_char('@')?;
        match self.address {
            IpAddr::V4(ip) => {
                ip.fmt(f)?;
            }
            IpAddr::V6(ip) => {
                // encapsulate with brackets
                f.write_char('[')?;
                ip.fmt(f)?;
                f.write_char(']')?;
            }
        }
        f.write_char(':')?;
        self.tcp_port.fmt(f)?;
        if self.tcp_port != self.udp_port {
            f.write_str("?discport=")?;
            self.udp_port.fmt(f)?;
        }

        Ok(())
    }
}

/// Possible error types when parsing a `NodeRecord`
#[derive(Debug, thiserror::Error)]
pub enum NodeRecordParseError {
    #[error("Failed to parse url: {0}")]
    InvalidUrl(String),
    #[error("Failed to parse id")]
    InvalidId(String),
    #[error("Failed to discport query: {0}")]
    Discport(ParseIntError),
}

impl FromStr for NodeRecord {
    type Err = NodeRecordParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let url = Url::parse(s).map_err(|e| NodeRecordParseError::InvalidUrl(e.to_string()))?;

        let address = match url.host() {
            Some(Host::Ipv4(ip)) => IpAddr::V4(ip),
            Some(Host::Ipv6(ip)) => IpAddr::V6(ip),
            Some(Host::Domain(ip)) => IpAddr::V4(
                Ipv4Addr::from_str(ip)
                    .map_err(|e| NodeRecordParseError::InvalidUrl(e.to_string()))?,
            ),
            _ => return Err(NodeRecordParseError::InvalidUrl(format!("invalid host: {url:?}"))),
        };
        let port = url
            .port()
            .ok_or_else(|| NodeRecordParseError::InvalidUrl("no port specified".to_string()))?;

        let udp_port = if let Some(discovery_port) =
            url.query_pairs().find_map(|(maybe_disc, port)| {
                if maybe_disc.as_ref() == "discport" {
                    Some(port)
                } else {
                    None
                }
            }) {
            discovery_port.parse::<u16>().map_err(NodeRecordParseError::Discport)?
        } else {
            port
        };

        let id = url
            .username()
            .parse::<PeerId>()
            .map_err(|e| NodeRecordParseError::InvalidId(e.to_string()))?;

        Ok(Self { address, id, tcp_port: port, udp_port })
    }
}