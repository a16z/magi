use std::net::SocketAddr;

use eyre::Result;

use magi::{
    network::{self, discovery},
    telemetry,
};

#[tokio::main]
async fn main() -> Result<()> {
    let _guards = telemetry::init(false, None, None);

    // get_peers().await?;

    network::run().await.unwrap();

    Ok(())
}

async fn get_peers() -> Result<()> {
    let addr = "0.0.0.0:9000".parse::<SocketAddr>()?;
    let chain_id = 420;

    let mut recv = discovery::start(addr, chain_id)?;

    while let Some(peer) = recv.recv().await {
        tracing::info!("found peer: {:?}", peer);
    }

    Ok(())
}
