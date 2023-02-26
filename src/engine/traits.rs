use eyre::Result;
use super::{ForkchoiceState, PayloadAttributes, ForkChoiceUpdate, ExecutionPayload, PayloadStatus, PayloadId};

/// ## L2 Engine API
///
/// A set of methods that allow a consensus client to interact with an execution engine.
/// This is a modified version of the [Ethereum Execution API Specs](https://github.com/ethereum/execution-apis),
/// as defined in the [Optimism Exec Engine Specs](https://github.com/ethereum-optimism/optimism/blob/develop/specs/exec-engine.md).
#[allow(non_snake_case)]
pub trait L2EngineApi {
    /// ## forkchoice_updated
    ///
    /// Updates were made to [`engine_forkchoiceUpdatedV1`](https://github.com/ethereum/execution-apis/blob/main/src/engine/paris.md#engine_forkchoiceupdatedv1)
    /// for L2. This updates which L2 blocks the engine considers to be canonical ([ForkchoiceState] argument),
    /// and optionally initiates block production ([PayloadAttributes] argument).
    ///
    /// ### Specification
    ///
    /// method: engine_forkchoiceUpdatedV1
    /// params:
    /// - [ForkchoiceState]
    /// - [PayloadAttributes]
    /// timeout: 8s
    /// returns:
    /// - [ForkChoiceUpdate]
    /// potential errors:
    /// - code and message set in case an exception happens while the validating payload, updating the forkchoice or initiating the payload build process.
    ///
    /// ### Reference
    ///
    /// See more details in the [Optimism Specs](https://github.com/ethereum-optimism/optimism/blob/develop/specs/exec-engine.md#engine_forkchoiceupdatedv1).
    fn forkchoice_updated(
        forkchoiceState: ForkchoiceState,
        payloadAttributes: Option<PayloadAttributes>,
    ) -> Result<ForkChoiceUpdate>;

    /// ## new_payload
    ///
    /// No modifications to [`engine_newPayloadV1`](https://github.com/ethereum/execution-apis/blob/main/src/engine/paris.md#engine_newpayloadv1)
    /// were made for L2. Applies a L2 block to the engine state.
    ///
    /// ### Specification
    ///
    /// method: engine_newPayloadV1
    /// params:
    /// - [ExecutionPayload]
    /// timeout: 8s
    /// returns:
    /// - [PayloadStatus]
    /// potential errors:
    /// - code and message set in case an exception happens while processing the payload.
    ///
    /// ### Reference
    ///
    /// See more details in the [Optimism Specs](https://github.com/ethereum-optimism/optimism/blob/develop/specs/exec-engine.md#engine_newPayloadv1).
    fn new_payload(executionPayload: ExecutionPayload) -> Result<PayloadStatus>;

    /// ## get_payload
    ///
    /// No modifications to [`engine_getPayloadV1`](https://github.com/ethereum/execution-apis/blob/main/src/engine/paris.md#engine_getpayloadv1)
    /// were made for L2. Retrieves a payload by ID, prepared by [engine_forkchoiceUpdatedV1](EngineApi::engine_forkchoiceUpdatedV1)
    /// when called with [PayloadAttributes].
    ///
    /// ### Specification
    ///
    /// method: engine_getPayloadV1
    /// params:
    /// - [PayloadId]: DATA, 8 Bytes - Identifier of the payload build process
    /// timeout: 1s
    /// returns:
    /// - [ExecutionPayload]
    /// potential errors:
    /// - code and message set in case an exception happens while getting the payload.
    ///
    /// ### Reference
    ///
    /// See more details in the [Optimism Specs](https://github.com/ethereum-optimism/optimism/blob/develop/specs/exec-engine.md#engine_getPayloadv1).
    fn get_payload(payloadId: PayloadId) -> Result<ExecutionPayload>;
}
