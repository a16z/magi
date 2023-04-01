use ethers::contract::abigen;

abigen!(
    SystemConfigContract,
    r#"[
        event ConfigUpdate(uint256 indexed version, uint256 indexed updateType, bytes data)
    ]"#,
);
