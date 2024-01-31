use crate::engine::ExecutionPayload;
use serde::{Deserialize, Serialize};

use super::common::{BlockInfo, HeadInfo};

use eyre::Result;

/// The node sync status.
#[derive(Copy, Clone, Debug, Default, Serialize, Deserialize)]
pub struct SyncStatus {
    pub current_l1: BlockInfo,
    pub current_l1_finalized: BlockInfo,
    pub head_l1: BlockInfo,
    pub safe_l1: BlockInfo,
    pub finalized_l1: BlockInfo,
    pub unsafe_l2: HeadInfo,
    pub safe_l2: HeadInfo,
    pub finalized_l2: HeadInfo,
    pub queued_unsafe_l2: HeadInfo,
    pub engine_sync_target: HeadInfo,
}

impl SyncStatus {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        current_l1: BlockInfo,
        finalized_l1: BlockInfo,
        head_l1: BlockInfo,
        safe_l1: BlockInfo,
        unsafe_l2: HeadInfo,
        safe_l2: HeadInfo,
        finalized_l2: HeadInfo,
        queued_payload: Option<&ExecutionPayload>,
        engine_sync_target: HeadInfo,
    ) -> Result<Self> {
        let queued_unsafe_l2 = match queued_payload {
            Some(payload) => payload.try_into()?,
            None => Default::default(),
        };

        Ok(Self {
            current_l1,
            current_l1_finalized: finalized_l1,
            head_l1,
            safe_l1,
            finalized_l1,
            unsafe_l2,
            safe_l2,
            finalized_l2,
            queued_unsafe_l2,
            engine_sync_target,
        })
    }
}
