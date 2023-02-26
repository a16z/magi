use magi::engine::{EngineApi, L2EngineApi};

#[tokio::test]
async fn test_engine_api() {
    let engine_api = EngineApi::from_env();

    let base_body = engine_api.base_body();
    assert_eq!(base_body.get("jsonrpc").unwrap(), "2.0");
    assert_eq!(base_body.get("id").unwrap(), "1");

    let res = engine_api.get_payload(10).await;
    println!("Response: {:?}", res);
}
