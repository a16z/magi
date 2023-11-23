use std::net::SocketAddr;

use eyre::Result;
use lazy_static::lazy_static;
use prometheus_exporter::{
    prometheus::{register_int_gauge, IntGauge},
    start,
};

pub const LISTENING_AS_STR: &str = "127.0.0.1:9200";

lazy_static! {
    pub static ref FINALIZED_HEAD: IntGauge =
        register_int_gauge!("finalized_head", "finalized head number").unwrap();
    pub static ref SAFE_HEAD: IntGauge =
        register_int_gauge!("safe_head", "safe head number").unwrap();
    pub static ref SYNCED: IntGauge = register_int_gauge!("synced", "synced flag").unwrap();
}

pub fn init(binding: SocketAddr) -> Result<()> {
    start(binding)?;
    Ok(())
}
