

/// ## L2 Engine API
///
/// A set of methods that allow a consensus client to interact with an execution engine.
/// This is a modified version of the [Ethereum Execution API Specs](https://github.com/ethereum/execution-apis),
/// as defined in the [Optimism Exec Engine Specs](https://github.com/ethereum-optimism/optimism/blob/develop/specs/exec-engine.md).
pub trait L2EngineApi {
    /// ## engine_forkchoiceUpdatedV1
    ///
    /// Updates were made to [`engine_forkchoiceUpdatedV1`](https://github.com/ethereum/execution-apis/blob/main/src/engine/paris.md#engine_forkchoiceupdatedv1)
    /// for L2. This updates which L2 blocks the engine considers to be canonical (`forkchoiceState` argument),
    /// and optionally initiates block production (`payloadAttributes` argument).
    ///
    /// ### Specifications
    ///
    /// method: engine_forkchoiceUpdatedV1
    /// params:
    /// - ForkchoiceStateV1
    /// - PayloadAttributesV1
    /// timeout: 8s
    ///
    /// ### Reference
    ///
    /// See more details in the [Optimism Specs](https://github.com/ethereum-optimism/optimism/blob/develop/specs/exec-engine.md#engine_forkchoiceupdatedv1).
    fn engine_forkchoiceUpdatedV1() -> eyre::Result<()>;

    /// ## engine_newPayloadV1
    ///
    /// No modifications to [`engine_newPayloadV1`](https://github.com/ethereum/execution-apis/blob/main/src/engine/paris.md#engine_newpayloadv1)
    /// were made for L2. Applies a L2 block to the engine state.
    ///
    /// ### Specifications
    ///
    /// method: engine_newPayloadV1
    /// params:
    /// - ExecutionPayloadV1
    /// timeout: 8s
    fn engine_newPayloadV1() -> eyre::Result<()>;

    /// ## engine_getPayloadV1
    ///
    /// No modifications to [`engine_getPayloadV1`](https://github.com/ethereum/execution-apis/blob/main/src/engine/paris.md#engine_getpayloadv1)
    /// were made for L2. Retrieves a payload by ID, prepared by [engine_forkchoiceUpdatedV1](EngineApi::engine_forkchoiceUpdatedV1)
    /// when called with [payloadAttributes].
    fn engine_forkchoiceUpdatedV1() -> eyre::Result<()>;

}