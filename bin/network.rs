use eyre::Result;

use futures::future;
use magi::{
    network::{handlers::block_handler::BlockHandler, service::ServiceBuilder},
    telemetry,
};

#[tokio::main]
async fn main() -> Result<()> {
    let _guards = telemetry::init(false, None, None);

    let addr = "0.0.0.0:9876".parse()?;
    let chain_id = 420;
    let block_handler = BlockHandler::new(chain_id);

    ServiceBuilder::new(addr, chain_id)
        .add_handler(Box::new(block_handler))
        .start()?;

    future::pending::<()>().await;

    Ok(())
}
