//! Module contains the network handlers for processing incoming p2p gossip messages.

use libp2p::gossipsub::{Message, MessageAcceptance, TopicHash};

pub mod block_handler;
pub use block_handler::BlockHandler;

/// This trait defines the functionality required to process incoming messages
/// and determine their acceptance within the network. Implementors of this trait
/// can specify how messages are handled and which topics they are interested in.
pub trait Handler: Send {
    /// Manages validation and further processing of messages
    fn handle(&self, msg: Message) -> MessageAcceptance;
    /// Specifies which topics the handler is interested in
    fn topics(&self) -> Vec<TopicHash>;
}
