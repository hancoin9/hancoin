//! Tor网络支持模块
//! 
//! 该模块提供了通过Tor网络进行匿名通信的功能，包括：
//! - Tor配置
//! - Tor连接器
//! - .onion地址支持

use std::io;
use std::net::SocketAddr;
use std::str::FromStr;
use async_trait::async_trait;
use tokio::net::TcpStream;
use tokio_socks::tcp::Socks5Stream;
use log::{debug, error, info};

/// Tor配置
#[derive(Clone, Debug)]
pub struct TorConfig {
    /// 是否启用Tor
    pub enabled: bool,
    /// Tor SOCKS5代理地址
    pub proxy_addr: String,
    /// 是否只允许.onion地址
    pub only_onion: bool,
}

impl Default for TorConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            proxy_addr: "127.0.0.1:9050".to_string(),
            only_onion: false,
        }
    }
}

/// Tor连接器
/// 
/// 用于通过Tor网络建立TCP连接
#[derive(Clone)]
pub struct TorConnector {
    config: TorConfig,
}

impl TorConnector {
    /// 创建新的Tor连接器
    pub fn new(config: TorConfig) -> Self {
        Self { config }
    }
    
    /// 检查是否启用Tor
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }
    
    /// 检查地址是否为.onion地址
    pub fn is_onion_address(addr: &str) -> bool {
        addr.contains(".onion:")
    }
    
    /// 通过Tor网络连接到目标地址
    pub async fn connect(&self, addr: &str) -> io::Result<TcpStream> {
        debug!("通过Tor连接到: {}", addr);
        
        // 解析代理地址
        let proxy_addr = match SocketAddr::from_str(&self.config.proxy_addr) {
            Ok(addr) => addr,
            Err(e) => {
                error!("无效的Tor代理地址: {}", e);
                return Err(io::Error::new(io::ErrorKind::InvalidInput, "无效的Tor代理地址"));
            }
        };
        
        // 解析目标地址
        let (host, port) = match addr.rsplit_once(':') {
            Some((host, port)) => {
                let port = match port.parse::<u16>() {
                    Ok(p) => p,
                    Err(e) => {
                        error!("无效的端口号: {}", e);
                        return Err(io::Error::new(io::ErrorKind::InvalidInput, "无效的端口号"));
                    }
                };
                (host.to_string(), port)
            },
            None => {
                error!("无效的地址格式: {}", addr);
                return Err(io::Error::new(io::ErrorKind::InvalidInput, "无效的地址格式"));
            }
        };
        
        // 通过SOCKS5代理连接
        match Socks5Stream::connect(proxy_addr, (host, port)).await {
            Ok(stream) => {
                debug!("成功通过Tor连接到: {}", addr);
                Ok(stream.into_inner())
            },
            Err(e) => {
                error!("通过Tor连接失败: {}", e);
                Err(io::Error::new(io::ErrorKind::ConnectionRefused, format!("通过Tor连接失败: {}", e)))
            }
        }
    }
}

/// Tor网络接口
/// 
/// 定义了Tor网络的基本操作
#[async_trait]
pub trait TorNetwork {
    /// 获取.onion地址
    async fn get_onion_address(&self) -> io::Result<String>;
    
    /// 发布服务到Tor网络
    async fn publish_service(&self, port: u16) -> io::Result<String>;
    
    /// 连接到Tor网络
    async fn connect(&self) -> io::Result<()>;
    
    /// 断开与Tor网络的连接
    async fn disconnect(&self) -> io::Result<()>;
}

/// Tor网络状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TorNetworkStatus {
    /// 已连接
    Connected,
    /// 已断开
    Disconnected,
    /// 正在连接
    Connecting,
    /// 连接错误
    Error,
}