use eyre::Result;

use magi::derive::Pipeline;

#[tokio::main]
async fn main() -> Result<()> {
    let start_epoch = 8494058;
    let mut pipeline = Pipeline::new(start_epoch);

    loop {
        let attributes = pipeline.next();
        if let Some(attributes) = attributes {
            println!("{:?}", attributes);
        }
    }
}
