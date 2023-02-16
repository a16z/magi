use crate::batcher_transaction::Frame;

pub struct ChannelBank {
    pending_channels: Vec<PendingChannel>,
}

impl ChannelBank {
    pub fn new() -> Self {
        Self {
            pending_channels: Vec::new(),
        }
    }

    pub fn push_frame(&mut self, frame: Frame) {
        let pending = self
            .pending_channels
            .iter_mut()
            .find(|c| c.channel_id == frame.channel_id);
        if let Some(pending) = pending {
            let seen_numbers = pending
                .frames
                .iter()
                .map(|f| f.frame_number)
                .collect::<Vec<_>>();
            if !seen_numbers.contains(&frame.frame_number) {
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
            };

            self.pending_channels.push(pending);
        }
    }

    pub fn next(&mut self) -> Option<Channel> {
        let i = self
            .pending_channels
            .iter_mut()
            .position(|c| c.size == Some(c.frames.len() as u16));

        i.map(|i| {
            let c = self.pending_channels.get_mut(i).unwrap();
            c.frames.sort_by_key(|f| f.frame_number);
            let data = c
                .frames
                .iter()
                .fold(Vec::new(), |a, b| [a, b.frame_data.clone()].concat());
            let id = c.channel_id.clone();

            self.pending_channels.remove(i);

            Channel { id, data }
        })
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
