#![allow(dead_code)]

//! Authentication module for the Engine API.
//!
//! This module was built using [reth](https://github.com/paradigmxyz/reth).

use eyre::Result;
use jsonwebtoken::Algorithm;
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// JWT hex encoded 256 bit secret key length.
const JWT_SECRET_LEN: usize = 64;

/// The maximum amount of drift from the JWT claims issued-at `iat` time.
const JWT_MAX_IAT_DIFF: Duration = Duration::from_secs(60);

/// The execution layer client MUST support at least the following alg HMAC + SHA256 (HS256)
const JWT_SIGNATURE_ALGO: Algorithm = Algorithm::HS256;

/// JwtSecret is a 256-bit hex-encoded secret key used to perform JWT-based authentication.
///
/// See: [Secret key - Engine API specs](https://github.com/ethereum/execution-apis/blob/main/src/engine/authentication.md#key-distribution)
#[derive(Clone)]
pub struct JwtSecret([u8; 32]);

impl JwtSecret {
    /// Creates an instance of [`JwtSecret`][crate::engine::JwtSecret].
    /// The provided `secret` must be a valid hexadecimal string of length 64.
    pub fn from_hex<S: AsRef<str>>(hex: S) -> Result<Self> {
        let hex: &str = hex.as_ref().trim();
        // Remove the "0x" or "0X" prefix if it exists
        let hex = hex
            .strip_prefix("0x")
            .or_else(|| hex.strip_prefix("0X"))
            .unwrap_or(hex);
        if hex.len() != JWT_SECRET_LEN {
            Err(eyre::eyre!(
                "Invalid JWT secret key length. Expected {} characters, got {}.",
                JWT_SECRET_LEN,
                hex.len()
            ))
        } else {
            let hex_bytes = hex::decode(hex)?;
            let bytes = hex_bytes.try_into().expect("is expected len");
            Ok(JwtSecret(bytes))
        }
    }

    /// Generates a random [`JwtSecret`]
    pub fn random() -> Self {
        let random_bytes: [u8; 32] = rand::thread_rng().gen();
        let secret = hex::encode(random_bytes);
        JwtSecret::from_hex(secret).unwrap()
    }

    /// Returns if the provided JWT token is equal to the JWT secret.
    pub fn equal(&self, token: &str) -> bool {
        hex::encode(self.0) == token
    }

    /// Generate claims constructs a [`Claims`][crate::engine::Claims] instance.
    ///
    /// ## Panics
    ///
    /// This function will panic if the system time is before the UNIX_EPOCH.
    pub(crate) fn generate_claims(&self, time: Option<SystemTime>) -> Claims {
        let now = time.unwrap_or_else(SystemTime::now);
        let now_secs = now.duration_since(UNIX_EPOCH).unwrap().as_secs();
        Claims {
            iat: now_secs,
            exp: now_secs + 60,
        }
    }

    /// Encodes the [`Claims`][crate::engine::Claims] in a [jsonwebtoken::Header] String format.
    pub(crate) fn encode(&self, claims: &Claims) -> Result<String, Box<dyn std::error::Error>> {
        let bytes = &self.0;
        let key = jsonwebtoken::EncodingKey::from_secret(bytes);
        let algo = jsonwebtoken::Header::new(Algorithm::HS256);
        Ok(jsonwebtoken::encode(&algo, claims, &key)?)
    }
}

impl std::fmt::Debug for JwtSecret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("JwtSecret").field(&"{{}}").finish()
    }
}

/// Claims are a set of information about an actor authorized by a JWT.
///
/// The Engine API requires that the `iat` (issued-at) claim is provided.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Claims {
    /// The number of seconds since the UNIX_EPOCH.
    pub(crate) iat: u64,
    /// The expiration time of the JWT.
    pub(crate) exp: u64,
}

impl Claims {
    /// Valid returns if the given claims are valid.
    pub(crate) fn valid(&self) -> bool {
        let now = SystemTime::now();
        let now_secs = now.duration_since(UNIX_EPOCH).unwrap().as_secs();
        now_secs.abs_diff(self.iat) <= JWT_MAX_IAT_DIFF.as_secs()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    const SECRET: &str = "f79ae5046bc11c9927afe911db7143c51a806c4a537cc08e0d37140b0192f430";

    #[tokio::test]
    async fn construct_valid_raw_claims() {
        let claims = Claims {
            iat: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            exp: 10000000000,
        };
        assert!(claims.valid());
    }

    #[tokio::test]
    async fn construct_valid_secret_claims() {
        let secret = JwtSecret::from_hex(SECRET).unwrap();
        let secret_claims = secret.generate_claims(None);
        assert!(secret_claims.valid());
    }

    #[tokio::test]
    async fn encode_secret() {
        let secret = JwtSecret::from_hex(SECRET).unwrap();
        let claims = secret.generate_claims(Some(SystemTime::UNIX_EPOCH));
        let jwt = secret.encode(&claims).unwrap();
        assert!(!jwt.is_empty());
    }
}
