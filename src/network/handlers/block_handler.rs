use libp2p::gossipsub::{IdentTopic, Message, MessageAcceptance, TopicHash};

use super::Handler;

pub struct BlockHandler {
    chain_id: u64,
}

impl Handler for BlockHandler {
    fn handle(&self, msg: Message) -> MessageAcceptance {
        _ = msg;
        tracing::info!("received block");
        MessageAcceptance::Accept
    }

    fn topic(&self) -> TopicHash {
        IdentTopic::new(format!("/optimism/{}/0/blocks", self.chain_id)).into()
    }
}

impl BlockHandler {
    pub fn new(chain_id: u64) -> Self {
        Self { chain_id }
    }
}
