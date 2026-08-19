use std::sync::OnceLock;

use rsa::pkcs8::{EncodePrivateKey, EncodePublicKey, LineEnding};
use rsa::{RsaPrivateKey, RsaPublicKey};

pub struct TestKeypair {
    pub signing_pem: String,
    pub verifying_pem: String,
}

static KEYPAIR: OnceLock<TestKeypair> = OnceLock::new();

pub fn keypair() -> &'static TestKeypair {
    KEYPAIR.get_or_init(|| {
        let mut rng = rand::thread_rng();
        let private = RsaPrivateKey::new(&mut rng, 2048).expect("a test key is generated");
        let public = RsaPublicKey::from(&private);

        TestKeypair {
            signing_pem: private
                .to_pkcs8_pem(LineEnding::LF)
                .expect("private key encodes")
                .to_string(),
            verifying_pem: public
                .to_public_key_pem(LineEnding::LF)
                .expect("public key encodes"),
        }
    })
}

pub fn signing_key() -> &'static [u8] {
    keypair().signing_pem.as_bytes()
}

pub fn verifying_key() -> &'static [u8] {
    keypair().verifying_pem.as_bytes()
}
