use serde::{Serialize, Deserialize};
use dashmap::DashMap;
use std::sync::atomic::{AtomicU64, Ordering};

pub const HAN_TOTAL_SUPPLY: u64 = 1_500_000_000 * 100_000;
pub const HAN_FAUCET_YEARS: u64 = 50;
pub const SECONDS_PER_DAY: u64 = 86400;
pub const HAN_FAUCET_DAILY: u64 = HAN_TOTAL_SUPPLY / (HAN_FAUCET_YEARS * 365);

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