//! L2 Engine API module.

pub mod payload;
pub use payload::{ExecutionPayload, PayloadAttributes, PayloadId, PayloadStatus, Status};

pub mod fork;
pub use fork::{ForkChoiceUpdate, ForkchoiceState};

pub mod api;
pub use api::{EngineApi, EngineApiErrorPayload, EngineApiResponse};

pub mod auth;
pub use auth::JwtSecret;

pub mod params;
pub use params::{
    DEFAULT_AUTH_PORT, ENGINE_FORKCHOICE_UPDATED_TIMEOUT, ENGINE_FORKCHOICE_UPDATED_V2,
    ENGINE_GET_PAYLOAD_TIMEOUT, ENGINE_GET_PAYLOAD_V2, ENGINE_NEW_PAYLOAD_TIMEOUT,
    ENGINE_NEW_PAYLOAD_V2, JSONRPC_VERSION, STATIC_ID,
};

pub mod traits;
pub use traits::Engine;

pub mod mock_engine;
pub use mock_engine::MockEngine;

#[cfg(test)]
mod tests {
    use crate::engine::EngineApi;

    #[test]
    fn test_engine_api() {
        let jwt_secret = "bf549f5188556ce0951048ef467ec93067bc4ea21acebe46ef675cd4e8e015ff";
        let url = "http://localhost:8551";

        let engine_api = EngineApi::new(url, jwt_secret);

        let base_body = engine_api.base_body();
        assert_eq!(base_body.get("jsonrpc").unwrap(), "2.0");
        assert_eq!(base_body.get("id").unwrap(), 1);
    }
}
