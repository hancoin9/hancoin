use serde::{Serialize, Deserialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{RwLock, Arc, HashMap};

/// HAN 总发行量（100亿 * 100_000 = 1_000_000_000_000）
pub const HAN_TOTAL_SUPPLY: u64 = 1_000_000_000_000;

/// 按年度返回当年分配量
pub fn yearly_distribution(year: u32) -> Option<u64> {
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

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Account {
    pub balance: u64,
    pub last_claim: u64,
}

use dashmap::DashMap;

#[derive(Default)]
pub struct Ledger {
    pub accounts: Arc<DashMap<String, Account>>,
    pub issued: AtomicU64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Tx {
    pub from: String,
    pub to: String,
    pub amount: u64,
    pub signature: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Moment {
    pub from: String,
    pub text: String,
    pub timestamp: u64,
    pub signature: String,
}

/// 水龙头每日领取限额
pub const FAUCET_DAILY_LIMIT: u64 = 100;
/// 水龙头领取冷却时间（秒）
pub const FAUCET_COOLDOWN: u64 = 86400; // 24小时