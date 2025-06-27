use ed25519_dalek::{Keypair, PublicKey, Signature, Signer, Verifier};
use rand::rngs::OsRng;

/// 生成新密钥对
pub fn generate_keypair() -> Keypair {
    let mut csprng = OsRng;
    Keypair::generate(&mut csprng)
}

/// 签名
pub fn sign_message(keypair: &Keypair, msg: &[u8]) -> Signature {
    keypair.sign(msg)
}

/// 验证签名
pub fn verify_signature(pubkey: &PublicKey, msg: &[u8], sig: &Signature) -> bool {
    pubkey.verify(msg, sig).is_ok()
}