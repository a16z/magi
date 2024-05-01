//! A module defining the stages of the derivation pipeline.

pub mod attributes;
pub use attributes::{Attributes, DepositedTransaction, AttributesDeposited, UserDeposited};

pub mod batcher_transactions;
pub use batcher_transactions::{BatcherTransactions, BatcherTransactionMessage, BatcherTransaction, Frame};

pub mod batches;
pub use batches::Batches;

/// A module to handle building a [BlockInput](crate::derive::stages::block_input::BlockInput)
mod block_input;

/// A module to handle the channel bank derivation stage
pub mod channels;

/// A module to handle processing of a [SingleBatch](crate::derive::stages::single_batch::SingleBatch)
mod single_batch;

/// A module to handle processing of a [SpanBatch](crate::derive::stages::span_batch::SpanBatch)
mod span_batch;
