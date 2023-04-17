#![warn(unreachable_pub)]
#![deny(missing_docs, missing_debug_implementations)]

/// Payload Types
mod payload;
pub use payload::*;

/// Forkchoice Types
mod fork;
pub use fork::*;

/// The Engine Drive
mod api;
pub use api::*;

/// Auth module
mod auth;
pub use auth::*;

/// Common Types
mod types;
pub use types::*;

/// Core Trait
mod traits;
pub use traits::*;

/// Mock Engine
mod mock_engine;
pub use mock_engine::*;


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
