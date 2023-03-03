use eyre::Result;

use home::home_dir;
use magi::{
    config::{ChainConfig, Config},
    driver::Driver,
    telemetry,
};

#[tokio::main]
async fn main() -> Result<()> {
    telemetry::init(false)?;

    let l1_rpc = "https://eth-goerli.g.alchemy.com/v2/a--NIcyeycPntQX42kunxUIVkg6_ekYc".to_string();
    let engine_url = "http://127.0.0.1:8551".to_string();
    let jwt_secret = "bf549f5188556ce0951048ef467ec93067bc4ea21acebe46ef675cd4e8e015ff".to_string();

    let mut db_path = home_dir().ok_or(eyre::eyre!("home directory not found"))?;
    db_path.push(".magi/data");
    let db_location = Some(db_path);

    let config = Config {
        l1_rpc,
        engine_url,
        jwt_secret,
        db_location,
        chain: ChainConfig::goerli(),
    };

    let mut driver = Driver::from_config(config)?;
    loop {
        driver.advance().await?;
    }
}
