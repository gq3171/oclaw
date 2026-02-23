# OpenClaw (oclaw)

一个用 Rust 编写的模块化 AI 智能体网关框架。提供 WebSocket/HTTP 网关服务、多供应商 LLM 集成、多渠道消息通信、插件/技能扩展和终端 UI。

## 特性

- **网关** — HTTP + WebSocket 服务器，支持 TLS、速率限制、安全头、优雅关闭
- **多模型** — Anthropic、OpenAI、AWS Bedrock、OpenRouter、Together AI，支持模型降级链
- **渠道** — Telegram、Slack、Discord、Matrix、Signal、LINE、Mattermost、Google Chat、飞书、MS Teams
- **智能体** — 子智能体编排、工具执行、循环检测
- **存储** — SQLite、PostgreSQL、向量搜索 (LanceDB)、全文搜索和混合搜索
- **安全** — OAuth 2.0 (Google/Discord/GitHub/Slack)、CSRF 防护、HMAC 验证、TLS 证书校验
- **扩展** — 插件系统、技能注册与模式匹配、沙箱执行
- **可观测性** — Prometheus 指标、结构化 JSON 日志、健康检查、系统诊断
- **界面** — 终端 UI (ratatui)、Web 聊天、控制面板

## 快速开始

### 前置要求

- Rust 1.85+（edition 2024）
- SQLite（内置）或 PostgreSQL（可选）

### 构建与运行

```bash
cargo build --workspace
cargo run -p oclaw-cli -- start --port 8080
```

### 配置

```bash
# 初始化配置
cargo run -p oclaw-cli -- config init

# 交互式配置向导
cargo run -p oclaw-cli -- wizard

# 设置渠道 / 供应商 / 技能
cargo run -p oclaw-cli -- channel setup
cargo run -p oclaw-cli -- provider setup
cargo run -p oclaw-cli -- skill setup
```

配置文件位置：`~/.oclaws/config.json`（Linux/macOS）或 `%APPDATA%\oclaws\config.json`（Windows）。

完整配置参考请查看 [`config.example.json`](config.example.json)。

## CLI 命令

| 命令 | 说明 |
|------|------|
| `start` | 启动网关服务器（`--port`、`--host`、`--http-only`、`--ws-only`） |
| `config init\|show\|validate` | 管理配置 |
| `wizard` | 交互式配置向导 |
| `channel setup\|list` | 管理消息渠道 |
| `skill setup\|list` | 管理技能 |
| `provider setup\|status` | 管理 LLM 供应商 |
| `agent -m "消息"` | 向智能体发送消息 |
| `sessions list\|show\|delete` | 管理会话 |
| `models list` | 列出可用模型 |
| `doctor` | 运行系统诊断 |
| `daemon start\|stop\|status` | 后台服务管理 |
| `tui` | 启动终端 UI |
| `status` | 查看网关状态 |

### 全局参数

```
--log-level <LEVEL>    日志级别：trace, debug, info, warn, error（默认：info）
--log-format <FORMAT>  日志格式：text, json（默认：text）
--config <PATH>        配置文件路径
--gateway-url <URL>    网关地址（默认：http://127.0.0.1:8081）
```

## 架构

```
crates/
├── cli/            # CLI 二进制文件 (oclaws)
├── protocol/       # 基于帧的通信协议
├── gateway-core/   # HTTP + WebSocket 服务器、中间件、Webhook
├── agent-core/     # 智能体编排、子智能体、模型降级
├── llm-core/       # 多供应商 LLM 集成
├── channel-core/   # 消息渠道适配器
├── tools-core/     # 工具执行框架
├── storage-core/   # 数据库抽象层 (SQLite/PG/向量)
├── config/         # 配置管理
├── plugin-core/    # 插件系统
├── skills-core/    # 技能注册
├── security-core/  # OAuth、加密、审计
├── sandbox-core/   # 沙箱执行
├── doctor-core/    # 健康检查与诊断
├── voice-core/     # 音频流 (STT/TTS)
├── media-core/     # 图像/音频处理
├── browser-core/   # 浏览器自动化
├── tui-core/       # 终端 UI
└── daemon-core/    # 后台服务
```

## 开发

```bash
cargo test --workspace --all-features    # 运行所有测试
cargo test -p oclaws-security-core       # 测试单个 crate
cargo clippy --workspace --all-features -- -D warnings
cargo fmt --all -- --check
```

## API 端点

| 端点 | 方法 | 说明 |
|------|------|------|
| `/health` | GET | 存活检查 |
| `/ready` | GET | 就绪检查（含组件健康状态） |
| `/ws` | GET | WebSocket 连接 |
| `/v1/chat/completions` | POST | OpenAI 兼容聊天 API |
| `/v1/responses` | POST | 响应 API |
| `/agent/status` | GET | 智能体状态 |
| `/sessions` | GET | 列出会话 |
| `/config` | GET | 获取配置 |
| `/config/reload` | POST | 重新加载配置 |
| `/models` | GET | 列出可用模型 |
| `/metrics` | GET | Prometheus 指标 |
| `/webhooks/{channel}` | POST | 渠道 Webhook |

## 许可证

MIT
