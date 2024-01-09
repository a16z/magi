use libp2p::gossipsub::{Message, MessageAcceptance, TopicHash};

pub mod block_handler;

pub trait Handler: Send {
    fn handle(&self, msg: Message) -> MessageAcceptance;
    fn topics(&self) -> Vec<TopicHash>;
}
