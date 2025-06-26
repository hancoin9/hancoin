use serde::{Serialize, Deserialize};
use dashmap::DashMap;
use std::sync::atomic::{AtomicU64, Ordering};

// 极简参数：发行量、免费水龙头
pub const HAN_TOTAL_SUPPLY: u64 = 1_500_000_000 * 100_000;
pub const HAN_FAUCET_YEARS: u64 = 50;
pub const SECONDS_PER_DAY: u64 = 86400;
pub const HAN_FAUCET_DAILY: u64 = HAN_TOTAL_SUPPLY / (HAN_FAUCET_YEARS * 365);

// 极简账户
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Account {
    pub balance: u64,
    pub last_claim: u64,
}

// 极速并发账本
#[derive(Default)]
pub struct Ledger {
    pub accounts: DashMap<String, Account>,
    pub issued: AtomicU64,
}

// 转账
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Tx {
    pub from: String,
    pub to: String,
    pub amount: u64,
    pub signature: String,
}

// 朋友圈
#[derive(Clone, Serialize, Deserialize)]
pub struct Moment {
    pub from: String,
    pub text: String,
    pub timestamp: u64,
    pub signature: String,
}