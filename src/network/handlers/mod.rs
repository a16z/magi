use libp2p::gossipsub::{Message, MessageAcceptance, TopicHash};

/// A module for managing incoming p2p gossip messages
pub mod block_handler;

pub trait Handler: Send {
    fn handle(&self, msg: Message) -> MessageAcceptance;
    fn topics(&self) -> Vec<TopicHash>;
}
