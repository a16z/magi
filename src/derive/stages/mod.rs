/// A module to handle the payload attributes derivation stage
pub mod attributes;

/// A module to handle batcher transactions and frames
pub mod batcher_transactions;

/// A module to handle processing of a [Batch](crate::derive::stages::batches::Batch)
pub mod batches;

/// A module to handle building a [BlockInput](crate::derive::stages::block_input::BlockInput)
mod block_input;

/// A module to handle the channel bank derivation stage
pub mod channels;

/// A module to handle processing of a [SingleBatch](crate::derive::stages::single_batch::SingleBatch)
mod single_batch;

/// A module to handle processing of a [SpanBatch](crate::derive::stages::span_batch::SpanBatch)
mod span_batch;
