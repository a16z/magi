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
    let config = Arc::new(Config {
        base_chain_rpc: "https://eth-goerli.g.alchemy.com/v2/a--NIcyeycPntQX42kunxUIVkg6_ekYc"
            .to_string(),
        chain: ChainConfig::goerli(),
    });

    let mut pipeline = Pipeline::new(start_epoch, config);

    loop {
        let attributes = pipeline.next();
        if let Some(attributes) = attributes {
            println!("{:?}", attributes);
        }
    }
}
