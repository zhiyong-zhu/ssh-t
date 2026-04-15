# ssh-t

一个基于终端的 SSH 客户端，支持 SFTP 文件传输，使用 Rust 开发。

![Rust](https://img.shields.io/badge/rust-1.70%2B-orange)
![License](https://img.shields.io/badge/license-MIT-blue)

## 功能特性

- **终端界面**：基于 Ratatui 构建的简洁键盘驱动界面
- **SSH 终端**：完整的 PTY 支持，键盘输入直接转发
- **SFTP 浏览器**：浏览远程文件，下载文件到本地
- **多种认证方式**：密码认证、SSH 密钥、SSH Agent
- **凭证存储**：使用操作系统密钥链安全存储密码
- **跨平台**：支持 Windows、macOS、Linux
- **鼠标支持**：点击选择，滚轮导航

## 安装

### 从源码构建

```bash
git clone https://github.com/yourname/ssh-t.git
cd ssh-t
cargo build --release
```

编译后的二进制文件位于 `target/release/ssh-t`。

## 使用方法

```
ssh-t
```

### 键盘快捷键

#### 全局操作
| 按键 | 功能 |
|-----|------|
| `1` | 切换到主机列表 |
| `2` | 切换到终端 |
| `3` | 切换到 SFTP |
| `?` | 显示帮助 |
| `q` | 退出（在主机列表界面） |

#### 主机列表
| 按键 | 功能 |
|-----|------|
| `↑/↓` | 导航主机 |
| `Enter` | 连接到选中的主机 |
| `Esc` | 清除筛选 |
| `输入字符` | 按名称/主机/用户筛选 |

#### 终端（PTY 模式）
| 按键 | 功能 |
|-----|------|
| 所有按键 | 直接发送到远程 shell |
| `Ctrl+Q` | 断开连接并返回主机列表 |

#### SFTP
| 按键 | 功能 |
|-----|------|
| `↑/↓` 或 `j/k` | 导航文件列表 |
| `Enter` | 进入目录 |
| `Backspace` | 返回上级目录 |
| `r` | 刷新当前目录 |
| `d` | 下载选中的文件 |
| `u` | 上传文件（占位符） |

#### 鼠标操作
| 操作 | 行为 |
|------|------|
| 左键点击 | 选择项目 |
| 滚轮滚动 | 导航列表 |

## 配置

配置文件位于 `~/.config/ssh-t/config.toml`：

```toml
[[hosts]]
name = "我的服务器"
host = "192.168.1.100"
port = 22
user = "admin"
auth = "Password"
group = "work"

[[hosts]]
name = "github"
host = "github.com"
port = 22
user = "git"
auth = { Key = { key_path = "/home/user/.ssh/id_ed25519" } }

[[hosts]]
name = "agent服务器"
host = "10.0.0.1"
port = 22
user = "deploy"
auth = "Agent"
```

### 认证方式

- `Password`：连接时提示输入密码，存储在系统密钥链中
- `{ Key = { key_path = "/path/to/key" } }`：使用 SSH 私钥文件
- `Agent`：使用 SSH Agent（ssh-agent）

## 项目结构

```
src/
├── main.rs         # 程序入口，事件循环
├── app/
│   └── mod.rs      # 应用状态和逻辑
├── config/
│   └── mod.rs      # 配置管理
├── cred/
│   └── mod.rs      # 凭证存储（密钥链）
├── ssh/
│   └── mod.rs      # SSH 连接管理
├── sftp/
│   └── mod.rs      # SFTP 文件操作
├── terminal/
│   └── mod.rs      # ANSI 解析器（待实现）
└── tui/
    └── mod.rs      # TUI 渲染
```

## 依赖库

- [ratatui](https://github.com/ratatui-org/ratatui) - 终端 UI 框架
- [crossterm](https://github.com/crossterm-rs/crossterm) - 跨平台终端控制
- [russh](https://github.com/warp-tech/russh) - SSH 协议实现
- [russh-sftp](https://github.com/warp-tech/russh) - SFTP 协议
- [tokio](https://github.com/tokio-rs/tokio) - 异步运行时
- [keyring](https://github.com/hwchen/keyring-rs) - 系统凭证存储

## 许可证

MIT License
