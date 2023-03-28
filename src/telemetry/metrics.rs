use eyre::Result;
use lazy_static::lazy_static;
use prometheus_exporter::{prometheus::{IntGauge, register_int_gauge}, start};

lazy_static! {
    pub static ref FINALIZED_HEAD: IntGauge = register_int_gauge!("finalized_head", "finalized head number").unwrap();
    pub static ref SAFE_HEAD: IntGauge = register_int_gauge!("safe_head", "safe head number").unwrap();
}

pub fn init() -> Result<()> {
    start("0.0.0.0:9200".parse().unwrap())?;
    Ok(())
}
