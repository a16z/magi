use ethers_core::types::H256;
use serde::{Deserialize, Serialize};

use super::{PayloadId, PayloadStatus};

/// The result of a fork choice update.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForkChoiceUpdate {
    /// Payload status.
    /// Note: values of the status field in the context of this method are restricted to the following subset: VALID, INVALID, SYNCING.
    pub payload_status: PayloadStatus,
    /// 8 byte identifier of the payload build process or null
    pub payload_id: Option<PayloadId>,
}

/// ## ForkchoiceStateV1
///
/// Note: [ForkchoiceState.safe_block_hash] and [ForkchoiceState.finalized_block_hash]fields are allowed to have
/// 0x0000000000000000000000000000000000000000000000000000000000000000 value unless transition block is finalized.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForkchoiceState {
    /// 32 byte block hash of the head of the canonical chain
    pub head_block_hash: H256,
    /// 32 byte "safe" block hash of the canonical chain under certain synchrony and honesty assumptions
    /// This value MUST be either equal to or an ancestor of headBlockHash
    pub safe_block_hash: H256,
    /// 32 byte block hash of the most recent finalized block
    pub finalized_block_hash: H256,
}
