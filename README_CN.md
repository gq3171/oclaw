# oclaw

[English](README.md)

一个用 Rust 编写的模块化 AI 智能体网关框架。单一二进制文件，零外部依赖，开箱即用。

## 为什么选择 oclaw

- **单一二进制** — 一个 `oclaws` 文件搞定一切。不需要 Node.js、Python 或 Docker，单文件即可部署到任何环境。
- **极致性能** — 纯 Rust 编写，异步优先架构。以极低的内存占用处理数千并发连接。
- **18 大 LLM 供应商** — Anthropic、OpenAI、Google Gemini、Cohere、Ollama、AWS Bedrock、OpenRouter、Together AI、MiniMax、Hugging Face、vLLM、通义千问、豆包、Moonshot、xAI、Cloudflare AI、LiteLLM、GitHub Copilot。一行配置切换，供应商故障自动降级。
- **19 个消息渠道** — Telegram、Slack、Discord、WhatsApp、Matrix、Signal、LINE、Mattermost、Google Chat、飞书、Nostr、IRC、网页聊天、iMessage/BlueBubbles、Microsoft Teams、Nextcloud Talk、Synology Chat、Twitch、Zalo。
- **跨端记忆聊天** — 统一记忆管线：召回相关上下文 → 智能体执行 → 自动捕获关键信息。同一用户在 Telegram、Discord、Slack 等不同渠道共享记忆。
- **工作区与身份** — 通过 `SOUL.md` 定义智能体人格，首次运行自动进行身份发现对话（Hatching），运行时自感知（模型、系统、工具、版本）。
- **内置 Web 界面** — 聊天界面、配置管理和实时画布，全部内嵌在二进制文件中。
- **OpenAI 兼容 API** — 直接替代 `/v1/chat/completions` 和 `/v1/responses` 端点，兼容所有 OpenAI 客户端。
- **48 个 WebSocket RPC 方法** — 完整的编程控制：会话、智能体、定时任务、TTS、节点配对、配置、向导等。
- **企业级特性** — OAuth 2.0、速率限制、TLS、Prometheus 指标、OpenTelemetry、结构化日志、健康检查、定时任务、插件系统。
- **中英文配置界面** — 可视化配置编辑器，完整中英文支持。在浏览器中编辑所有设置，即时保存生效。

## 快速开始

### 前置要求

- Rust 1.85+（edition 2024）

### 安装与运行

```bash
# 克隆并构建
git clone https://github.com/gq3171/oclaw.git
cd oclaw
cargo build --release

# 初始化配置
./target/release/oclaws config init

# 或使用交互式向导
./target/release/oclaws wizard

# 启动网关
./target/release/oclaws start --port 8080
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

### 配置

配置文件位置：`~/.oclaws/config.json`（Linux/macOS）或 `%APPDATA%\oclaws\config.json`（Windows）。

三种配置方式：

1. **Web 界面** — 访问 `http://127.0.0.1:8081/ui/config`，所有字段预渲染的可视化编辑器
2. **CLI 向导** — 运行 `oclaws wizard` 进行交互式配置
3. **JSON 文件** — 直接编辑 `config.json`，参考 [`config.example.json`](config.example.json)

```bash
# CLI 配置命令
oclaws config init          # 创建默认配置
oclaws config show          # 显示当前配置
oclaws config validate      # 验证配置
oclaws channel setup        # 设置消息渠道
oclaws provider setup       # 设置 LLM 供应商
oclaws skill setup          # 设置技能
```

## Web 界面

### 聊天 UI (`/ui/chat`)

内嵌的 Web 聊天界面，无需额外前端部署，启动网关即可使用。

- 与 LLM 实时对话，支持流式响应
- 工具调用可视化，可折叠的工具卡片
- Markdown 渲染（代码块、引用、列表、链接，代码一键复制）
- 会话管理 — 顶部下拉切换/新建会话
- 模型切换 — 顶部下拉切换当前模型
- 斜杠命令 — 输入 `/` 弹出命令补全（/help、/clear、/model、/session、/abort 等）
- 键盘快捷键 — Enter 发送、Shift+Enter 换行、Escape/Ctrl+C 中止生成
- WebSocket 自动重连（指数退避 1s → 15s）

### 配置管理 UI (`/ui/config`)

可视化配置编辑器，完整中英文支持。

- 9 个配置页面：网关、模型、频道、会话、浏览器、定时任务、记忆、日志、高级设置
- 所有字段预渲染 — 无需手动添加配置项
- 供应商管理，支持添加/删除和类型选择
- 频道卡片，支持启用/禁用开关和独立配置
- 配置导入/导出为 JSON
- 实时保存并验证

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

## API 端点

### OpenAI 兼容接口

| 端点 | 方法 | 说明 |
|------|------|------|
| `/v1/chat/completions` | POST | 聊天补全（支持流式和非流式） |
| `/v1/responses` | POST | 响应 API |

### 网关管理

| 端点 | 方法 | 说明 |
|------|------|------|
| `/health` | GET | 存活检查 |
| `/ready` | GET | 就绪检查（含组件健康状态） |
| `/ws` | GET | WebSocket 协议连接 |
| `/webchat/ws` | GET | Web 聊天 WebSocket |
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

### Web 界面

| 端点 | 方法 | 说明 |
|------|------|------|
| `/ui/chat` | GET | Web 聊天界面 |
| `/ui/config` | GET | Web 配置管理界面 |
| `/ui/canvas` | GET | 实时画布渲染 |

### Webhooks

| 端点 | 方法 | 说明 |
|------|------|------|
| `/webhooks/telegram` | POST | Telegram Webhook |
| `/webhooks/slack` | POST | Slack Webhook |
| `/webhooks/discord` | POST | Discord Webhook |
| `/webhooks/whatsapp` | POST | WhatsApp Webhook |
| `/webhooks/feishu` | POST | 飞书 Webhook |
| `/webhooks/{channel}` | POST | 通用渠道 Webhook |

## 功能状态

与 [Node OpenClaw](https://github.com/nicepkg/openclaw) 参考实现对比。

### 网关与网络

- [x] HTTP 服务器 + REST API
- [x] WebSocket 服务器
- [x] TLS / SSL
- [x] Tailscale 集成
- [x] Webhook 支持（Telegram、Slack、Discord、WhatsApp、飞书、通用）
- [x] OpenAI 兼容 API（`/v1/chat/completions`、`/v1/responses`）
- [x] 速率限制
- [x] CORS
- [x] Prometheus 指标（`/metrics`）
- [x] Web 聊天界面（`/ui/chat`）
- [x] Web 配置管理（`/ui/config`）
- [x] OpenTelemetry 链路追踪
- [x] Canvas Host（实时画布渲染）

### LLM 供应商

- [x] Anthropic (Claude)
- [x] OpenAI (GPT)
- [x] Google Gemini
- [x] Cohere
- [x] Ollama（本地）
- [x] AWS Bedrock
- [x] OpenRouter
- [x] Together AI
- [x] MiniMax
- [x] Hugging Face
- [x] vLLM
- [x] 通义千问 (Qwen)
- [x] 豆包 / 火山引擎 (Doubao / Volcengine)
- [x] Moonshot (Kimi)
- [x] xAI (Grok)
- [x] Cloudflare AI Gateway
- [x] LiteLLM
- [x] GitHub Copilot

### 消息渠道

- [x] Telegram
- [x] Slack
- [x] Discord
- [x] WhatsApp
- [x] Matrix
- [x] Signal
- [x] LINE
- [x] Mattermost
- [x] Google Chat
- [x] 飞书 (Feishu / Lark)
- [x] Nostr
- [x] IRC
- [x] 网页聊天（内置）
- [x] iMessage / BlueBubbles
- [x] Microsoft Teams
- [x] Nextcloud Talk
- [x] Synology Chat
- [x] Twitch
- [x] Zalo

### 智能体与编排

- [x] 多智能体编排
- [x] 子智能体系统
- [x] 模型降级链
- [x] 循环检测
- [x] 回声检测
- [x] 会话持久化（Transcript）
- [x] 历史压缩与裁剪
- [x] 工具变更追踪
- [x] 流式分块
- [x] 自动召回（记忆集成）
- [x] 线程所有权
- [x] 回复分发
- [x] 思考模式（扩展推理）
- [x] 上下文窗口守卫
- [x] 跨端记忆管线（召回 → 智能体 → 捕获 → 回复）
- [x] 工作区身份（SOUL.md、IDENTITY.md）
- [x] Hatching 引导（首次运行身份发现对话）
- [x] 运行时自感知（模型、系统、架构、工具、版本）
- [x] 跨平台会话身份（DmScope + IdentityLinks）
- [x] 记忆刷写至工作区文件
- [x] 智能体通信协议（ACP）

### 工具与集成

- [x] 工具执行框架
- [x] 工具调度
- [x] 工具审批门控
- [x] 浏览器自动化（CDP）
- [x] Docker 沙箱执行
- [x] 网页搜索（Brave / Perplexity）
- [x] 网页抓取（Firecrawl）
- [x] Playwright 集成

### 存储与记忆

- [x] SQLite
- [x] PostgreSQL
- [x] LanceDB（向量）
- [x] 向量搜索
- [x] 全文搜索
- [x] 混合搜索
- [x] MMR 重排序
- [x] 查询扩展
- [x] 时间衰减
- [x] 语义记忆
- [x] 嵌入搜索
- [x] 嵌入缓存
- [x] 文件监控索引
- [x] 自动捕获（对话 → 记忆）
- [x] 记忆刷写（持久化至工作区文件）

### 技能与插件

- [x] 技能注册与发现
- [x] 技能安装
- [x] 技能门控
- [x] 内置技能
- [x] 插件系统（加载、钩子、HTTP 路由）
- [x] 工作区技能

### 媒体与语音

- [x] 图像处理
- [x] 音频处理
- [x] MIME 检测
- [x] 媒体理解（图像/音频/视频分析，多供应商）
- [x] STT（语音转文字）
- [x] TTS（文字转语音，多供应商，指令解析）
- [x] 音频流（WebSocket）
- [x] ElevenLabs TTS
- [x] Deepgram STT
- [x] 语音唤醒检测

### 安全

- [x] OAuth 2.0
- [x] Token / 密码认证
- [x] 设备配对
- [x] 节点配对（点对点网格，白名单与配对码）
- [x] HMAC / SHA2 加密
- [x] 审计日志
- [x] 多密钥轮换（Auth Profiles）

### CLI 与界面

- [x] 完整 CLI（`start`、`config`、`wizard`、`channel`、`skill`、`doctor`、`provider` 等）
- [x] 交互式配置向导
- [x] 终端 UI（ratatui）
- [x] 后台服务管理
- [x] 系统诊断（`doctor`）
- [x] 引导命令（Onboarding）

### 定时任务与后台

- [x] 定时任务调度与持久化
- [x] 退避与错峰
- [x] 运行日志与遥测
- [x] 定时任务事件系统
- [x] 会话回收
- [x] 心跳系统
- [x] 进程监控
- [x] 信号处理

## 架构

23 个 crate 组成的 Cargo workspace（edition 2024，resolver v2）：

```
crates/
├── cli/                 # CLI 二进制文件 (oclaws)
├── protocol/            # 基于帧的通信协议
├── gateway-core/        # HTTP + WebSocket 服务器、中间件、Webhook、Web UI、记忆管线
├── agent-core/          # 智能体编排、子智能体、模型降级、回声检测、上下文压缩
├── llm-core/            # 多供应商 LLM 集成（18 个供应商）
├── channel-core/        # 消息渠道适配器（19 个渠道）
├── tools-core/          # 工具执行框架、审批门控、配置文件
├── storage-core/        # 数据库抽象层 (SQLite/PG/向量)、时间衰减、查询扩展
├── memory-core/         # 长期记忆管理、嵌入搜索、自动捕获、MMR 重排序
├── workspace-core/      # 智能体工作区（SOUL.md、IDENTITY.md、心跳、记忆刷写、引导）
├── config/              # 配置管理、验证、迁移
├── plugin-core/         # 插件系统（HTTP 路由、工具注册、钩子）
├── skills-core/         # 技能注册、发现、安装
├── cron-core/           # 定时任务调度、持久化、退避、遥测
├── security-core/       # OAuth、加密、审计
├── sandbox-core/        # 沙箱执行
├── doctor-core/         # 健康检查与诊断
├── voice-core/          # 音频流 (STT/TTS)
├── media-understanding/ # 图像/音频/视频分析（多供应商支持）
├── tts-core/            # 文字转语音合成（多供应商、指令解析）
├── acp/                 # 智能体通信协议（跨智能体消息传递）
├── auto-reply/          # 消息处理管线（规范化 → 上下文 → 智能体 → 分发）
├── pairing/             # 设备/节点配对（白名单、配对码）
├── browser-core/        # 浏览器自动化 (CDP)
├── tui-core/            # 终端 UI (ratatui)
└── daemon-core/         # 后台服务
```

## 开发

```bash
cargo test --workspace --all-features    # 运行所有测试
cargo test -p oclaws-security-core       # 测试单个 crate
cargo clippy --workspace --all-features -- -D warnings
cargo fmt --all -- --check
```

## 许可证

MIT
