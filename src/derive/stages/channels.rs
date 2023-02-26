use std::{cell::RefCell, rc::Rc, sync::Arc};

use ethers_core::types::H256;
use eyre::Result;

use super::batcher_transactions::{BatcherTransactions, Frame};
use crate::config::Config;

pub struct Channels {
    pending_channels: Vec<PendingChannel>,
    prev_stage: Rc<RefCell<BatcherTransactions>>,
    ready_channel: Option<Channel>,
    /// A bank of frames and their version numbers pulled from a [BatcherTransaction]
    frame_bank: Vec<(u8, Frame)>,
    /// The maximum number of pending channels to hold in the bank
    max_channels: usize,
    /// The max timeout for a channel (as measured by the frame L1 block number)
    max_timeout: u64,
}

impl Iterator for Channels {
    type Item = Channel;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if self.ready_channel.is_some() {
                return self.ready_channel.take();
            }
            if let Err(e) = self.process_frame() {
                tracing::error!(
                    "Failed to process frame in the channels stage! Err: {:?}",
                    e
                );
                return None;
            }
        }
    }
}

impl Channels {
    pub fn new(
        prev_stage: Rc<RefCell<BatcherTransactions>>,
        config: Arc<Config>,
    ) -> Rc<RefCell<Self>> {
        Rc::new(RefCell::new(Self {
            pending_channels: Vec::new(),
            prev_stage,
            ready_channel: None,
            frame_bank: Vec::new(),
            max_channels: config.max_channels,
            max_timeout: config.max_timeout,
        }))
    }

    /// Pushes a frame into the correct pending channel
    fn push_frame(&mut self, frame: Frame) {
        // Find a pending channel matching on the channel id
        let pending = self
            .pending_channels
            .iter_mut()
            .find(|c| c.channel_id == frame.channel_id);

        // Insert frame if pending channel exists
        // Otherwise, construct a new pending channel with the frame's id
        if let Some(pending) = pending {
            let has_seen = pending
                .frames
                .iter()
                .map(|f| f.frame_number)
                .any(|n| n == frame.frame_number);

            if !has_seen {
                if frame.is_last {
                    pending.size = Some(frame.frame_number + 1);
                }
                pending.frames.push(frame);
            }
        } else {
            let size = if frame.is_last {
                Some(frame.frame_number + 1)
            } else {
                None
            };

            let pending = PendingChannel {
                channel_id: frame.channel_id,
                frames: vec![frame],
                size,
                // TODO: we'll need to set these by param
                highest_l1_block: BlockID {
                    hash: H256::zero(),
                    number: 0,
                    parent_hash: H256::zero(),
                },
                lowest_l1_block: BlockID {
                    hash: H256::zero(),
                    number: 0,
                    parent_hash: H256::zero(),
                },
            };

            self.pending_channels.push(pending);
        }
    }

    /// Pull the next batcher transaction from the [BatcherTransactions] stage
    fn fill_bank(&mut self) -> Result<()> {
        if !self.frame_bank.is_empty() {
            return Err(eyre::eyre!("Trying to fill bank when it's not empty!"));
        }
        let next_batcher_tx = self
            .prev_stage
            .borrow_mut()
            .next()
            .ok_or(eyre::eyre!("No batcher tx"))??;
        self.frame_bank = next_batcher_tx
            .clone()
            .frames
            .into_iter()
            .map(|f| (next_batcher_tx.version, f))
            .collect();
        Ok(())
    }

    /// Load Ready Channel
    fn load_ready_channel(&mut self, id: u128) {
        if let Some(pc) = self.pending_channels.iter().find(|c| c.channel_id == id) {
            if pc.is_complete(self.max_timeout) {
                self.ready_channel = Some(Channel {
                    id: pc.channel_id,
                    data: pc.assemble(),
                });
            }
        }
    }

    /// Processes the next frame in the [BatcherTransactions] stage
    fn process_frame(&mut self) -> Result<()> {
        // If there's no frame in the bank, fill it with the next batcher tx
        if self.frame_bank.is_empty() && self.fill_bank().is_err() {
            tracing::debug!("Failed to pull batcher tx in the channels stage!");
            return Ok(());
        }

        // Append the frame to the channel
        let (_, frame) = self.frame_bank.remove(0);
        let frame_channel_id = frame.channel_id;
        self.push_frame(frame);
        self.load_ready_channel(frame_channel_id);
        self.prune();

        Ok(())
    }

    /// Removes a pending channel from the bank
    fn remove(&mut self) -> Option<PendingChannel> {
        match self.pending_channels.is_empty() {
            true => Some(self.pending_channels.remove(0)),
            false => None,
        }
    }

    /// Gets the total size of all pending channels
    fn total_size(&self) -> usize {
        self.pending_channels.len()
    }

    /// Prunes channels to the max size
    fn prune(&mut self) {
        // First, remove any timed out channels, then remove any beyond the max capacity
        self.pending_channels
            .retain(|c| !c.is_timed_out(self.max_timeout));
        while self.total_size() > self.max_channels {
            self.remove().expect("Should have removed a channel");
        }
    }
}

/// A Block Identifier
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct BlockID {
    pub hash: H256,
    pub number: u64,
    pub parent_hash: H256,
}

/// An intermediate pending channel
#[derive(Debug)]
struct PendingChannel {
    channel_id: u128,
    frames: Vec<Frame>,
    size: Option<u16>,
    highest_l1_block: BlockID,
    lowest_l1_block: BlockID,
}

impl PendingChannel {
    pub fn is_complete(&self, max_channel_timeout: u64) -> bool {
        let sized = self.size == Some(self.frames.len() as u16);
        let not_expired = !self.is_timed_out(max_channel_timeout);
        sized && not_expired
    }

    /// Checks if the channel has timed out
    pub fn is_timed_out(&self, max_timeout: u64) -> bool {
        self.highest_l1_block.number - self.lowest_l1_block.number > max_timeout
    }

    /// Assembles the pending channel into channel data
    pub fn assemble(&self) -> Vec<u8> {
        let mut frames = self.frames.clone();
        frames.sort_by_key(|f| f.frame_number);
        frames
            .iter()
            .fold(Vec::new(), |a, b| [a, b.frame_data.clone()].concat())
    }
}

/// A Channel
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Channel {
    pub id: u128,
    pub data: Vec<u8>,
}
