# OpenClaw (oclaw)

一个用 Rust 编写的模块化 AI 智能体网关框架。提供 WebSocket/HTTP 网关服务、多供应商 LLM 集成、多渠道消息通信、插件/技能扩展、定时任务、记忆系统和终端 UI。

## 特性

- **网关** — HTTP + WebSocket 服务器，支持 TLS、速率限制、安全头、优雅关闭
- **多模型** — Anthropic、OpenAI、Google、Cohere、Ollama、AWS Bedrock、OpenRouter、Together AI、MiniMax，支持模型降级链
- **渠道** — Telegram、Slack、Discord、Matrix、Signal、LINE、Mattermost、Google Chat、飞书、MS Teams
- **智能体** — 子智能体编排、工具执行、循环检测、回声检测、上下文压缩、自动记忆召回
- **存储** — SQLite、PostgreSQL、向量搜索 (LanceDB)、全文搜索、混合搜索、时间衰减排序
- **记忆** — 长期记忆管理、嵌入向量搜索、文件监控自动索引
- **安全** — OAuth 2.0 (Google/Discord/GitHub/Slack)、CSRF 防护、HMAC 验证、TLS 证书校验
- **扩展** — 插件系统（含 HTTP 路由注册）、技能注册与模式匹配、沙箱执行
- **定时任务** — Cron 调度、并发控制、持久化存储、Webhook 触发
- **可观测性** — Prometheus 指标、结构化 JSON 日志、健康检查、系统诊断
- **界面** — Web 聊天 UI、Web 配置管理 UI、终端 UI (ratatui)

## 快速开始

### 前置要求

- Rust 1.85+（edition 2024）
- SQLite（内置）或 PostgreSQL（可选）

### 构建与运行

```bash
cargo build --workspace
cargo run -p oclaw-cli -- start --port 8080
```

启动后可访问：
- Web 聊天界面：`http://127.0.0.1:8081/ui/chat`
- Web 配置管理：`http://127.0.0.1:8081/ui/config`
- WebSocket 网关：`ws://127.0.0.1:8080/ws`

> HTTP 端口 = WebSocket 端口 + 1（默认 WS 8080，HTTP 8081）

### 环境变量

支持 `.env` 文件自动加载（通过 dotenvy），也可在 `config.json` 中用 `${VAR_NAME}` 引用环境变量。

```bash
cp .env.example .env
# 编辑 .env 填入你的 API Key 和 Token
```

`.env.example` 示例：

```env
OCLAWS_PROVIDER_MINIMAX_API_KEY=your-api-key
OCLAWS_TELEGRAM_BOT_TOKEN=123456789:ABCdefGHIjklMNOpqrsTUVwxyz
OCLAWS_FEISHU_APP_ID=your-feishu-app-id
OCLAWS_FEISHU_APP_SECRET=your-feishu-app-secret
OCLAWS_GATEWAY_AUTH_TOKEN=your-random-token-here
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

也可通过 Web 配置管理界面在线编辑：启动网关后访问 `http://127.0.0.1:8081/ui/config`。

## Web 界面

### 聊天 UI (`/ui/chat`)

内嵌的 Web 聊天界面，无需额外前端部署，启动网关即可使用。

功能：
- 与 LLM 实时对话，支持工具调用展示
- Markdown 渲染（代码块、引用、列表、链接，代码一键复制）
- 会话管理 — 顶部下拉切换/新建会话
- 模型切换 — 顶部下拉切换当前模型
- 斜杠命令 — 输入 `/` 弹出命令补全（/help、/clear、/model、/session、/abort 等）
- 键盘快捷键 — Enter 发送、Shift+Enter 换行、Escape/Ctrl+C 中止生成
- WebSocket 自动重连（指数退避 1s → 15s）

### 配置管理 UI (`/ui/config`)

可视化编辑 `config.json` 的全部字段，支持中英文切换，保存后即时生效。

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
├── gateway-core/   # HTTP + WebSocket 服务器、中间件、Webhook、Web UI
├── agent-core/     # 智能体编排、子智能体、模型降级、回声检测、上下文压缩
├── llm-core/       # 多供应商 LLM 集成（9 个供应商）
├── channel-core/   # 消息渠道适配器（10 个渠道）
├── tools-core/     # 工具执行框架
├── storage-core/   # 数据库抽象层 (SQLite/PG/向量)、时间衰减、查询扩展
├── memory-core/    # 长期记忆管理、嵌入搜索、文件监控
├── config/         # 配置管理与验证
├── plugin-core/    # 插件系统（含 HTTP 路由、工具注册）
├── skills-core/    # 技能注册、发现、安装
├── cron-core/      # 定时任务调度与持久化
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
| `/ws` | GET | WebSocket 协议连接 |
| `/webchat/ws` | GET | Web 聊天 WebSocket |
| `/v1/chat/completions` | POST | OpenAI 兼容聊天 API |
| `/v1/responses` | POST | 响应 API |
| `/agent/status` | GET | 智能体状态 |
| `/sessions` | GET | 列出会话 |
| `/sessions/{key}` | DELETE | 删除会话 |
| `/config` | GET | 获取网关配置 |
| `/config/reload` | POST | 重新加载配置 |
| `/models` | GET | 列出可用模型 |
| `/metrics` | GET | Prometheus 指标 |
| `/cron/jobs` | GET/POST | 列出/创建定时任务 |
| `/cron/jobs/{id}` | DELETE | 删除定时任务 |
| `/api/config/full` | GET/PUT | 读取/写入完整配置 |
| `/ui/chat` | GET | Web 聊天界面 |
| `/ui/config` | GET | Web 配置管理界面 |
| `/webhooks/telegram` | POST | Telegram Webhook |
| `/webhooks/slack` | POST | Slack Webhook |
| `/webhooks/discord` | POST | Discord Webhook |
| `/webhooks/feishu` | POST | 飞书 Webhook |
| `/webhooks/{channel}` | POST | 通用渠道 Webhook |

## 许可证

MIT
