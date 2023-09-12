use std::sync::Arc;

use super::batcher_transactions::{BatcherTransaction, Frame};
use crate::{config::Config, derive::PurgeableIterator};

pub struct Channels<I> {
    batcher_tx_iter: I,
    /// List of incomplete channels
    pending_channels: Vec<PendingChannel>,
    /// A bank of frames and their version numbers pulled from a [BatcherTransaction]
    frame_bank: Vec<Frame>,
    /// The maximum total byte size of pending channels to hold in the bank
    max_channel_size: u64,
    /// The max timeout for a channel (as measured by the frame L1 block number)
    channel_timeout: u64,
}

impl<I> Iterator for Channels<I>
where
    I: Iterator<Item = BatcherTransaction>,
{
    type Item = Channel;

    fn next(&mut self) -> Option<Self::Item> {
        self.process_frames()
    }
}

impl<I> PurgeableIterator for Channels<I>
where
    I: PurgeableIterator<Item = BatcherTransaction>,
{
    fn purge(&mut self) {
        self.batcher_tx_iter.purge();
        self.pending_channels.clear();
        self.frame_bank.clear();
    }
}

impl<I> Channels<I> {
    pub fn new(batcher_tx_iter: I, config: Arc<Config>) -> Self {
        Self {
            batcher_tx_iter,
            pending_channels: Vec::new(),
            frame_bank: Vec::new(),
            max_channel_size: config.chain.max_channel_size,
            channel_timeout: config.chain.channel_timeout,
        }
    }
}

impl<I> Channels<I>
where
    I: Iterator<Item = BatcherTransaction>,
{
    /// Pushes a frame into the correct pending channel
    fn push_frame(&mut self, frame: Frame) {
        // Find a pending channel matching on the channel id
        let pending_index = self
            .pending_channels
            .iter_mut()
            .position(|c| c.channel_id == frame.channel_id);

        // Insert frame if pending channel exists
        // Otherwise, construct a new pending channel with the frame's id
        if let Some(pending_index) = pending_index {
            self.pending_channels[pending_index].push_frame(frame);

            if self.pending_channels[pending_index].is_timed_out(self.channel_timeout) {
                self.pending_channels.remove(pending_index);
            }
        } else {
            let pending = PendingChannel::new(frame);
            self.pending_channels.push(pending);
        }
    }

    /// Pull the next batcher transaction from the [BatcherTransactions] stage
    fn fill_bank(&mut self) {
        let next_batcher_tx = self.batcher_tx_iter.next();

        if let Some(tx) = next_batcher_tx {
            self.frame_bank.append(&mut tx.frames.to_vec());
        }
    }

    /// Fetch the completed channel if it is ready
    fn fetch_ready_channel(&mut self, id: u128) -> Option<Channel> {
        let channel_index = self
            .pending_channels
            .iter()
            .position(|c| c.channel_id == id && c.is_complete());

        channel_index.map(|index| {
            let pc = self.pending_channels.remove(index);
            Channel::from(pc)
        })
    }

    /// Processes frames until there are either none left or a channel is ready
    fn process_frames(&mut self) -> Option<Channel> {
        self.fill_bank();

        while !self.frame_bank.is_empty() {
            // Append the frame to the channel
            let frame = self.frame_bank.remove(0);
            let frame_channel_id = frame.channel_id;
            self.push_frame(frame);
            self.prune();

            if let Some(channel) = self.fetch_ready_channel(frame_channel_id) {
                return Some(channel);
            }
        }

        None
    }

    /// Removes a pending channel from the bank
    fn remove(&mut self) -> Option<PendingChannel> {
        match self.pending_channels.is_empty() {
            true => None,
            false => Some(self.pending_channels.remove(0)),
        }
    }

    /// Gets the total size of all pending channels
    fn total_size(&self) -> u64 {
        self.pending_channels
            .iter()
            .map(|p| p.frames.iter().fold(0, |a, f| a + f.frame_data_len))
            .sum::<u32>() as u64
    }

    /// Prunes channels to the max size
    fn prune(&mut self) {
        while self.total_size() > self.max_channel_size {
            self.remove().expect("should have removed a channel");
        }
    }
}

/// An intermediate pending channel
#[derive(Debug)]
struct PendingChannel {
    channel_id: u128,
    frames: Vec<Frame>,
    size: Option<u16>,
    highest_l1_block: u64,
    lowest_l1_block: u64,
}

impl PendingChannel {
    /// Creates a new pending channel with an initial frame
    pub fn new(frame: Frame) -> Self {
        let size = if frame.is_last {
            Some(frame.frame_number + 1)
        } else {
            None
        };

        Self {
            channel_id: frame.channel_id,
            highest_l1_block: frame.l1_inclusion_block,
            lowest_l1_block: frame.l1_inclusion_block,
            frames: vec![frame],
            size,
        }
    }

    pub fn is_complete(&self) -> bool {
        self.size == Some(self.frames.len() as u16)
    }

    /// Checks if the channel has timed out
    pub fn is_timed_out(&self, max_timeout: u64) -> bool {
        self.highest_l1_block - self.lowest_l1_block > max_timeout
    }

    /// Assembles the pending channel into channel data
    pub fn assemble(&self) -> Vec<u8> {
        let mut frames = self.frames.clone();
        frames.sort_by_key(|f| f.frame_number);
        frames
            .iter()
            .fold(Vec::new(), |a, b| [a, b.frame_data.clone()].concat())
    }

    pub fn l1_inclusion_block(&self) -> u64 {
        self.frames
            .iter()
            .map(|f| f.l1_inclusion_block)
            .max()
            .expect("empty frame not allowed")
    }

    /// Adds a new frame to the pending channel. If the frame has already
    /// been seen it ignores it.
    pub fn push_frame(&mut self, frame: Frame) {
        let has_seen = self
            .frames
            .iter()
            .map(|f| f.frame_number)
            .any(|n| n == frame.frame_number);

        if !has_seen {
            if frame.l1_inclusion_block > self.highest_l1_block {
                self.highest_l1_block = frame.l1_inclusion_block;
            } else if frame.l1_inclusion_block < self.lowest_l1_block {
                self.lowest_l1_block = frame.l1_inclusion_block;
            }

            if frame.is_last {
                self.size = Some(frame.frame_number + 1);
            }

            self.frames.push(frame)
        }
    }
}

/// A Channel
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Channel {
    pub id: u128,
    pub data: Vec<u8>,
    pub l1_inclusion_block: u64,
}

impl From<PendingChannel> for Channel {
    fn from(pc: PendingChannel) -> Self {
        Channel {
            id: pc.channel_id,
            data: pc.assemble(),
            l1_inclusion_block: pc.l1_inclusion_block(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        config::{ChainConfig, Config},
        derive::stages::batcher_transactions::{
            BatcherTransactionMessage, BatcherTransactions, Frame,
        },
    };
    use std::sync::{mpsc, Arc};

    use super::Channels;

    #[test]
    fn test_push_single_channel_frame() {
        let (mut stage, _tx) = create_stage();

        let frame = Frame {
            channel_id: 5,
            frame_number: 0,
            frame_data: Vec::new(),
            frame_data_len: 0,
            is_last: true,
            l1_inclusion_block: 0,
        };

        stage.push_frame(frame);

        assert_eq!(stage.pending_channels.len(), 1);
        assert_eq!(stage.pending_channels[0].channel_id, 5);
        assert!(stage.pending_channels[0].is_complete());
    }

    #[test]
    fn test_push_multi_channel_frame() {
        let (mut stage, _tx) = create_stage();

        let frame_1 = Frame {
            channel_id: 5,
            frame_number: 0,
            frame_data: Vec::new(),
            frame_data_len: 0,
            is_last: false,
            l1_inclusion_block: 0,
        };

        let frame_2 = Frame {
            channel_id: 5,
            frame_number: 1,
            frame_data: Vec::new(),
            frame_data_len: 0,
            is_last: true,
            l1_inclusion_block: 0,
        };

        stage.push_frame(frame_1);

        assert_eq!(stage.pending_channels.len(), 1);
        assert_eq!(stage.pending_channels[0].channel_id, 5);
        assert!(!stage.pending_channels[0].is_complete());

        stage.push_frame(frame_2);

        assert_eq!(stage.pending_channels.len(), 1);
        assert_eq!(stage.pending_channels[0].channel_id, 5);
        assert!(stage.pending_channels[0].is_complete());
    }

    #[test]
    fn test_ready_channel() {
        let (mut stage, _tx) = create_stage();

        let frame_1 = Frame {
            channel_id: 5,
            frame_number: 0,
            frame_data: Vec::new(),
            frame_data_len: 0,
            is_last: false,
            l1_inclusion_block: 43,
        };

        let frame_2 = Frame {
            channel_id: 5,
            frame_number: 1,
            frame_data: Vec::new(),
            frame_data_len: 0,
            is_last: true,
            l1_inclusion_block: 96,
        };

        stage.push_frame(frame_1);
        stage.push_frame(frame_2);

        let channel = stage.fetch_ready_channel(5).unwrap();

        assert_eq!(channel.id, 5);
        assert_eq!(channel.l1_inclusion_block, 96);
    }

    #[test]
    fn test_ready_channel_still_pending() {
        let (mut stage, _tx) = create_stage();

        let frame_1 = Frame {
            channel_id: 5,
            frame_number: 0,
            frame_data: Vec::new(),
            frame_data_len: 0,
            is_last: false,
            l1_inclusion_block: 43,
        };

        stage.push_frame(frame_1);

        let channel = stage.fetch_ready_channel(5);

        assert_eq!(channel, None);
    }

    #[test]
    fn test_channel_timeout() {
        let (mut stage, _tx) = create_stage();

        let frame_1 = Frame {
            channel_id: 5,
            frame_number: 0,
            frame_data: Vec::new(),
            frame_data_len: 0,
            is_last: false,
            l1_inclusion_block: 0,
        };

        let frame_2 = Frame {
            channel_id: 5,
            frame_number: 1,
            frame_data: Vec::new(),
            frame_data_len: 0,
            is_last: true,
            l1_inclusion_block: 500,
        };

        stage.push_frame(frame_1);
        assert_eq!(stage.pending_channels.len(), 1);

        stage.push_frame(frame_2);
        assert_eq!(stage.pending_channels.len(), 0);
    }

    fn create_stage() -> (
        Channels<BatcherTransactions>,
        mpsc::Sender<BatcherTransactionMessage>,
    ) {
        let config = Config {
            l1_rpc_url: String::new(),
            l2_rpc_url: String::new(),
            l2_engine_url: String::new(),
            jwt_secret: String::new(),
            rpc_port: 9545,
            chain: ChainConfig::optimism_goerli(),
            checkpoint_sync_url: None,
            devnet: false,
        };

        let (tx, rx) = mpsc::channel();
        (
            Channels::new(BatcherTransactions::new(rx), Arc::new(config)),
            tx,
        )
    }
}
