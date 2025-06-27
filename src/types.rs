use serde::{Serialize, Deserialize};
use dashmap::DashMap;
use std::sync::atomic::{AtomicU64, Ordering};

<<<<<<< HEAD
/// HAN 总发行量（100亿 * 100_000 = 1_000_000_000_000）
pub const HAN_TOTAL_SUPPLY: u64 = 10_000_000_000 * 100_000;

/// 按年度返回当年分配量
///
/// - 第1年：20%
/// - 第2年：10%
/// - 第3年：5%
/// - 第4年：3%
/// - 第5年：2%
/// - 第6-105年：剩余60%平均分配
/// - 其他年份返回0
=======
// 极简参数：发行量、免费水龙头
pub const HAN_TOTAL_SUPPLY: u64 = 10_000_000_000 * 100_000;// 100亿
>>>>>>> 164e387645567593aef151eecc431e4513cd90cf
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

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Account {
    pub balance: u64,
    pub last_claim: u64,
}

#[derive(Default)]
pub struct Ledger {
    pub accounts: DashMap<String, Account>,
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
