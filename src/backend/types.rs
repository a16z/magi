use eyre::Result;
use serde::{Deserialize, Serialize};

use crate::common::{BlockInfo, Epoch};

/// Block info for the current head of the chain
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HeadInfo {
    /// L2 BlockInfo value
    pub l2_block_info: BlockInfo,
    /// L1 batch epoch of the head L2 block
    pub l1_epoch: Epoch,
}

impl TryFrom<sled::IVec> for HeadInfo {
    type Error = eyre::Report;

    fn try_from(bytes: sled::IVec) -> Result<Self, Self::Error> {
        Ok(serde_json::from_slice(bytes.as_ref())
            .map_err(|e| eyre::Result::Err(eyre::eyre!("Failed to deserialize HeadInfo: {}", e)))?)
    }
}

impl From<HeadInfo> for sled::IVec {
    fn from(val: HeadInfo) -> Self {
        serde_json::to_vec(&val)
            .map(sled::IVec::from)
            .unwrap_or_else(|e| eyre::Result::Err(eyre::eyre!("Failed to serialize HeadInfo: {}", e)).unwrap())
    }
}
