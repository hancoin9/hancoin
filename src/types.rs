use serde::{Serialize, Deserialize};
use dashmap::DashMap;
use std::sync::atomic::{AtomicU64, Ordering};

// 极简参数：发行量、免费水龙头
pub const HAN_TOTAL_SUPPLY: u64 = 10_000_000_000 * 100_000;// 100亿
pub fn yearly_distribution(year: u32) -> u64 {
    match year {
        1 => HAN_TOTAL_SUPPLY * 20 / 100,
        2 => HAN_TOTAL_SUPPLY * 10 / 100,
        3 => HAN_TOTAL_SUPPLY * 5 / 100,
        4 => HAN_TOTAL_SUPPLY * 3 / 100,
        5 => HAN_TOTAL_SUPPLY * 2 / 100,
        6..=105 => {
            let remaining = HAN_TOTAL_SUPPLY * 60 / 100; // 剩余60%
            remaining / 100 // 100年平均分配
        }
        _ => 0,
    }
}

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
