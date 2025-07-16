//! CoinJoin混币功能模块
//! 
//! 本模块提供了CoinJoin混币功能，允许多个用户将他们的交易合并成一个交易，
//! 从而提高交易的隐私性，使外部观察者难以确定哪些输入对应哪些输出。

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use log::{debug, info, warn, error};
use serde::{Serialize, Deserialize};
use uuid::Uuid;
use tokio::sync::mpsc;
use axum::{
    extract::{Path, State, Json},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use dashmap::DashMap;

/// CoinJoin会话状态
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CoinJoinStatus {
    /// 等待参与者加入
    Waiting,
    /// 收集输入中
    CollectingInputs,
    /// 收集输出中
    CollectingOutputs,
    /// 收集签名中
    CollectingSignatures,
    /// 广播交易中
    Broadcasting,
    /// 已完成
    Completed,
    /// 已失败
    Failed,
    /// 已超时
    TimedOut,
}

/// 交易输入
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxInput {
    /// 交易ID
    pub txid: String,
    /// 输出索引
    pub vout: u32,
    /// 金额
    pub amount: u64,
    /// 脚本
    pub script: String,
    /// 公钥
    pub pubkey: String,
}

/// 交易输出
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxOutput {
    /// 接收地址
    pub address: String,
    /// 金额
    pub amount: u64,
}

/// 交易签名
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxSignature {
    /// 输入索引
    pub input_index: usize,
    /// 签名数据
    pub signature: String,
    /// 公钥
    pub pubkey: String,
}

/// CoinJoin会话
#[derive(Debug)]
pub struct CoinJoinSession {
    /// 会话ID
    pub id: String,
    /// 会话状态
    pub status: CoinJoinStatus,
    /// 创建时间
    pub created_at: u64,
    /// 最后活动时间
    pub last_active: u64,
    /// 最小参与者数量
    pub min_participants: usize,
    /// 最大参与者数量
    pub max_participants: usize,
    /// 目标金额
    pub target_amount: u64,
    /// 交易费率（聪/字节）
    pub fee_rate: u64,
    /// 超时时间（秒）
    pub timeout: u64,
    /// 参与者
    pub participants: HashSet<String>,
    /// 交易输入
    pub inputs: Vec<TxInput>,
    /// 交易输出
    pub outputs: Vec<TxOutput>,
    /// 交易签名
    pub signatures: Vec<TxSignature>,
    /// 最终交易ID
    pub final_txid: Option<String>,
}

impl CoinJoinSession {
    /// 创建新的CoinJoin会话
    pub fn new(
        min_participants: usize,
        max_participants: usize,
        target_amount: u64,
        fee_rate: u64,
        timeout: u64,
    ) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
            
        Self {
            id: Uuid::new_v4().to_string(),
            status: CoinJoinStatus::Waiting,
            created_at: now,
            last_active: now,
            min_participants,
            max_participants,
            target_amount,
            fee_rate,
            timeout,
            participants: HashSet::new(),
            inputs: Vec::new(),
            outputs: Vec::new(),
            signatures: Vec::new(),
            final_txid: None,
        }
    }
    
    /// 添加参与者
    pub fn add_participant(&mut self, participant_id: &str) -> bool {
        if self.status != CoinJoinStatus::Waiting {
            return false;
        }
        
        self.participants.insert(participant_id.to_string());
        self.update_last_active();
        
        // 如果达到最小参与者数量，进入下一阶段
        if self.participants.len() >= self.min_participants {
            self.status = CoinJoinStatus::CollectingInputs;
        }
        
        true
    }
    
    /// 添加交易输入
    pub fn add_input(&mut self, input: TxInput) -> bool {
        if self.status != CoinJoinStatus::CollectingInputs {
            return false;
        }
        
        self.inputs.push(input);
        self.update_last_active();
        
        // 如果每个参与者都提供了至少一个输入，进入下一阶段
        if self.inputs.len() >= self.participants.len() {
            self.status = CoinJoinStatus::CollectingOutputs;
        }
        
        true
    }
    
    /// 添加交易输出
    pub fn add_output(&mut self, output: TxOutput) -> bool {
        if self.status != CoinJoinStatus::CollectingOutputs {
            return false;
        }
        
        self.outputs.push(output);
        self.update_last_active();
        
        // 如果每个参与者都提供了至少一个输出，进入下一阶段
        if self.outputs.len() >= self.participants.len() {
            self.status = CoinJoinStatus::CollectingSignatures;
        }
        
        true
    }
    
    /// 添加交易签名
    pub fn add_signature(&mut self, signature: TxSignature) -> bool {
        if self.status != CoinJoinStatus::CollectingSignatures {
            return false;
        }
        
        // 验证输入索引是否有效
        if signature.input_index >= self.inputs.len() {
            return false;
        }
        
        self.signatures.push(signature);
        self.update_last_active();
        
        // 如果所有输入都已签名，进入下一阶段
        if self.signatures.len() >= self.inputs.len() {
            self.status = CoinJoinStatus::Broadcasting;
        }
        
        true
    }
    
    /// 完成会话
    pub fn complete(&mut self, txid: &str) -> bool {
        if self.status != CoinJoinStatus::Broadcasting {
            return false;
        }
        
        self.final_txid = Some(txid.to_string());
        self.status = CoinJoinStatus::Completed;
        self.update_last_active();
        
        true
    }
    
    /// 标记会话失败
    pub fn fail(&mut self) {
        self.status = CoinJoinStatus::Failed;
        self.update_last_active();
    }
    
    /// 检查会话是否已超时
    pub fn check_timeout(&mut self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
            
        if now - self.last_active > self.timeout {
            self.status = CoinJoinStatus::TimedOut;
            return true;
        }
        
        false
    }
    
    /// 更新最后活动时间
    fn update_last_active(&mut self) {
        self.last_active = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
    }
    
    /// 获取会话信息
    pub fn get_info(&self) -> CoinJoinSessionInfo {
        CoinJoinSessionInfo {
            id: self.id.clone(),
            status: self.status.clone(),
            created_at: self.created_at,
            last_active: self.last_active,
            min_participants: self.min_participants,
            max_participants: self.max_participants,
            target_amount: self.target_amount,
            fee_rate: self.fee_rate,
            timeout: self.timeout,
            participants_count: self.participants.len(),
            inputs_count: self.inputs.len(),
            outputs_count: self.outputs.len(),
            signatures_count: self.signatures.len(),
            final_txid: self.final_txid.clone(),
        }
    }
}

/// CoinJoin会话信息（用于API响应）
#[derive(Debug, Serialize)]
pub struct CoinJoinSessionInfo {
    /// 会话ID
    pub id: String,
    /// 会话状态
    pub status: CoinJoinStatus,
    /// 创建时间
    pub created_at: u64,
    /// 最后活动时间
    pub last_active: u64,
    /// 最小参与者数量
    pub min_participants: usize,
    /// 最大参与者数量
    pub max_participants: usize,
    /// 目标金额
    pub target_amount: u64,
    /// 交易费率（聪/字节）
    pub fee_rate: u64,
    /// 超时时间（秒）
    pub timeout: u64,
    /// 参与者数量
    pub participants_count: usize,
    /// 输入数量
    pub inputs_count: usize,
    /// 输出数量
    pub outputs_count: usize,
    /// 签名数量
    pub signatures_count: usize,
    /// 最终交易ID
    pub final_txid: Option<String>,
}

/// CoinJoin会话创建请求
#[derive(Debug, Deserialize)]
pub struct CoinJoinRequest {
    /// 最小参与者数量
    pub min_participants: Option<usize>,
    /// 最大参与者数量
    pub max_participants: Option<usize>,
    /// 目标金额
    pub target_amount: u64,
    /// 交易费率（聪/字节）
    pub fee_rate: Option<u64>,
    /// 超时时间（秒）
    pub timeout: Option<u64>,
    /// 参与者ID
    pub participant_id: String,
}

/// CoinJoin输入请求
#[derive(Debug, Deserialize)]
pub struct InputRequest {
    /// 参与者ID
    pub participant_id: String,
    /// 交易输入
    pub input: TxInput,
}

/// CoinJoin输出请求
#[derive(Debug, Deserialize)]
pub struct OutputRequest {
    /// 参与者ID
    pub participant_id: String,
    /// 交易输出
    pub output: TxOutput,
}

/// CoinJoin签名请求
#[derive(Debug, Deserialize)]
pub struct SignatureRequest {
    /// 参与者ID
    pub participant_id: String,
    /// 交易签名
    pub signature: TxSignature,
}

/// CoinJoin完成请求
#[derive(Debug, Deserialize)]
pub struct FinalizeRequest {
    /// 参与者ID
    pub participant_id: String,
}

/// CoinJoin会话管理器
pub struct CoinJoinManager {
    /// 会话映射表
    sessions: DashMap<String, CoinJoinSession>,
    /// 会话超时时间（秒）
    session_timeout: u64,
    /// 清理任务通道
    _cleanup_tx: Option<mpsc::Sender<()>>,
}

impl CoinJoinManager {
    /// 创建新的CoinJoin会话管理器
    pub fn new(session_timeout: u64) -> Self {
        let (tx, mut rx) = mpsc::channel::<()>(1);
        
        // 启动清理任务
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60));
            
            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        // 定期清理过期会话
                        debug!("执行CoinJoin会话清理");
                    }
                    _ = rx.recv() => {
                        debug!("CoinJoin会话清理任务退出");
                        break;
                    }
                }
            }
        });
        
        Self {
            sessions: DashMap::new(),
            session_timeout,
            _cleanup_tx: Some(tx),
        }
    }
    
    /// 创建新的CoinJoin会话
    pub fn create_session(&self, req: &CoinJoinRequest) -> CoinJoinSessionInfo {
        let min_participants = req.min_participants.unwrap_or(3);
        let max_participants = req.max_participants.unwrap_or(10);
        let fee_rate = req.fee_rate.unwrap_or(1);
        let timeout = req.timeout.unwrap_or(self.session_timeout);
        
        let mut session = CoinJoinSession::new(
            min_participants,
            max_participants,
            req.target_amount,
            fee_rate,
            timeout,
        );
        
        // 添加创建者作为第一个参与者
        session.add_participant(&req.participant_id);
        
        let session_info = session.get_info();
        self.sessions.insert(session.id.clone(), session);
        
        info!("创建新的CoinJoin会话: {}", session_info.id);
        session_info
    }
    
    /// 获取会话
    pub fn get_session(&self, id: &str) -> Option<CoinJoinSession> {
        self.sessions.get(id).map(|s| s.clone())
    }
    
    /// 添加交易输入
    pub fn add_input(&self, session_id: &str, req: &InputRequest) -> Result<CoinJoinSessionInfo, String> {
        let mut session = self.sessions.get_mut(session_id)
            .ok_or_else(|| format!("会话不存在: {}", session_id))?;
            
        if !session.participants.contains(&req.participant_id) {
            return Err(format!("参与者不在会话中: {}", req.participant_id));
        }
        
        if !session.add_input(req.input.clone()) {
            return Err(format!("无法添加输入，会