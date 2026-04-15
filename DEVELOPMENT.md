# 开发文档

## 项目概述

`ssh-t` 是一个用 Rust 编写的 TUI（终端用户界面）SSH 终端客户端。它提供交互式终端界面，支持 SSH 连接和 SFTP 文件浏览功能。

## 技术栈

| 组件 | 技术 | 用途 |
|------|------|------|
| TUI 框架 | Ratatui 0.29 | 终端界面渲染 |
| 终端控制 | Crossterm 0.28 | 跨平台终端处理 |
| SSH 协议 | russh 0.49 | SSH 客户端实现 |
| SFTP | russh-sftp 2 | SFTP 文件操作 |
| 异步运行时 | Tokio 1 | 异步 I/O |
| 凭证存储 | keyring 3 | 操作系统原生密码存储 |
| 序列化 | serde + toml | 配置文件解析 |

## 架构设计

### 模块结构

```
src/
├── main.rs              # 程序入口
│   ├── 终端初始化/清理
│   ├── 事件循环
│   └── SSH 清理
│
├── app/mod.rs           # 核心应用状态
│   ├── Panel 枚举（HostList, Terminal, Sftp, Help）
│   ├── Dialog 枚举（None, PasswordInput, Connecting）
│   ├── App 结构体及所有状态
│   ├── 键盘/鼠标事件处理
│   └── SSH 事件轮询
│
├── config/mod.rs        # 配置管理
│   ├── HostConfig 结构体
│   ├── AuthMethod 枚举
│   └── AppConfig 加载/保存
│
├── cred/mod.rs          # 凭证管理
│   └── CredentialStore（keyring 封装）
│
├── ssh/mod.rs           # SSH 连接
│   ├── SshEvent 枚举
│   ├── ShellInput 枚举
│   ├── SshManager 结构体
│   └── PTY 处理
│
├── sftp/mod.rs          # SFTP 操作
│   ├── FileEntry 结构体
│   ├── TransferEvent 枚举
│   ├── TransferState 结构体
│   └── SftpEngine 结构体
│
├── terminal/mod.rs      # ANSI 解析（待实现）
│   └── AnsiParser 结构体
│
└── tui/mod.rs           # UI 渲染
    ├── draw() 主函数
    ├── 对话框渲染
    └── 面板渲染
```

### 数据流

```
用户输入（键盘/鼠标）
         │
         ▼
    ┌─────────┐
    │ main.rs │ ◄─── 事件循环（100ms 轮询）
    └────┬────┘
         │
         ▼
    ┌─────────┐
    │   App   │ ◄─── handle_key() / handle_mouse()
    └────┬────┘
         │
    ┌────┴────┬────────────┐
    ▼         ▼            ▼
┌───────┐ ┌───────┐   ┌───────┐
│ SSH   │ │ SFTP  │   │  TUI  │
│Manager│ │Engine │   │ Render│
└───┬───┘ └───┬───┘   └───────┘
    │         │
    ▼         ▼
┌───────────────────┐
│   异步任务        │
│ (Tokio 运行时)    │
└───────────────────┘
         │
         ▼
    SSH 服务器
```

### 事件系统

应用使用 MPSC 通道进行异步通信：

```rust
// SSH 事件：从异步任务发送到主线程
pub enum SshEvent {
    Connected(String),      // 连接成功
    Output(Vec<u8>),        // 终端输出
    Disconnected(String),   // 断开连接
    Error(String),          // 错误
    SftpReady,              // SFTP 就绪
}

// SFTP 传输进度
pub enum TransferEvent {
    Started { file, total, is_upload },   // 传输开始
    Progress { file, transferred, total }, // 传输进度
    Completed { file },                    // 传输完成
    Error { file, error },                 // 传输错误
}
```

## 关键实现细节

### SSH 连接流程

1. 用户选择主机并按 Enter
2. `initiate_connection()` 检查存储的凭证
3. 如果没有存储密码，显示 `PasswordInput` 对话框
4. `start_connection()` 启动异步任务：
   - 创建 `SshManager`
   - 调用 `connect()` 进行认证
   - 调用 `open_shell()` 打开 PTY
   - 通过 oneshot 通道发送 manager
5. 主线程通过 `poll_ssh_events()` 接收 manager
6. 终端输出通过 `SshEvent::Output` 流式传输

### SFTP 架构

SFTP 使用独立的 SSH 连接，避免会话所有权问题：

```rust
fn list_sftp_dir(&mut self, path: String) {
    // 为 SFTP 创建新的 SSH 连接
    let host_config = self.connected_host_config.clone();
    tokio::spawn(async move {
        let mut manager = SshManager::new(host_config, event_tx);
        manager.connect().await?;
        let stream = manager.open_sftp_stream().await?;
        let mut engine = SftpEngine::new(tx);
        engine.init(stream).await?;
        let entries = engine.list_dir(&path).await?;
        // 将结果发送回主线程
    });
}
```

### 终端 PTY 处理

```rust
// 打开 PTY shell
pub async fn open_shell(&mut self, cols: u16, rows: u16) -> Result<()> {
    let channel = session.channel_open_session().await?;
    channel.request_pty(true, "xterm-256color", cols, rows, 0, 0, &[]).await?;
    channel.request_shell(true).await?;
    
    // 启动 I/O 处理任务
    tokio::spawn(async move {
        loop {
            tokio::select! {
                input = input_rx.recv() => { /* 发送到 channel */ }
                msg = channel.wait() => { /* 转发到 event_tx */ }
            }
        }
    });
}
```

## 开发环境配置

### 前置要求

- Rust 1.70+
- Cargo

### 构建

```bash
# 调试构建
cargo build

# 发布构建
cargo build --release

# 运行
cargo run
```

### 测试

```bash
cargo test
```

### 代码检查

```bash
cargo clippy
cargo fmt --check
```

## 配置文件格式

位置：`~/.config/ssh-t/config.toml`

```toml
[[hosts]]
name = "显示名称"
host = "主机名或IP"
port = 22
user = "用户名"
auth = "Password"  # 或 "Agent"
group = "可选分组名"

[[hosts]]
name = "密钥认证服务器"
host = "example.com"
port = 22
user = "admin"
auth = { Key = { key_path = "/home/user/.ssh/id_ed25519" } }
```

## 安全考虑

1. **密码存储**：通过 `keyring` 库使用操作系统密钥链
   - macOS：Keychain
   - Linux：Secret Service
   - Windows：Credential Manager

2. **SSH 密钥处理**：密钥在运行时加载，不常驻内存

3. **服务器密钥验证**：当前接受所有密钥（待实现 known_hosts）

## 已知限制

1. SFTP 使用独立 SSH 连接（不与终端会话共享）
2. 没有 known_hosts 验证
3. 没有文件上传 UI（后端已实现）
4. 终端滚动缓冲区限制为 100KB

## 未来改进

- [ ] Known hosts 管理
- [ ] 文件上传对话框
- [ ] 端口转发
- [ ] 多会话支持
- [ ] 终端搜索
- [ ] TUI 内编辑配置文件

## 贡献指南

1. Fork 仓库
2. 创建功能分支
3. 进行修改
4. 运行 `cargo fmt` 和 `cargo clippy`
5. 提交 Pull Request

## 调试

启用调试输出：

```bash
RUST_LOG=debug cargo run
```

SSH 协议调试：

```bash
RUST_LOG=russh=trace cargo run
```
