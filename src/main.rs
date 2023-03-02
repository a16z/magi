use std::sync::Arc;

use eyre::Result;

use magi::{
    config::{ChainConfig, Config},
    derive::Pipeline,
    driver::Driver,
    engine::EngineApi,
    telemetry,
};

#[tokio::main]
async fn main() -> Result<()> {
    telemetry::init(false)?;

    let rpc = "https://eth-goerli.g.alchemy.com/v2/a--NIcyeycPntQX42kunxUIVkg6_ekYc";
    let l2_rpc = "TODO";

    let config = Arc::new(Config {
        l1_rpc: rpc.to_string(),
        l2_rpc: Some(l2_rpc.to_string()),
        chain: ChainConfig::goerli(),
        max_channels: 100_000_000,
        max_timeout: 100,
    });

    let pipeline = Pipeline::new(config.chain.l1_start_epoch.number, config.clone())?;

    let engine_url = "http://127.0.0.1:8551".to_string();
    let jwt_secret = "bf549f5188556ce0951048ef467ec93067bc4ea21acebe46ef675cd4e8e015ff".to_string();
    let engine = EngineApi::new(engine_url, Some(jwt_secret));

    let mut driver = Driver::new(engine, pipeline, config);
    driver.advance().await?;
    driver.advance().await?;
    driver.advance().await?;
    driver.advance().await?;
    driver.advance().await?;

    Ok(())
}
