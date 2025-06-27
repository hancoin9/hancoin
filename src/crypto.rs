use ed25519_dalek::{Keypair, PublicKey, Signature, Signer, Verifier};
use rand_core::OsRng; // 用 rand_core 提供的 OsRng

pub fn generate_keypair() -> Keypair {
    let mut csprng = OsRng;
    Keypair::generate(&mut csprng)
}

pub fn sign_message(keypair: &Keypair, msg: &[u8]) -> Signature {
    keypair.sign(msg)
}

pub fn verify_signature(pubkey: &PublicKey, msg: &[u8], sig: &Signature) -> bool {
    pubkey.verify(msg, sig).is_ok()
}