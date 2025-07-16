use ed25519_dalek::{Signer, Verifier, Signature, SigningKey, VerifyingKey};
use once_cell::sync::Lazy;
use data_encoding::BASE32;
use crc::{Crc, CRC_32_ISO_HDLC};
use rand::{rngs::OsRng, Rng};
use std::sync::Once;
use parking_lot::Mutex;
use std::time::{Instant, Duration, SystemTime, UNIX_EPOCH};
use log::{warn, debug, error, info};
use dashmap::DashMap;
use rayon::prelude::*;
use subtle::ConstantTimeEq;
use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use chrono::{Utc, DateTime};
use tokio::sync::broadcast;
use std::sync::atomic::{AtomicUsize, Ordering};
use thiserror::Error;

/// 密钥轮换错误
#[derive(Debug, Error)]
pub enum KeyRotationError {
    #[error("Key rotation not needed")]
    NotNeeded,
    #[error("Key rotation failed: {0}")]
    Failed(String),
}

/// 密钥版本信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyVersion {
    pub id: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub active: bool,
}

/// 高性能密钥管理器
pub struct KeyManager {
    current_key: SigningKey,
    key_versions: DashMap<String, VerifyingKey>, // 使用DashMap替代HashMap，减少锁竞争
    key_rotation_interval: Duration,
    last_rotation: Instant,
    rotation_sender: broadcast::Sender<()>,
    mandatory_rotation: bool,
    max_key_usage: usize,
    usage_counter: AtomicUsize, // 使用原子计数器
    last_used: Instant,
}

impl Drop for KeyManager {
    fn drop(&mut self) {
        // 清除敏感数据
        self.key_versions.clear();
    }
}

impl KeyManager {
    /// 创建新的密钥管理器
    pub fn new(initial_key: SigningKey) -> Self {
        let (tx, _) = broadcast::channel(16); // 增加通道容量
        
        // 获取初始密钥的公钥
        let public_key = VerifyingKey::from(&initial_key);
        
        // 创建密钥管理器
        let mut manager = Self {
            current_key: initial_key,
            key_versions: DashMap::with_capacity(10), // 预分配容量
            key_rotation_interval: Duration::from_secs(86400), // 24小时轮换
            last_rotation: Instant::now(),
            rotation_sender: tx,
            mandatory_rotation: true,
            max_key_usage: 1000,
            usage_counter: AtomicUsize::new(0),
            last_used: Instant::now(),
        };
        
        // 添加初始密钥到版本列表
        let key_id = format!("key-{}", Utc::now().timestamp());
        manager.key_versions.insert(key_id, public_key);
        
        manager
    }

    /// 获取当前公钥
    #[inline]
    pub fn get_public_key(&self) -> VerifyingKey {
        VerifyingKey::from(&self.current_key)
    }

    /// 轮换密钥
    pub fn rotate_key(&mut self, force: bool) -> Result<(), KeyRotationError> {
        // 检查是否需要轮换
        if !force && !self.should_rotate() {
            return Err(KeyRotationError::NotNeeded);
        }

        // 生成新密钥
        let new_key = SigningKey::generate(&mut OsRng);
        
        // 获取旧密钥的公钥
        let old_public = VerifyingKey::from(&self.current_key);
        
        // 替换当前密钥
        self.current_key = new_key;
        
        // 保存旧公钥版本
        let key_id = format!("key-{}", Utc::now().timestamp());
        self.key_versions.insert(key_id.clone(), old_public);
        
        // 重置计数器
        self.usage_counter.store(0, Ordering::Relaxed);
        self.last_rotation = Instant::now();
        
        // 通知密钥轮换
        let _ = self.rotation_sender.send(());
        
        info!("Key rotated successfully. New key ID: {}", key_id);
        Ok(())
    }

    /// 检查是否需要轮换
    #[inline]
    fn should_rotate(&self) -> bool {
        self.last_rotation.elapsed() >= self.key_rotation_interval ||
        (self.mandatory_rotation && 
         self.usage_counter.load(Ordering::Relaxed) >= self.max_key_usage)
    }

    /// 检查并执行密钥轮换
    #[inline]
    pub fn check_rotation(&mut self) {
        if self.should_rotate() {
            let _ = self.rotate_key(false);
        }
    }

    /// 验证签名(支持多版本密钥)
    #[inline]
    pub fn verify_signature(
        &self,
        message: &[u8],
        signature: &Signature,
    ) -> bool {
        // 首先用当前密钥验证
        let current_public = VerifyingKey::from(&self.current_key);
        if current_public.verify(message, signature).is_ok() {
            return true;
        }

        // 用历史密钥验证，使用并行迭代器加速
        self.key_versions.iter()
            .any(|entry| entry.value().verify(message, signature).is_ok())
    }
    
    /// 使用当前密钥签名消息
    #[inline]
    pub fn sign_message(&self, message: &[u8]) -> Signature {
        // 增加使用计数
        self.increment_usage(1);
        
        // 签名消息
        self.current_key.sign(message)
    }
    
    /// 增加使用计数
    #[inline]
    pub fn increment_usage(&self, count: usize) {
        self.usage_counter.fetch_add(count, Ordering::Relaxed);
    }
}

lazy_static::lazy_static! {
    static ref KEY_MANAGER: Mutex<Option<KeyManager>> = Mutex::new(None);
}

// 用于初始化加密子系统的静态初始化器
static INIT: Once = Once::new();

/// 高性能签名缓存结构
pub struct SignatureCache {
    cache: DashMap<[u8; 64], (Instant, u64)>, // 固定大小签名 + (时间戳, 计数器)
    max_size: usize,
    ttl: Duration,
    cleanup_threshold: usize,  // 触发清理的阈值
    cleanup_counter: AtomicUsize, // 操作计数器
}

impl Drop for SignatureCache {
    fn drop(&mut self) {
        self.cache.clear();
    }
}

impl SignatureCache {
    /// 创建新的签名缓存
    #[inline]
    pub fn new(max_size: usize) -> Self {
        Self {
            cache: DashMap::with_capacity(max_size),
            max_size,
            ttl: Duration::from_secs(3600), // 默认1小时TTL
            cleanup_threshold: max_size / 10, // 当达到容量的10%时清理
            cleanup_counter: AtomicUsize::new(0),
        }
    }

    /// 插入签名到缓存
    #[inline]
    pub fn insert(&self, signature: [u8; 64]) -> bool {
        // 检查签名是否已存在
        if self.cache.contains_key(&signature) {
            warn!("Possible replay attack detected");
            return false;
        }
        
        let now = Instant::now();
        
        // 增加操作计数
        let counter = self.cleanup_counter.fetch_add(1, Ordering::Relaxed);
        
        // 周期性清理，而不是每次插入都检查
        if counter % self.cleanup_threshold == 0 {
            self.cleanup();
        }

        // 检查缓存大小
        if self.cache.len() >= self.max_size {
            // 只在真正需要时执行清理
            self.cleanup();
            
            // 如果仍然满了，移除最旧的条目
            if self.cache.len() >= self.max_size {
                if let Some((oldest_key, _)) = self.find_oldest_entry() {
                    self.cache.remove(&oldest_key);
                }
            }
        }

        // 插入新签名，使用固定大小数组作为键
        self.cache.insert(signature, (now, counter as u64));
        true
    }
    
    /// 检查签名是否在缓存中
    #[inline]
    pub fn check_signature(&self, signature: &[u8; 64]) -> bool {
        // 检查签名是否在缓存中
        if self.cache.contains_key(signature) {
            // 签名已存在，可能是重放攻击
            warn!("Signature replay detected");
            return false;
        }
        true
    }
    
    // 查找最旧的条目
    #[inline]
    fn find_oldest_entry(&self) -> Option<([u8; 64], (Instant, u64))> {
        self.cache.iter()
            .min_by_key(|entry| entry.value().0.elapsed())
            .map(|entry| (*entry.key(), *entry.value()))
    }
    
    // 清理过期签名
    #[inline]
    fn cleanup(&self) {
        let now = Instant::now();
        // 使用并行迭代器清理过期签名
        self.cache.retain(|_, (timestamp, _)| now.duration_since(*timestamp) < self.ttl);
    }
}

// 优化的全局签名缓存
static SIGNATURE_CACHE: Lazy<SignatureCache> = Lazy::new(|| {
    SignatureCache::new(10000) // 最多10000个签名
});

// 初始化加密子系统
pub fn init_crypto() {
    INIT.call_once(|| {
        // 这里可以进行一些加密库的初始化工作
        debug!("Crypto subsystem initialized");
        
        // 初始化全局密钥管理器
        let mut manager_lock = KEY_MANAGER.lock();
        if manager_lock.is_none() {
            // 生成初始密钥
            let initial_key = SigningKey::generate(&mut OsRng);
            *manager_lock = Some(KeyManager::new(initial_key));
        }
    });
}

/// 生成 Ed25519 密钥对
pub fn generate_keypair() -> SigningKey {
    init_crypto();
    SigningKey::generate(&mut OsRng)
}

/// 高性能签名消息
#[inline]
pub fn sign_message(keypair: &SigningKey, message: &[u8]) -> Signature {
    // 初始化加密子系统
    init_crypto();
    
    // 执行签名操作
    keypair.sign(message)
}

/// 验证签名
#[inline]
pub fn verify_signature(public_key: &VerifyingKey, message: &[u8], signature: &Signature) -> bool {
    // 尝试将签名转换为字节数组
    if let Ok(sig_array) = signature.to_bytes().try_into() {
        // 检查签名是否在缓存中(防止重放攻击)
        if !SIGNATURE_CACHE.check_signature(&sig_array) {
            return false;
        }
        
        // 验证签名
        let result = public_key.verify(message, signature).is_ok();
        
        // 如果验证成功，将签名添加到缓存
        if result {
            SIGNATURE_CACHE.insert(sig_array);
        }
        
        result
    } else {
        // 签名格式无效
        false
    }
}