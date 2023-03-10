use discv5::{enr::k256, enr::CombinedKey};

/// Generates a [discv5] [CombinedKey](discv5::enr::CombinedKey) from the server config.
/// If the static_key flag is set, a fixed key is used for testing.
pub fn generate(static_key: bool, key: Option<String>) -> eyre::Result<CombinedKey> {
    if static_key {
        let raw_key = vec![
            183, 28, 113, 166, 126, 17, 119, 173, 78, 144, 22, 149, 225, 180, 185, 238, 23, 174,
            22, 198, 102, 141, 49, 62, 172, 47, 150, 219, 205, 163, 242, 145,
        ];
        let secret_key = k256::ecdsa::SigningKey::from_bytes(&raw_key)?;
        Ok(CombinedKey::from(secret_key))
    } else if let Some(string_key) = &key {
        let raw_key = hex::decode(string_key)
            .map_err(|_| eyre::eyre!("Invalid hex bytes for secp256k1 key"))?;
        let secret_key = k256::ecdsa::SigningKey::from_bytes(&raw_key)
            .map_err(|_| eyre::eyre!("Invalid secp256k1 key"))?;
        Ok(CombinedKey::from(secret_key))
    } else {
        Ok(CombinedKey::generate_secp256k1())
    }
}
