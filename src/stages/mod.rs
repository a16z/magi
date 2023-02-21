pub mod attributes;
pub mod batcher_transactions;
pub mod batches;
pub mod channels;

use eyre::Result;

pub trait Stage {
    type Output;

    fn next(&mut self) -> Result<Option<Self::Output>>;
}
