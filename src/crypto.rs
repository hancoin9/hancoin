use ed25519_dalek::{Keypair, PublicKey, Signature, Signer, Verifier};
use rand::rngs::OsRng;

pub fn gen_keypair() -> Keypair {
    Keypair::generate(&mut OsRng)
}

pub fn sign(msg: &[u8], kp: &Keypair) -> Signature {
    kp.sign(msg)
}

pub fn verify(msg: &[u8], sig: &Signature, pubkey: &PublicKey) -> bool {
    pubkey.verify(msg, sig).is_ok()
}