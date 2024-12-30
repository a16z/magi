use libp2p::gossipsub::{Message, MessageAcceptance, TopicHash};

/// A module for managing incoming p2p gossip messages
pub mod block_handler;

/// This trait defines the functionality required to process incoming messages
/// and determine their acceptance within the network. Implementers of this trait
/// can specify how messages are handled and which topics they are interested in.
pub trait Handler: Send {
    /// Manages validation and further processing of messages
    fn handle(&self, msg: Message) -> MessageAcceptance;
    /// Specifies which topics the handler is interested in
    fn topics(&self) -> Vec<TopicHash>;
}
