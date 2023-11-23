//
// Seems `ethers` crate cryptography was never audited.
// We must audit before prod.`
//
use ethers::{
    core::k256::ecdsa::SigningKey,
    types::{Bytes, U256},
    utils::keccak256,
};

use eyre::Result;

// Expect domain size.
const DOMAIN_BYTES_SIZE: usize = 32;

#[derive(Clone, Debug)]
pub struct Signer {
    chain_id: U256,
    private_key: SigningKey,
    domain: Bytes,
}

impl Signer {
    pub fn new(
        chain_id: u64,
        private_key: SigningKey,
        domain_bytes: Option<Bytes>,
    ) -> Result<Self> {
        let domain = match domain_bytes {
            Some(domain) => domain,
            None => Bytes::from(vec![0; DOMAIN_BYTES_SIZE]),
        };

        eyre::ensure!(
            domain.len() == DOMAIN_BYTES_SIZE,
            "invalid domain size: expected {} bytes but got {} bytes",
            DOMAIN_BYTES_SIZE,
            domain.len()
        );

        Ok(Self {
            chain_id: U256::from(chain_id),
            private_key,
            domain,
        })
    }

    pub fn sign(&self, data: &Vec<u8>) -> Result<(Vec<u8>, Vec<u8>)> {
        let hash = self.hash(data);
        let (sig, recovery) = self.private_key.sign_prehash_recoverable(&hash)?;

        let mut raw_sig = sig.to_vec();
        raw_sig.push(recovery.to_byte());

        Ok((hash.to_vec(), raw_sig))
    }

    fn hash(&self, data: &Vec<u8>) -> [u8; 32] {
        let mut bytes = vec![];
        // Domain (32 bytes).
        bytes.extend(self.domain.as_ref());
        // Chain ID (32 bytes).
        let mut chain_id: [u8; 32] = [0; 32];
        self.chain_id.to_big_endian(&mut chain_id);
        bytes.extend(chain_id);
        // Data hash (32 bytes).
        let data_hash = keccak256(data);
        bytes.extend(data_hash);

        keccak256(bytes)
    }
}

#[cfg(test)]
mod test {
    use rand;
    use rand::Rng;

    use super::Signer;
    use super::DOMAIN_BYTES_SIZE;

    use eyre::Result;

    use ethers::core::k256::ecdsa::signature::hazmat::PrehashVerifier;
    use ethers::core::k256::ecdsa::{Signature, SigningKey};
    use ethers::types::{Bytes, U256};

    #[test]
    fn test_new_chain_id() -> Result<()> {
        let private_key = SigningKey::random(&mut rand::thread_rng());

        let signer = Signer::new(16, private_key, None)?;
        assert_eq!(signer.chain_id, U256::from(16), "wrong chain id");

        Ok(())
    }

    #[test]
    fn test_new_key() -> Result<()> {
        let private_key = SigningKey::random(&mut rand::thread_rng());

        let signer = Signer::new(16, private_key.clone(), None)?;
        assert!(signer.private_key == private_key, "pk is not equal");

        Ok(())
    }

    #[test]
    fn test_new_with_domain() -> Result<()> {
        let mut rng = rand::thread_rng();

        let private_key = SigningKey::random(&mut rng);
        let domain = Bytes::from(rng.gen::<[u8; 32]>());

        let signer = Signer::new(32, private_key, Some(domain.clone()))?;
        assert_eq!(signer.domain, domain, "wrong domain");

        Ok(())
    }

    #[test]
    fn test_new_with_domain_wrong_size() {
        let mut rng = rand::thread_rng();

        let private_key = SigningKey::random(&mut rng);
        let domain = Bytes::from(rng.gen::<[u8; 1]>());

        let res = Signer::new(32, private_key, Some(domain.clone()));
        assert!(res.is_err(), "should be domain error");

        let err_msg = res.unwrap_err().to_string();
        assert!(err_msg.contains(&format!(
            "invalid domain size: expected {} bytes but got {} bytes",
            DOMAIN_BYTES_SIZE,
            domain.len()
        )));
    }

    #[test]
    fn test_new_without_domain() -> Result<()> {
        let private_key = SigningKey::random(&mut rand::thread_rng());

        let signer = Signer::new(32, private_key, None)?;
        assert_eq!(
            signer.domain,
            Bytes::from(vec![0; DOMAIN_BYTES_SIZE]),
            "wrong domain"
        );

        Ok(())
    }

    #[test]
    fn test_sign() -> Result<()> {
        let mut rng = rand::thread_rng();
        let private_key = SigningKey::random(&mut rng);

        let signer = Signer::new(1, private_key.clone(), None)?;
        let verifying_key = private_key.verifying_key();

        let data = rng.gen::<[u8; 32]>();
        let (hash, raw_sig) = signer.sign(&data.to_vec())?;

        let signature = Signature::try_from(&raw_sig.as_slice()[..64])?;

        assert!(
            verifying_key.verify_prehash(&hash, &signature).is_ok(),
            "signature can't be verified"
        );

        Ok(())
    }

    #[test]
    fn test_sign_static() -> Result<()> {
        let expected_hash =
            hex::decode("5f5692350e3f36252811cbec60967fc171ad5c53516cc0ee482fb5650ba3522f")?;
        let expected_sig = hex::decode("09b2819c1d89a5ad6ba226018b720576927e5f03a23c57ba94644c4c981847b82b5b932d8a48668dba4726851cd8da85636919878d85e57da79ae5a3283a136801")?;
        let pk_bytes =
            hex::decode("0424ec6a64ab50deb8aea88c09dba51107dbc24fb32ccad3507bfa94a5bff43d")?;

        let private_key = SigningKey::from_slice(&pk_bytes)?;

        let signer = Signer::new(1, private_key.clone(), None)?;
        let verifying_key = private_key.verifying_key();

        let data = hex::decode("cbda5a037a1379ece732e4a791b500f8316fdb301f704344d6f9ef97f3efc90d")?;
        let (hash, raw_sig) = signer.sign(&data.to_vec())?;

        assert_eq!(hash, expected_hash, "wrong hash");
        assert_eq!(raw_sig, expected_sig, "wrong signature");

        let signature = Signature::try_from(&raw_sig.as_slice()[..64])?;

        assert!(
            verifying_key.verify_prehash(&hash, &signature).is_ok(),
            "signature can't be verified"
        );

        Ok(())
    }
}
