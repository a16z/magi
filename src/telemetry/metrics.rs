//! Prometheus Metrics Module.

use anyhow::Result;
use lazy_static::lazy_static;
use prometheus_exporter::{
    prometheus::{register_int_gauge, IntGauge},
    start,
};

lazy_static! {
     /// Tracks the block number of the most recent finalized head.
    pub static ref FINALIZED_HEAD: IntGauge =
        register_int_gauge!("finalized_head", "finalized head number").unwrap();
           /// Tracks the block number considered to be the safe head.
    pub static ref SAFE_HEAD: IntGauge =
        register_int_gauge!("safe_head", "safe head number").unwrap();
           /// Monitors if the node is fully synced
    pub static ref SYNCED: IntGauge = register_int_gauge!("synced", "synced flag").unwrap();
}

/// Starts the metrics server on port 9200
pub fn init() -> Result<()> {
    match start("0.0.0.0:9200".parse()) {
        Ok(_) => Ok(()),
        Err(e) => Err(e.into()),
    }
}
