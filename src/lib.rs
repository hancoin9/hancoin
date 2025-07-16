//! Hancoin核心库
//!
//! 这个库包含了Hancoin加密货币的核心功能实现，
//! 包括加密、P2P网络、交易处理等组件。

/// 加密子系统模块
/// 
/// 提供签名验证、地址生成和密钥管理等功能
pub mod crypto;

/// 数据类型定义模块
pub mod types;

/// CoinJoin匿名交易模块
pub mod coinjoin;

/// P2P网络通信模块
pub mod p2p;

/// Tor网络集成模块
pub mod tor;

/// WebSocket接口模块
pub mod ws;