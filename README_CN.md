# oclaw

[English](README.md)

`oclaw` 是一个以 Rust 为核心的 AI 智能体网关，面向生产部署：单一二进制、模块化 crates、OpenAI 兼容接口、内置 Web UI，以及多渠道消息接入。

## oclaw 解决什么问题

- 在生产环境运行智能体网关，不依赖 Node/Python 运行时。
- 暴露 OpenAI 兼容接口（`/v1/chat/completions`、`/v1/responses`），可直接接入现有客户端。
- 在 Telegram、Slack、Discord、飞书、WhatsApp 等渠道之间统一会话与记忆上下文。
- 同时支持 CLI 与 Web UI（`/ui/chat`、`/ui/config`），共享同一后端能力。
- 降低部署和运维复杂度：单进程、单配置、可观测性完善。

## 核心能力

- 模块化 Rust 工作区（`crates/*`），领域边界清晰。
- Provider 抽象与故障降级链。
- Channel 抽象与动作路由（发送/回复/线程/媒体/反应/管理动作）。
- 会话生命周期、转录持久化、跨轮工具编排。
- 内置诊断（`doctor`）、守护进程模式、插件/技能管理、系统事件。
- 支持指标、日志、追踪，适合生产环境。

## 快速开始

### 1) 构建

```bash
git clone https://github.com/gq3171/oclaw.git
cd oclaw
cargo build --release
```

### 2) 初始化配置

```bash
./target/release/oclaw config init
# 或首次引导向导
./target/release/oclaw onboard
# 或完整交互式向导
./target/release/oclaw wizard
```

### 3) 启动网关

```bash
./target/release/oclaw start --port 8080
```

本地常用入口：

- WebSocket 网关：`ws://127.0.0.1:8080/ws`
- HTTP API：`http://127.0.0.1:8081`
- 聊天界面：`http://127.0.0.1:8081/ui/chat`
- 配置界面：`http://127.0.0.1:8081/ui/config`

> 默认行为为 HTTP 端口 = WS 端口 + 1。

## 配置说明

- 使用 `.env` 管理密钥（API Key、Token 等），并在配置中引用。
- 运行配置建议存放在仓库外（用户目录/AppData）。
- 启动前建议校验：

```bash
./target/release/oclaw config validate
./target/release/oclaw doctor
```

## CLI 总览

顶层命令：

- `start`
- `config`（`init|show|validate`）
- `wizard`、`onboard`
- `channel`（`setup|list`）
- `provider`（`setup|status`）
- `sessions`（`list|show|delete`）
- `models list`
- `plugin`（`list|info|enable|disable`）
- `system`（`event|heartbeat|presence`）
- `message`、`agent`、`status`、`version`、`tui`、`daemon`、`doctor`

请以当前二进制帮助为准：

```bash
./target/release/oclaw --help
./target/release/oclaw <command> --help
```

## API 能力面

### OpenAI 兼容接口

- `POST /v1/chat/completions`
- `POST /v1/responses`

### 网关与运行时

- `GET /health`
- `GET /ready`
- `GET /models`
- `GET /ws`
- `GET /webchat/ws`
- `GET /agent/status`
- `GET /sessions`
- `DELETE /sessions/{key}`
- `GET /metrics`
- `GET|POST /cron/jobs`
- `DELETE /cron/jobs/{id}`

### 内置 Web 界面

- `GET /ui/chat`
- `GET /ui/config`
- `GET /ui/canvas`

## 仓库结构

- `crates/cli`：二进制入口与命令分发。
- `crates/gateway-core`：HTTP/WS 服务、路由、运行时桥接。
- `crates/agent-core`：智能体循环、规划与编排内核。
- `crates/llm-core`：模型/供应商适配层。
- `crates/channel-core`：消息渠道实现。
- `crates/tools-core`：工具注册、Schema、动作意图映射。
- `crates/memory-core`：记忆存储与召回管线。
- `crates/plugin-core`、`crates/skills-core`：扩展系统。

## 开发工作流

```bash
cargo fmt --all
cargo check --workspace
cargo test --workspace
cargo build --release -p oclaw
```

提交前建议：

- 按领域 crate 控制改动范围。
- 行为变更必须补回归测试。
- 命令或接口变更后，复核 CLI 帮助与端点行为。

## 对齐说明

项目持续按 Node 参考实现（`openclaw`）进行行为对齐，同时保持 Rust 原生架构与运维模型。

## 许可证

MIT
