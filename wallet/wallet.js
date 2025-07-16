/**
 * HANCOIN（汉币）钱包 - 前端JavaScript实现
 * 
 * 功能包括：
 * - 密钥管理：生成、导入、导出密钥
 * - 账户管理：查询余额
 * - 交易功能：转账
 * - 水龙头功能：领取免费汉币
 * - 社交功能：群聊、私聊、发布动态
 * - 红包功能
 */

// 全局变量
const WALLET = {
  keyPair: null,        // 当前用户的密钥对
  publicKey: null,      // 当前用户的公钥（字符串形式）
  balance: 0,           // 当前余额
  transactions: [],     // 交易历史
  contacts: [],         // 联系人列表
  messages: {},         // 消息记录 {contactPublicKey: [messages]}
  posts: [],            // 朋友圈动态
  groups: {},           // 群组 {groupId: {name, members, messages}}
  ws: null,             // WebSocket连接
  apiBase: window.location.origin, // API基础URL
  
  // 新增请求缓存和队列相关变量
  requestCache: {},      // API请求缓存 {url: {data, timestamp}}
  pendingRequests: {},   // 正在进行的请求 {url: true}
  requestQueue: [],      // 请求队列
  lastRequestTime: 0,    // 上次请求时间
};

// 初始化函数
async function initWallet() {
  console.log("初始化汉币钱包...");
  
  // 显示加载中
  showLoading(true);
  
  // 检查本地存储的密钥
  loadKeyFromLocalStorage();
  
  // 初始化UI
  updateUI();
  
  // 连接WebSocket
  connectWebSocket();
  
  // 隐藏加载中
  showLoading(false);
  
  // 注册事件监听器
  registerEventListeners();
  
  console.log("钱包初始化完成");
}

// ==================== 密钥管理 ====================

// 生成新密钥对
async function generateNewKeyPair() {
  try {
    // 使用Web Crypto API生成Ed25519密钥对
    const keyPair = await window.crypto.subtle.generateKey(
      {
        name: "Ed25519",
        namedCurve: "Ed25519"
      },
      true,
      ["sign", "verify"]
    );
    
    WALLET.keyPair = keyPair;
    
    // 导出公钥
    const publicKeyBuffer = await window.crypto.subtle.exportKey("raw", keyPair.publicKey);
    WALLET.publicKey = bufferToHex(publicKeyBuffer);
    
    // 保存到本地存储
    saveKeyToLocalStorage();
    
    // 更新UI
    updateUI();
    
    // 显示成功消息
    showMessage("成功生成新密钥对", "success");
    
    // 获取账户信息
    await getAccountInfo();
    
    return true;
  } catch (error) {
    console.error("生成密钥对失败:", error);
    showMessage("生成密钥对失败: " + error.message, "error");
    return false;
  }
}

// 导出密钥（私钥）
async function exportPrivateKey() {
  if (!WALLET.keyPair) {
    showMessage("没有可导出的密钥", "error");
    return null;
  }
  
  try {
    const privateKeyBuffer = await window.crypto.subtle.exportKey("pkcs8", WALLET.keyPair.privateKey);
    const privateKeyHex = bufferToHex(privateKeyBuffer);
    
    // 显示导出的私钥
    document.getElementById("exportedKey").value = privateKeyHex;
    document.getElementById("exportKeyModal").style.display = "block";
    
    return privateKeyHex;
  } catch (error) {
    console.error("导出私钥失败:", error);
    showMessage("导出私钥失败: " + error.message, "error");
    return null;
  }
}

// 导入密钥（私钥）
async function importPrivateKey(privateKeyHex) {
  try {
    if (!privateKeyHex) {
      privateKeyHex = document.getElementById("importKeyInput").value.trim();
    }
    
    if (!privateKeyHex) {
      showMessage("请输入有效的私钥", "error");
      return false;
    }
    
    const privateKeyBuffer = hexToBuffer(privateKeyHex);
    
    // 导入私钥
    const privateKey = await window.crypto.subtle.importKey(
      "pkcs8",
      privateKeyBuffer,
      {
        name: "Ed25519",
        namedCurve: "Ed25519"
      },
      true,
      ["sign"]
    );
    
    // 从私钥派生公钥（实际应用中可能需要更复杂的处理）
    // 这里简化处理，假设私钥的前32字节是公钥
    const publicKeyBuffer = privateKeyBuffer.slice(0, 32);
    const publicKey = await window.crypto.subtle.importKey(
      "raw",
      publicKeyBuffer,
      {
        name: "Ed25519",
        namedCurve: "Ed25519"
      },
      true,
      ["verify"]
    );
    
    WALLET.keyPair = {
      privateKey: privateKey,
      publicKey: publicKey
    };
    
    WALLET.publicKey = bufferToHex(publicKeyBuffer);
    
    // 保存到本地存储
    saveKeyToLocalStorage();
    
    // 更新UI
    updateUI();
    
    // 关闭导入模态框
    document.getElementById("importKeyModal").style.display = "none";
    
    // 显示成功消息
    showMessage("成功导入密钥", "success");
    
    // 获取账户信息
    await getAccountInfo();
    
    return true;
  } catch (error) {
    console.error("导入私钥失败:", error);
    showMessage("导入私钥失败: " + error.message, "error");
    return false;
  }
}

// 从本地存储加载密钥
function loadKeyFromLocalStorage() {
  const savedKeyPair = localStorage.getItem("hancoinKeyPair");
  
  if (savedKeyPair) {
    try {
      const keyData = JSON.parse(savedKeyPair);
      
      // 这里简化处理，实际应用中需要更安全的方式存储和恢复密钥
      importPrivateKey(keyData.privateKey);
      
      return true;
    } catch (error) {
      console.error("从本地存储加载密钥失败:", error);
      return false;
    }
  }
  
  return false;
}

// 保存密钥到本地存储
async function saveKeyToLocalStorage() {
  if (!WALLET.keyPair) {
    return false;
  }
  
  try {
    const privateKeyBuffer = await window.crypto.subtle.exportKey("pkcs8", WALLET.keyPair.privateKey);
    const privateKeyHex = bufferToHex(privateKeyBuffer);
    
    const keyData = {
      privateKey: privateKeyHex,
      publicKey: WALLET.publicKey
    };
    
    localStorage.setItem("hancoinKeyPair", JSON.stringify(keyData));
    
    return true;
  } catch (error) {
    console.error("保存密钥到本地存储失败:", error);
    return false;
  }
}

// ==================== 账户管理 ====================

async function withExponentialBackoff(fn, maxRetries = 3, baseDelay = 1000) {
    let retryCount = 0;
    while (retryCount < maxRetries) {
        try {
            return await fn();
        } catch (error) {
            retryCount++;
            if (retryCount >= maxRetries) {
                throw error;
            }
            const delay = baseDelay * Math.pow(2, retryCount);
            console.warn(`操作失败，${delay}ms 后重试...`);
            await new Promise(resolve => setTimeout(resolve, delay));
        }
    }
}

// 获取账户信息（余额等）
async function getAccountInfo() {
    if (!WALLET.publicKey) {
        showMessage("请先创建或导入密钥", "error");
        return null;
    }

    const cacheKey = `account_${WALLET.publicKey}`;
    const cacheExpiry = 30 * 1000; // 30秒缓存

    // 检查缓存
    if (WALLET.requestCache[cacheKey] && 
        Date.now() - WALLET.requestCache[cacheKey].timestamp < cacheExpiry) {
        const cachedData = WALLET.requestCache[cacheKey].data;
        WALLET.balance = cachedData.balance || 0;
        WALLET.transactions = cachedData.transactions || [];
        updateBalanceDisplay();
        updateTransactionHistory();
        return cachedData;
    }

    // 检查是否已有相同请求正在进行
    if (WALLET.pendingRequests[cacheKey]) {
        return new Promise(resolve => {
            const checkPending = () => {
                if (!WALLET.pendingRequests[cacheKey] && WALLET.requestCache[cacheKey]) {
                    const cachedData = WALLET.requestCache[cacheKey].data;
                    WALLET.balance = cachedData.balance || 0;
                    WALLET.transactions = cachedData.transactions || [];
                    updateBalanceDisplay();
                    updateTransactionHistory();
                    resolve(cachedData);
                } else {
                    setTimeout(checkPending, 100);
                }
            };
            checkPending();
        });
    }

    // 标记请求为进行中
    WALLET.pendingRequests[cacheKey] = true;

    try {
        const accountData = await withExponentialBackoff(async () => {
            const response = await fetch(`${WALLET.apiBase}/api/account/${WALLET.publicKey}`);
            if (!response.ok) {
                throw new Error(`HTTP错误 ${response.status}`);
            }
            return await response.json();
        });

        // 更新缓存
        WALLET.requestCache[cacheKey] = {
            data: accountData,
            timestamp: Date.now()
        };

        // 更新钱包数据
        WALLET.balance = accountData.balance || 0;
        WALLET.transactions = accountData.transactions || [];

        // 更新UI
        updateBalanceDisplay();
        updateTransactionHistory();

        return accountData;
    } catch (error) {
        console.error("获取账户信息失败:", error);
        showMessage("获取账户信息失败: " + error.message, "warning");

        // 如果是新账户，可能还没有在区块链上注册
        WALLET.balance = 0;
        WALLET.transactions = [];

        // 更新UI
        updateBalanceDisplay();
        updateTransactionHistory();

        return null;
    } finally {
        // 清除进行中标记
        delete WALLET.pendingRequests[cacheKey];
    }
}

// ==================== 交易功能 ====================

// 发送交易（转账）

async function sendTransaction(transaction, amount, recipientPublicKey) {
    const requestId = Date.now().toString();
    const url = `${WALLET.apiBase}/api/transaction`;
    
    // 检查缓存
    if (WALLET.requestCache[url] && Date.now() - WALLET.requestCache[url].timestamp < 5000) {
        return true;
    }
  if (!WALLET.keyPair || !WALLET.publicKey) {
    showMessage("请先创建或导入密钥", "error");
    return false;
  }
  
  const recipientPublicKey = document.getElementById("recipientAddress").value.trim();
  const amountStr = document.getElementById("sendAmount").value.trim();
  const amount = parseFloat(amountStr);
  
  if (!recipientPublicKey) {
    showMessage("请输入接收方地址", "error");
    return false;
  }
  
  if (isNaN(amount) || amount <= 0) {
    showMessage("请输入有效的金额", "error");
    return false;
  }
  
  if (amount > WALLET.balance) {
    showMessage("余额不足", "error");
    return false;
  }
  
  // 创建交易对象
  const transaction = {
    sender: WALLET.publicKey,
    recipient: recipientPublicKey,
    amount: amount,
    timestamp: Date.now()
  };
  
  // 签名交易
  const signature = await signMessage(JSON.stringify(transaction));
  
  // 指数退避重试逻辑
  let retryCount = 0;
  const maxRetries = 3;
  const baseDelay = 1000;
  
  while (retryCount < maxRetries) {
    try {
      // 添加到请求队列
      const requestId = `${WALLET.publicKey}_${Date.now()}`;
      WALLET.requestQueue.push(requestId);
      
      // 发送到服务器
      const response = await fetch(`${WALLET.apiBase}/api/transaction`, {
        method: "POST",
        headers: {
          "Content-Type": "application/json"
        },
        body: JSON.stringify({
          transaction: transaction,
          signature: signature
        })
      });
      
      // 从队列中移除
      WALLET.requestQueue = WALLET.requestQueue.filter(id => id !== requestId);
      
      if (!response.ok) {
        const errorData = await response.json();
        throw new Error(errorData.message || `HTTP错误 ${response.status}`);
      }
      
      const result = await response.json();
      
      // 更新余额和交易历史
      await getAccountInfo();
      
      // 显示成功消息
      showMessage(`成功发送 ${amount} 汉币到 ${recipientPublicKey.substring(0, 8)}...`, "success");
      
      // 清空输入框
      document.getElementById("recipientAddress").value = "";
      document.getElementById("sendAmount").value = "";
      
      // 关闭发送模态框
      document.getElementById("sendModal").style.display = "none";
      
      return true;
      
    } catch (error) {
      // 从队列中移除
      WALLET.requestQueue = WALLET.requestQueue.filter(id => id !== requestId);
      
      retryCount++;
      if (retryCount >= maxRetries) {
        console.error("发送交易失败:", error);
        showMessage("发送交易失败: " + error.message, "error");
        return false;
      }
      
      const delay = baseDelay * Math.pow(2, retryCount);
      console.warn(`发送交易失败，${delay}ms后重试...`);
      await new Promise(resolve => setTimeout(resolve, delay));
    }
  }
}

// ==================== 水龙头功能 ====================

// 从水龙头领取免费汉币
async function claimFromFaucet() {
  if (!WALLET.keyPair || !WALLET.publicKey) {
    showMessage("请先创建或导入密钥", "error");
    return false;
  }
  
  try {
    // 创建请求对象
    const request = {
      publicKey: WALLET.publicKey,
      timestamp: Date.now()
    };
    
    // 签名请求
    const signature = await signMessage(JSON.stringify(request));
    
    // 发送到服务器
    const response = await fetch(`${WALLET.apiBase}/api/faucet`, {
      method: "POST",
      headers: {
        "Content-Type": "application/json"
      },
      body: JSON.stringify({
        request: request,
        signature: signature
      })
    });
    
    if (!response.ok) {
      const errorData = await response.json();
      throw new Error(errorData.message || `HTTP错误 ${response.status}`);
    }
    
    const result = await response.json();
    
    // 更新余额和交易历史
    await getAccountInfo();
    
    // 显示成功消息
    showMessage(`成功领取 ${result.amount} 汉币`, "success");
    
    return true;
  } catch (error) {
    console.error("从水龙头领取失败:", error);
    showMessage("从水龙头领取失败: " + error.message, "error");
    return false;
  }
}

// ==================== 社交功能 ====================

// 连接WebSocket
function connectWebSocket() {
  if (!WALLET.publicKey) {
    console.log("未找到公钥，无法连接WebSocket");
    return;
  }
  
  // 关闭现有连接
  if (WALLET.ws) {
    WALLET.ws.close();
  }
  
  // 创建新连接
  const wsProtocol = window.location.protocol === "https:" ? "wss:" : "ws:";
  const wsUrl = `${wsProtocol}//${window.location.host}/ws/${WALLET.publicKey}`;
  
  WALLET.ws = new WebSocket(wsUrl);
  
  // 心跳相关变量
  let heartbeatInterval;
  let missedHeartbeats = 0;
  const heartbeatTimeout = 30000; // 30秒无响应视为断开
  const heartbeatIntervalTime = 15000; // 15秒发送一次心跳
  
  WALLET.ws.onopen = function(event) {
    console.log("WebSocket连接已建立");
    showMessage("已连接到服务器", "success");
    
    // 启动心跳
    heartbeatInterval = setInterval(() => {
      if (WALLET.ws.readyState === WebSocket.OPEN) {
        WALLET.ws.send(JSON.stringify({type: 'ping'}));
        missedHeartbeats++;
        
        if (missedHeartbeats > 2) {
          console.warn('心跳无响应，强制重新连接');
          clearInterval(heartbeatInterval);
          connectWebSocket();
        }
      }
    }, heartbeatIntervalTime);
  };
  
  WALLET.ws.onmessage = function(event) {
    try {
      const message = JSON.parse(event.data);
      
      // 处理心跳响应
      if (message.type === 'pong') {
        missedHeartbeats = 0;
        return;
      }
      
      handleWebSocketMessage(message);
    } catch (error) {
      console.error("处理WebSocket消息失败:", error);
    }
  };
  
  WALLET.ws.onclose = function(event) {
    console.log("WebSocket连接已关闭");
    
    // 清除心跳
    if (heartbeatInterval) {
      clearInterval(heartbeatInterval);
    }
    
    // 指数退避重连
    const reconnectDelay = Math.min(5000 * (1 + Math.random()), 30000); // 5-30秒随机延迟
    setTimeout(connectWebSocket, reconnectDelay);
  };
  
  WALLET.ws.onerror = function(error) {
    console.error("WebSocket错误:", error);
    showMessage("WebSocket连接错误", "error");
    
    // 清除心跳
    if (heartbeatInterval) {
      clearInterval(heartbeatInterval);
    }
    
    // 立即尝试重新连接
    setTimeout(connectWebSocket, 1000);
  };
}

// 处理WebSocket消息
function handleWebSocketMessage(message) {
  // 使用requestAnimationFrame来批量处理消息，避免UI频繁更新
  if (!WALLET.messageQueue) {
    WALLET.messageQueue = [];
    requestAnimationFrame(processMessageQueue);
  }
  
  // 将消息加入队列
  WALLET.messageQueue.push(message);
  
  // 限制队列大小，防止内存泄漏
  if (WALLET.messageQueue.length > 100) {
    console.warn('消息队列过大，丢弃旧消息');
    WALLET.messageQueue = WALLET.messageQueue.slice(-50);
  }
}

// 处理消息队列
function processMessageQueue() {
  const startTime = performance.now();
  
  // 处理队列中的消息，但限制处理时间以避免阻塞主线程
  while (WALLET.messageQueue.length > 0 && performance.now() - startTime < 16) {
    const message = WALLET.messageQueue.shift();
    
    try {
      console.log("处理WebSocket消息:", message);
      
      switch (message.type) {
        case "private_message":
          handlePrivateMessage(message);
          break;
        case "group_message":
          handleGroupMessage(message);
          break;
        case "post":
          handleNewPost(message);
          break;
        case "transaction":
          handleTransactionUpdate(message);
          break;
        case "red_packet":
          handleRedPacket(message);
          break;
        default:
          console.log("未知消息类型:", message.type);
      }
    } catch (error) {
      console.error("处理消息失败:", error, message);
    }
  }
  
  // 如果队列不为空，继续处理
  if (WALLET.messageQueue.length > 0) {
    requestAnimationFrame(processMessageQueue);
  } else {
    // 所有消息处理完成后更新UI
    updateUI();
  }
}

// 处理私聊消息
function handlePrivateMessage(message) {
  const sender = message.sender;
  
  // 初始化发送者的消息数组（如果不存在）
  if (!WALLET.messages[sender]) {
    WALLET.messages[sender] = [];
  }
  
  // 添加消息
  WALLET.messages[sender].push({
    sender: sender,
    content: message.content,
    timestamp: message.timestamp,
    isRead: false,
    isSelf: false
  });
  
  // 如果当前正在查看该联系人的消息，则标记为已读
  const currentContact = document.getElementById("currentContact").dataset.publicKey;
  if (currentContact === sender) {
    markMessagesAsRead(sender);
  } else {
    // 否则显示通知
    showNotification(`来自 ${shortenPublicKey(sender)} 的新消息`, message.content);
  }
  
  // 更新联系人列表
  updateContactsList();
}

// 处理群聊消息
function handleGroupMessage(message) {
  const groupId = message.groupId;
  
  // 初始化群组（如果不存在）
  if (!WALLET.groups[groupId]) {
    WALLET.groups[groupId] = {
      id: groupId,
      name: message.groupName || `群组 ${groupId.substring(0, 8)}`,
      members: message.members || [],
      messages: []
    };
  }
  
  // 添加消息
  WALLET.groups[groupId].messages.push({
    sender: message.sender,
    content: message.content,
    timestamp: message.timestamp,
    isRead: false,
    isSelf: message.sender === WALLET.publicKey
  });
  
  // 如果当前正在查看该群组的消息，则标记为已读
  const currentGroup = document.getElementById("currentGroup").dataset.groupId;
  if (currentGroup === groupId) {
    markGroupMessagesAsRead(groupId);
  } else {
    // 否则显示通知
    showNotification(`来自群组 ${WALLET.groups[groupId].name} 的新消息`, message.content);
  }
  
  // 更新群组列表
  updateGroupsList();
}

// 处理新动态
function handleNewPost(message) {
  // 添加到动态列表
  WALLET.posts.unshift({
    id: message.id,
    author: message.author,
    content: message.content,
    timestamp: message.timestamp,
    likes: message.likes || 0,
    comments: message.comments || []
  });
  
  // 更新朋友圈UI
  updatePostsDisplay();
  
  // 显示通知
  if (message.author !== WALLET.publicKey) {
    showNotification(`${shortenPublicKey(message.author)} 发布了新动态`, message.content);
  }
}

// 处理交易更新
function handleTransactionUpdate(message) {
  // 更新余额
  if (message.balance !== undefined) {
    WALLET.balance = message.balance;
    updateBalanceDisplay();
  }
  
  // 更新交易历史
  if (message.transaction) {
    WALLET.transactions.unshift(message.transaction);
    updateTransactionHistory();
  }
  
  // 显示通知
  if (message.transaction && message.transaction.recipient === WALLET.publicKey) {
    showNotification(
      "收到新交易",
      `从 ${shortenPublicKey(message.transaction.sender)} 收到 ${message.transaction.amount} 汉币`
    );
  }
}

// 处理红包
function handleRedPacket(message) {
  // 显示红包通知
  showRedPacketNotification(message);
}