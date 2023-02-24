use std::{cell::RefCell, rc::Rc};

use eyre::Result;

use super::batcher_transactions::{BatcherTransaction, BatcherTransactions, Frame};

pub struct Channels {
    pending_channels: Vec<PendingChannel>,
    prev_stage: Rc<RefCell<BatcherTransactions>>,
}

impl Iterator for Channels {
    type Item = Result<Channel>;

    fn next(&mut self) -> Option<Self::Item> {
        // pull all batch transactions
        loop {
            let batcher_tx = self.prev_stage.borrow_mut().next()?;
            if batcher_tx.map(|b| self.push_batcher_tx(b)).is_ok() {
                break;
            }
        }

        // find the oldest complete channel
        let i = self
            .pending_channels
            .iter_mut()
            .position(|c| c.size == Some(c.frames.len() as u16));

        // assemble the channel
        i.map(|i| {
            let c = self.pending_channels.get_mut(i).unwrap();
            c.frames.sort_by_key(|f| f.frame_number);

            let data = c
                .frames
                .iter()
                .fold(Vec::new(), |a, b| [a, b.frame_data.clone()].concat());

            let id = c.channel_id;

            self.pending_channels.remove(i);

            Ok(Channel { id, data })
        })
    }
}

impl Channels {
    pub fn new(prev_stage: Rc<RefCell<BatcherTransactions>>) -> Rc<RefCell<Self>> {
        Rc::new(RefCell::new(Self {
            pending_channels: Vec::new(),
            prev_stage,
        }))
    }

    fn push_batcher_tx(&mut self, tx: BatcherTransaction) {
        tx.frames.into_iter().for_each(|f| self.push_frame(f));
    }

    fn push_frame(&mut self, frame: Frame) {
        // try to find the correct pending channel
        let pending = self
            .pending_channels
            .iter_mut()
            .find(|c| c.channel_id == frame.channel_id);

        if let Some(pending) = pending {
            // insert frame if pending channel exists
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
            // create pending channel if it doesn't exist yet
            let size = if frame.is_last {
                Some(frame.frame_number + 1)
            } else {
                None
            };

            let pending = PendingChannel {
                channel_id: frame.channel_id,
                frames: vec![frame],
                size,
            };

            self.pending_channels.push(pending);
        }
    }
}

struct PendingChannel {
    channel_id: u128,
    frames: Vec<Frame>,
    size: Option<u16>,
}

pub struct Channel {
    pub id: u128,
    pub data: Vec<u8>,
}
