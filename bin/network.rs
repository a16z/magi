use alloy_primitives::address;
use eyre::Result;

use magi::{
    network::{handlers::block_handler::BlockHandler, service::Service},
    telemetry,
};
use tokio::sync::watch;

#[tokio::main]
async fn main() -> Result<()> {
    let _guards = telemetry::init(false, None, None);

    let addr = "0.0.0.0:9876".parse()?;
    let chain_id = 420;
    let (_, recv) = watch::channel(address!("715b7219d986641df9efd9c7ef01218d528e19ec"));
    let (block_handler, block_recv) = BlockHandler::new(chain_id, recv);

    Service::new(addr, chain_id)
        .add_handler(Box::new(block_handler))
        .start()?;

    while let Ok(payload) = block_recv.recv() {
        tracing::info!("received unsafe block with hash: {:?}", payload.block_hash);
    }

    Ok(())
}
