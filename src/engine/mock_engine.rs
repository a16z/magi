use async_trait::async_trait;
use eyre::Result;

use super::{
    ExecutionPayload, ForkChoiceUpdate, ForkchoiceState, L2EngineApi, PayloadAttributes, PayloadId,
    PayloadStatus,
};

/// Mock L2 Engine API that returns preset responses
#[derive(Debug, Clone)]
pub struct MockEngine {
    /// Forkchoice updated call response when payload is Some
    pub forkchoice_updated_payloads_res: ForkChoiceUpdate,
    /// Forkchoice updated call response when payload is None
    pub forkchoice_updated_res: ForkChoiceUpdate,
    /// New payload call response
    pub new_payload_res: PayloadStatus,
    /// Get payload call response
    pub get_payload_res: ExecutionPayload,
}

#[async_trait]
impl L2EngineApi for MockEngine {
    async fn forkchoice_updated(
        &self,
        _forkchoice_state: ForkchoiceState,
        payload_attributes: Option<PayloadAttributes>,
    ) -> Result<ForkChoiceUpdate> {
        Ok(if payload_attributes.is_some() {
            self.forkchoice_updated_payloads_res.clone()
        } else {
            self.forkchoice_updated_res.clone()
        })
    }

    async fn new_payload(&self, _execution_payload: ExecutionPayload) -> Result<PayloadStatus> {
        Ok(self.new_payload_res.clone())
    }

    async fn get_payload(&self, _payload_id: PayloadId) -> Result<ExecutionPayload> {
        Ok(self.get_payload_res.clone())
    }
}
