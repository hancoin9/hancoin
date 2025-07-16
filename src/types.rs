use serde::{Serialize, Deserialize};
use serde_with::{serde_as, skip_serializing_none};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::collections::{VecDeque, HashMap};
use log::{warn, info, debug, error};
use std::time::{SystemTime, UNIX_EPOCH};
use regex::Regex;
use lazy_static::lazy_static;
use sha2::{Digest, Sha256};
use parking_lot::RwLock;
use once_cell::sync::Lazy;
use std::hash::Hasher;
use twox_hash::XxHash64;
use dashmap::DashMap;
use lru::LruCache;
use serde_bytes;

// 使用once_cell替代lazy_static
static ACCOUNT_ID_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^[1-9A-HJ-NP-Za-km-z]{32,44}$").expect("Invalid account ID regex")
});

// 优化的哈希器实例
static HASHER: Lazy<XxHash64> = Lazy::new(|| XxHash64::default());

/// 验证账户ID格式
pub fn is_valid_account_id(account_id: &str) -> bool {
    ACCOUNT_ID_REGEX.is_match(account_id)
}

/// 最大交易历史记录数
const MAX_TX_HISTORY: usize = 100;
/// 最大动态消息长度
const MAX_MOMENT_LENGTH: usize = 280;

/// 水龙头冷却时间(秒)
pub const FAUCET_COOLDOWN: u64 = 86400; // 24小时

/// 每日水龙头限额
pub const FAUCET_DAILY_LIMIT: u64 = 100_000; // 100,000 HAN

/// HAN 总发行量（100亿 * 100_000 = 1_000_000_000_000）
pub const HAN_TOTAL_SUPPLY: u64 = 1_000_000_000_000;

/// 按年度返回当年分配量
pub fn yearly_distribution(year: u32) -> Option<u64> {
    if year == 0 {
        return Some(0);
    }
    
    match year {
        1 => HAN_TOTAL_SUPPLY.checked_mul(20)?.checked_div(100),
        2 => HAN_TOTAL_SUPPLY.checked_mul(10)?.checked_div(100),
        3 => HAN_TOTAL_SUPPLY.checked_mul(5)?.checked_div(100),
        4 => HAN_TOTAL_SUPPLY.checked_mul(3)?.checked_div(100),
        5 => HAN_TOTAL_SUPPLY.checked_mul(2)?.checked_div(100),
        6..=105 => {
            let remaining = HAN_TOTAL_SUPPLY.checked_mul(60)?.checked_div(100)?;
            remaining.checked_div(100)
        },
        _ => Some(0),
    }
}

/// 账户结构体，包含余额和状态信息
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Account {
    /// 账户余额
    pub balance: u64,
    /// 上次领取水龙头时间
    pub last_claim: u64,
    /// 交易历史记录
    #[serde(default)]
    pub transactions: VecDeque<TxRef>,
    /// 账户创建时间
    #[serde(default)]
    pub created_at: u64,
    /// 最后活动时间
    #[serde(default)]
    pub last_active: u64,
    /// 交易nonce，防止重放攻击
    #[serde(default)]
    pub nonce: u64,
    /// 账户状态
    #[serde(default)]
    pub status: AccountStatus,
}

/// 账户状态
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub enum AccountStatus {
    #[default]
    Active,
    Frozen,
    Restricted,
}

impl Default for Account {
    fn default() -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
            
        Self {
            balance: 0,
            last_claim: 0,
            transactions: VecDeque::with_capacity(100), // 保存最近100笔交易
            created_at: now,
            last_active: now,
            nonce: 0,
            status: AccountStatus::Active,
        }
    }
}

impl Account {
    /// 更新账户的最后活动时间
    pub fn update_activity(&mut self) {
        self.last_active = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
    }
    
    /// 添加交易引用
    pub fn add_transaction(&mut self, tx_ref: TxRef) {
        // 保持交易历史在100条以内
        if self.transactions.len() >= MAX_TX_HISTORY {
            self.transactions.pop_front();
        }
        self.transactions.push_back(tx_ref);
        self.update_activity();
    }
    
    /// 检查账户是否可以领取水龙头
    pub fn can_claim_faucet(&self) -> Result<(), String> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
            
        if now - self.last_claim < FAUCET_COOLDOWN {
            let wait_time = FAUCET_COOLDOWN - (now - self.last_claim);
            return Err(format!(
                "Too frequent requests. Please wait {} seconds before claiming again.",
                wait_time
            ));
        }
        
        Ok(())
    }
}

/// 交易引用，用于在账户中存储交易历史
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TxRef {
    pub tx_id: String,
    pub timestamp: u64,
    pub amount: u64,
    pub is_incoming: bool,
}

/// 交易结构体
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Tx {
    pub id: String,
    pub from: String,
    pub to: String,
    pub amount: u64,
    pub timestamp: u64,
    pub fee: u64,
    pub memo: Option<String>,
    pub status: TxStatus,
}

/// 交易状态
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum TxStatus {
    Pending,
    Completed,
    Failed(String),
}

/// 动态消息结构体
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Moment {
    pub id: String,
    pub author: String,
    pub content: String,
    pub timestamp: u64,
    pub likes: u64,
    pub reposts: u64,
    pub comments: Vec<Comment>,
}

/// 评论结构体
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Comment {
    pub id: String,
    pub author: String,
    pub content: String,
    pub timestamp: u64,
}

/// 优化的账本结构体
#[serde_as]
#[derive(Serialize, Deserialize)]
pub struct Ledger {
    pub accounts: Arc<DashMap<String, Account>>,
    pub issued: AtomicU64,
    pub transactions: Arc<DashMap<String, Tx>>,
    pub moments: Arc<DashMap<String, Moment>>,
    // 添加缓存优化频繁访问的数据
    #[serde(skip)]
    pub cache: Arc<RwLock<LruCache<String, Account>>>,
    // 缓存统计
    #[serde(skip)]
    pub cache_hits: AtomicU64,
    #[serde(skip)]
    pub cache_misses: AtomicU64,
}

impl Default for Ledger {
    fn default() -> Self {
        Self {
            accounts: Arc::new(DashMap::new()),
            issued: AtomicU64::new(0),
            transactions: Arc::new(DashMap::new()),
            moments: Arc::new(DashMap::new()),
            cache: Arc::new(RwLock::new(LruCache::new(1000))), // 缓存1000个账户
            cache_hits: AtomicU64::new(0),
            cache_misses: AtomicU64::new(0),
        }
    }
}

impl Ledger {
    /// 创建新的账本实例
    pub fn new() -> Self {
        Self::default()
    }

    /// 获取账户信息，优先使用缓存
    pub fn get_account(&self, account_id: &str) -> Option<Account> {
        let cache = self.cache.read();
        if let Some(account) = cache.get(account_id) {
            self.cache_hits.fetch_add(1, Ordering::Relaxed);
            return Some(account.clone());
        }
        self.cache_misses.fetch_add(1, Ordering::Relaxed);

        drop(cache);

        if let Some(account) = self.accounts.get(account_id) {
            let mut cache = self.cache.write();
            cache.put(account_id.to_string(), account.clone());
            Some(account.clone())
        } else {
            None
        }
    }
    
    /// 批量获取账户信息，优先使用缓存
    pub fn get_accounts_batch(&self, account_ids: &[String]) -> HashMap<String, Account> {
        let mut result = HashMap::with_capacity(account_ids.len());
        
        // 先从缓存中获取
        let mut cache_miss = Vec::new();
        {
            let cache = self.cache.read();
            for id in account_ids {
                if let Some(account) = cache.get(id) {
                    result.insert(id.clone(), account.clone());
                    self.cache_hits.fetch_add(1, Ordering::Relaxed);
                } else {
                    cache_miss.push(id.clone());
                    self.cache_misses.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
        
        // 对于缓存未命中的账户，从存储中获取
        for id in cache_miss {
            if let Some(account) = self.accounts.get(&id) {
                let account_clone = account.clone();
                result.insert(id.clone(), account_clone.clone());
                
                // 更新缓存
                let mut cache = self.cache.write();
                cache.put(id, account_clone);
            }
        }
        
        result
    }
    
    /// 批量获取动态信息
    pub fn get_moments_batch(&self, moment_ids: &[String]) -> HashMap<String, Moment> {
        let mut result = HashMap::with_capacity(moment_ids.len());
        
        for id in moment_ids {
            if let Some(moment) = self.moments.get(id) {
                result.insert(id.clone(), moment.clone());
            }
        }
        
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_account_activity() {
        let mut account = Account::default();
        let old_active = account.last_active;
        account.update_activity();
        assert!(account.last_active > old_active);
    }
    
    #[test]
    fn test_tx_ref_management() {
        let mut account = Account::default();
        let tx_ref = TxRef {
            tx_id: "test123".to_string(),
            timestamp: 123456789,
            amount: 100,
            is_incoming: true,
        };
        
        account.add_transaction(tx_ref.clone());
        assert_eq!(account.transactions.len(), 1);
        assert_eq!(account.transactions[0].tx_id, "test123");
    }
    
    #[test]
    fn test_ledger_cache() {
        let ledger = Ledger::new();
        let account_id = "test_account".to_string();
        let account = Account::default();
        
        ledger.accounts.insert(account_id.clone(), account.clone());
        let batch = ledger.get_accounts_batch(&[account_id.clone()]);
        
        assert_eq!(batch.len(), 1);
        assert_eq!(batch[&account_id].balance, account.balance);
    }
}

use thiserror::Error;

#[derive(Error, Debug)]
pub enum HancoinError {
    #[error("Missing account_id")]
    MissingAccountId,
    #[error("Missing signature")]
    MissingSignature,
    #[error("Invalid account_id format")]
    InvalidAccountIdFormat,
    #[error("Invalid public key")]
    InvalidPublicKey,
    #[error("Invalid signature format")]
    InvalidSignatureFormat,
    #[error("Invalid signature data")]
    InvalidSignatureData,
    #[error("Invalid signature")]
    InvalidSignature,
    #[error("System time error")]
    SystemTimeError,
    #[error("Faucet cooldown period not over")]
    FaucetCooldownNotOver,
    #[error("Total supply limit reached")]
    TotalSupplyLimitReached,
    #[error("Account not found")]
    AccountNotFound,
    #[error("Invalid transaction")]
    InvalidTransaction,
    #[error("Session not found: {0}")]
    SessionNotFound(String),
}