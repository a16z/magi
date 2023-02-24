

pub trait EngineApi {

    /// ## engine_forkchoiceUpdatedV1
    ///
    /// This updates which L2 blocks the engine considers to be canonical (`forkchoiceState` argument),
    /// and optionally initiates block production (`payloadAttributes` argument).
    ///
    /// 
    fn forkChoiceUpdatedV1() -> eyre::Result<()>;
}