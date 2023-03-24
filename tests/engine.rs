use ethers_core::types::U64;
use magi::engine::{Engine, EngineApi};

#[tokio::test]
async fn test_engine_api() {
    if std::env::var("ENGINE_API_URL").is_ok() && std::env::var("JWT_SECRET").is_ok() {
        let engine_api = EngineApi::from_env();

        let base_body = engine_api.base_body();
        assert_eq!(base_body.get("jsonrpc").unwrap(), "2.0");
        assert_eq!(base_body.get("id").unwrap(), 1);

        match engine_api.get_payload(U64([10])).await {
            Ok(res) => {
                println!("Response: {:?}", res);
                // TODO: assert expected response payload
            }
            Err(e) => {
                println!("Error: {:?}", e);
            }
        }
    } else {
        println!(
            "Skipping test_engine_api because either ENGINE_API_URL or JWT_SECRET are not set..."
        );
    }
}
