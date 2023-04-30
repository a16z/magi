use std::net::SocketAddr;

use discv5::{enr::{CombinedKey, EnrBuilder}, Discv5ConfigBuilder, Discv5};
use eyre::Result;

#[tokio::main]
async fn main() -> Result<()> {
    let addr = "0.0.0.0:9000".parse::<SocketAddr>()?;

    let key = CombinedKey::generate_secp256k1();
    let enr = EnrBuilder::new("v4").build(&key)?;
    let config = Discv5ConfigBuilder::new().build();

    let mut discv5: Discv5 = Discv5::new(enr, key, config).map_err(convert_err)?;
    discv5.start(addr).await.map_err(convert_err)?;

    // let bootnode: Enr = "enode://869d07b5932f17e8490990f75a3f94195e9504ddb6b85f7189e5a9c0a8fff8b00aecf6f3ac450ecba6cdabdb5858788a94bde2b613e0f2d82e9b395355f76d1a@34.65.67.101:0?discport=30305".parse().map_err(|_| eyre::eyre!("could not parse enr"))?;

    discv5.add_enr(enr).map_err(convert_err)?;

    let mut events = discv5.event_stream().await.map_err(convert_err)?;

    while let Some(event) = events.recv().await {
        println!("{:?}", event);
    }

    Ok(())
}

fn convert_err<E>(_err: E) -> eyre::Report {
    eyre::eyre!("error")
}
