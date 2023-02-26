use std::sync::Arc;

use eyre::Result;

use magi::{
    config::{ChainConfig, Config},
    derive::Pipeline,
    telemetry,
};

#[tokio::main]
async fn main() -> Result<()> {
    telemetry::init(false)?;

    let start_epoch = 8494058;
    let rpc = "https://eth-goerli.g.alchemy.com/v2/a--NIcyeycPntQX42kunxUIVkg6_ekYc";

    let config = Arc::new(Config {
        l1_rpc: rpc.to_string(),
        chain: ChainConfig::goerli(),
        max_channels: 100_000_000,
        max_timeout: 100,
    });

    let mut pipeline = Pipeline::new(start_epoch, config);

    loop {
        let attributes = pipeline.next();
        if let Some(attributes) = attributes {
            println!("{:?}", attributes);
        }
    }
}
