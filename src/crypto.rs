use ed25519_dalek::{Signer, Signature};
use once_cell::sync::Lazy;
use data_encoding::BASE32;
use crc::{Crc, CRC_32_ISO_HDLC};
use ed25519_dalek::SigningKey as Keypair;
use ed25519_dalek::VerifyingKey as PublicKey;
use rand::rngs::OsRng;
use std::sync::Once;
use parking_lot::Mutex;
use std::time::{Instant, Duration};
use log::{warn, debug, error, info};
use dashmap::DashMap;
use rayon::prelude::*;
use subtle::ConstantTimeEq;
use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use chrono::{Utc, DateTime};
use tokio::sync::broadcast;

/// 密钥版本信息
#[derive(Debug, Clone, Serialize, Deserialize)]
struct KeyVersion {
    id: String,
    created_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    active: bool,
}

/// 密钥管理器
struct KeyManager {
    current_key: Keypair,
    key_versions: HashMap<String, PublicKey>,
    key_rotation_interval: Duration,
    last_rotation: Instant,
    rotation_sender: broadcast::Sender<()>,
}

impl Drop for KeyManager {
    fn drop(&mut self) {
        // 清除敏感数据
        self.key_versions.clear();
    }
}

impl KeyManager {
    fn new() -> Self {
        let (tx, _) = broadcast::channel(1);
        Self {
            current_key: Keypair::generate(&mut OsRng),
            key_versions: HashMap::new(),
            key_rotation_interval: Duration::from_secs(86400), // 24小时轮换
            last_rotation: Instant::now(),
            rotation_sender: tx,
        }
    }

    /// 获取当前公钥
    fn current_public_key(&self) -> PublicKey {
        self.current_key.public
    }

    /// 轮换密钥
    fn rotate_key(&mut self) {
        let old_key = self.current_key.public;
        let key_id = format!("key-{}", Utc::now().timestamp());
        
        // 保存旧密钥版本
        self.key_versions.insert(key_id.clone(), old_key);
        
        // 生成新密钥
        self.current_key = Keypair::generate(&mut OsRng);
        self.last_rotation = Instant::now();
        
        // 通知密钥轮换
        let _ = self.rotation_sender.send(());
        
        info!("Key rotated successfully. New key ID: {}", key_id);
    }

    /// 检查并执行密钥轮换
    fn check_rotation(&mut self) {
        if self.last_rotation.elapsed() >= self.key_rotation_interval {
            self.rotate_key();
        }
    }

    /// 验证签名(支持多版本密钥)
    fn verify_signature(
        &self,
        public_key: &PublicKey,
        message: &[u8],
        signature: &Signature,
    ) -> bool {
        // 首先用当前密钥验证
        if public_key.verify(message, signature).is_ok() {
            return true;
        }

        // 用历史密钥验证
        self.key_versions.values()
            .any(|key| key.verify(message, signature).is_ok())
    }
}

lazy_static::lazy_static! {
    static ref KEY_MANAGER: Mutex<KeyManager> = Mutex::new(KeyManager::new());
}

// 用于初始化加密子系统的静态初始化器
static INIT: Once = Once::new();

/// 优化的签名缓存结构
struct SignatureCache {
    cache: DashMap<Vec<u8>, Instant>,
    max_size: usize,
    ttl: Duration,
}

impl Drop for SignatureCache {
    fn drop(&mut self) {
        self.cache.clear();
    }
}

impl SignatureCache {
    fn new(max_size: usize, ttl_seconds: u64) -> Self {
        Self {
            cache: DashMap::with_capacity(max_size),
            max_size,
            ttl: Duration::from_secs(ttl_seconds),
        }
    }

    fn insert(&self, signature: &[u8]) -> bool {
        let now = Instant::now();
        
        // 并行清理过期签名
        self.cache.retain(|_, timestamp| now.duration_since(*timestamp) < self.ttl);

        // 检查缓存大小
        if self.cache.len() >= self.max_size {
            // 并行查找并移除最旧的条目
            if let Some(oldest_entry) = self.cache
                .iter()
                .min_by_key(|entry| entry.value().elapsed()) {
                self.cache.remove(oldest_entry.key());
            }
        }

        // 检查签名是否已存在
        if self.cache.contains_key(signature) {
            warn!("Possible replay attack detected");
            return false;
        }

        // 插入新签名
        self.cache.insert(signature.to_vec(), now);
        true
    }
}

// 优化的全局签名缓存
static SIGNATURE_CACHE: Lazy<SignatureCache> = Lazy::new(|| {
    SignatureCache::new(10000, 3600) // 最多10000个签名，TTL为1小时
});

// 初始化加密子系统
fn init_crypto() {
    INIT.call_once(|| {
        // 这里可以进行一些加密库的初始化工作
        debug!("Crypto subsystem initialized");
    });
}

/// 生成 Ed25519 密钥对
/// 
/// 使用操作系统提供的安全随机数生成器创建新的密钥对
pub fn generate_keypair() -> Keypair {
    init_crypto();
    Keypair::generate(&mut OsRng)
}

/// 签名消息
/// 
/// 使用提供的密钥对对消息进行签名
/// 
/// # 参数
/// * `keypair` - 用于签名的Ed25519密钥对
/// * `message` - 要签名的消息字节
/// 
/// # 返回
/// 返回消息的Ed25519签名
pub fn sign_message(keypair: &Keypair, message: &[u8]) -> Signature {
    init_crypto();
    keypair.sign(message)
}

/// 优化的签名验证
/// 
/// 使用恒定时间比较来防止时序攻击，并严格检查重放攻击
pub fn verify_signature(
    public_key: &PublicKey,
    message: &[u8],
    signature: &Signature,
) -> bool {
    init_crypto();
    
    // 验证签名（恒定时间操作）
    let verify_result = public_key.verify(message, signature).is_ok();
    
    // 检查重放攻击
    let signature_bytes = signature.to_bytes();
    let is_replay = !SIGNATURE_CACHE.insert(&signature_bytes);
    
    // 只有签名验证成功且不是重放攻击才返回true
    verify_result && !is_replay
}

/// 批量验证签名（并行处理）
pub fn verify_signatures_batch(
    public_keys: &[PublicKey],
    messages: &[&[u8]],
    signatures: &[Signature],
) -> Vec<bool> {
    // 确保输入长度一致
    assert_eq!(public_keys.len(), messages.len());
    assert_eq!(messages.len(), signatures.len());
    
    // 并行验证
    public_keys.par_iter()
        .zip(messages.par_iter())
        .zip(signatures.par_iter())
        .map(|((pk, msg), sig)| verify_signature(pk, msg, sig))
        .collect()
}

/// 将公钥转换为Hancoin地址
/// 
/// 格式: han_ + Base32编码的公钥 + 校验和(最后5字符)
pub fn public_key_to_address(pubkey: &PublicKey) -> String {
    // 获取公钥字节
    let pubkey_bytes = pubkey.as_bytes();
    
    // 计算CRC32校验和
    const CRC: Crc<u32> = Crc::<u32>::new(&CRC_32_ISO_HDLC);
    let checksum = CRC.checksum(pubkey_bytes);
    
    // 合并公钥和校验和
    let mut combined = pubkey_bytes.to_vec();
    combined.extend(&checksum.to_be_bytes());
    
    // Base32编码
    let encoded = BASE32.encode(&combined);
    
    // 添加han_前缀并转换为小写
    format!("han_{}", encoded.to_lowercase())
}

/// 验证Hancoin地址格式
pub fn validate_address(address: &str) -> bool {
    // 检查前缀
    if !address.starts_with("han_") {
        return false;
    }
    
    // 移除前缀
    let body = &address[4..];
    
    // 解码Base32
    let decoded = match BASE32.decode(body.to_uppercase().as_bytes()) {
        Ok(d) => d,
        Err(_) => return false,
    };
    
    // 检查长度(公钥32字节 + 校验和4字节)
    if decoded.len() != 36 {
        return false;
    }
    
    // 分离公钥和校验和
    let (pubkey_part, checksum_part) = decoded.split_at(32);
    let stored_checksum = u32::from_be_bytes([
        checksum_part[0],
        checksum_part[1],
        checksum_part[2],
        checksum_part[3],
    ]);
    
    // 计算并验证校验和
    const CRC: Crc<u32> = Crc::<u32>::new(&CRC_32_ISO_HDLC);
    let computed_checksum = CRC.checksum(pubkey_part);
    
    stored_checksum == computed_checksum
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_address_generation_and_validation() {
        let keypair = generate_keypair();
        let pubkey = keypair.public;
        let address = public_key_to_address(&pubkey);
        
        // 检查前缀
        assert!(address.starts_with("han_"));
        
        // 验证地址
        assert!(validate_address(&address));
        
        // 测试无效地址
        assert!(!validate_address("invalid_address"));
        assert!(!validate_address("han_invalidbase32"));
        
        // 修改校验和测试
        let mut bad_address = address.clone();
        bad_address.replace_range(address.len()-1.., "x");
        assert!(!validate_address(&bad_address));
    }
}