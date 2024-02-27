use async_trait::async_trait;
use eyre::Result;

use super::{
    ExecutionPayload, ForkChoiceUpdate, ForkchoiceState, PayloadAttributes, PayloadId,
    PayloadStatus,
};

/// ## Engine
///
/// A set of methods that allow a consensus client to interact with an execution engine.
/// This is a modified version of the [Ethereum Execution API Specs](https://github.com/ethereum/execution-apis),
/// as defined in the [Optimism Exec Engine Specs](https://github.com/ethereum-optimism/specs/blob/main/specs/protocol/exec-engine.md).
#[async_trait]
pub trait Engine: Send + Sync + 'static {
    /// ## forkchoice_updated
    ///
    /// Updates were made to [`engine_forkchoiceUpdatedV2`](https://github.com/ethereum/execution-apis/blob/main/src/engine/shanghai.md#engine_forkchoiceupdatedv2)
    /// for L2: an extended [PayloadAttributes]
    ///  This updates which L2 blocks the engine considers to be canonical ([ForkchoiceState] argument),
    /// and optionally initiates block production ([PayloadAttributes] argument).
    ///
    /// ### Specification
    ///
    /// method: engine_forkchoiceUpdatedV2
    /// params:
    /// - [ForkchoiceState]
    /// - [PayloadAttributes]
    ///
    /// timeout: 8s
    ///
    /// returns:
    /// - [ForkChoiceUpdate]
    ///
    /// potential errors:
    /// - code and message set in case an exception happens while the validating payload, updating the forkchoice or initiating the payload build process.
    ///
    /// ### Reference
    ///
    /// See more details in the [Optimism Specs](https://github.com/ethereum-optimism/specs/blob/main/specs/protocol/exec-engine.md#engine_forkchoiceupdatedv1).
    async fn forkchoice_updated(
        &self,
        forkchoice_state: ForkchoiceState,
        payload_attributes: Option<PayloadAttributes>,
    ) -> Result<ForkChoiceUpdate>;

    /// ## new_payload
    ///
    /// No modifications to [`engine_newPayloadV2`](https://github.com/ethereum/execution-apis/blob/main/src/engine/shanghai.md#engine_newpayloadv2)
    /// were made for L2. Applies a L2 block to the engine state.
    ///
    /// ### Specification
    ///
    /// method: engine_newPayloadV2
    ///
    /// params:
    /// - [ExecutionPayload]
    ///
    /// timeout: 8s
    ///
    /// returns:
    /// - [PayloadStatus]
    ///
    /// potential errors:
    /// - code and message set in case an exception happens while processing the payload.
    ///
    /// ### Reference
    ///
    /// See more details in the [Optimism Specs](https://github.com/ethereum-optimism/specs/blob/main/specs/protocol/exec-engine.md#engine_newPayloadv1).
    async fn new_payload(&self, execution_payload: ExecutionPayload) -> Result<PayloadStatus>;

    /// ## get_payload
    ///
    /// No modifications to [`engine_getPayloadV2`](https://github.com/ethereum/execution-apis/blob/main/src/engine/shanghai.md#engine_getpayloadv2)
    /// were made for L2. Retrieves a payload by ID, prepared by [engine_forkchoiceUpdatedV2](super::EngineApi)
    /// when called with [PayloadAttributes].
    ///
    /// ### Specification
    ///
    /// method: engine_getPayloadV2
    ///
    /// params:
    /// - [PayloadId]: DATA, 8 Bytes - Identifier of the payload build process
    ///
    /// timeout: 1s
    ///
    /// returns:
    /// - [ExecutionPayload]
    ///
    /// potential errors:
    /// - code and message set in case an exception happens while getting the payload.
    ///
    /// ### Reference
    ///
    /// See more details in the [Optimism Specs](https://github.com/ethereum-optimism/specs/blob/main/specs/protocol/exec-engine.md#engine_getPayloadv1).
    async fn get_payload(&self, payload_id: PayloadId) -> Result<ExecutionPayload>;
}
