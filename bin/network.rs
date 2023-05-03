use eyre::Result;

use libp2p_identity::Keypair;
use magi::{
    network::service,
    telemetry,
};

#[tokio::main]
async fn main() -> Result<()> {
    let _guards = telemetry::init(false, None, None);

    let chain_id = 420;
    let keypair = Keypair::generate_secp256k1();

    service::start(chain_id, keypair).await?;

    Ok(())
}
