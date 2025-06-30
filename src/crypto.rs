use ed25519_dalek::{Keypair, PublicKey, Signature, Signer, Verifier};
use rand::rngs::OsRng;

/// 生成 Ed25519 密钥对
pub fn generate_keypair() -> Keypair {
    let mut csprng = OsRng;
    Keypair::generate(&mut csprng)
}

/// 签名消息
pub fn sign_message(keypair: &Keypair, message: &[u8]) -> Signature {
    keypair.sign(message)
}

/// 验证签名
pub fn verify_signature(
    public_key: &PublicKey,
    message: &[u8],
    signature: &Signature,
) -> bool {
    public_key.verify(message, signature).is_ok()
}