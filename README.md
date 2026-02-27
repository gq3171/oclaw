# oclaw

[中文文档](README_CN.md)

`oclaw` is a Rust-first AI agent gateway focused on production deployment: one binary, modular crates, OpenAI-compatible APIs, built-in web UI, and multi-channel messaging integration.

## What oclaw Solves

- Run an agent gateway without a Node/Python runtime in production.
- Expose OpenAI-compatible endpoints (`/v1/chat/completions`, `/v1/responses`) for existing clients.
- Route conversations across channels (Telegram, Slack, Discord, Feishu, WhatsApp, etc.) with shared session and memory context.
- Operate through both CLI and Web UI (`/ui/chat`, `/ui/config`) with the same backend.
- Keep deployment and operations simple: one process, one config, strong observability hooks.

## Core Capabilities

- Modular Rust workspace (`crates/*`) with clear domain boundaries.
- Provider abstraction with fallback support.
- Channel abstraction with action routing (send/reply/thread/media/reaction/admin actions).
- Session lifecycle, transcript persistence, and cross-turn tool orchestration.
- Built-in diagnostics (`doctor`), daemon mode, plugin/skill management, and system events.
- Metrics/logging/tracing support for production operations.

## Quick Start

### 1) Build

```bash
git clone https://github.com/gq3171/oclaw.git
cd oclaw
cargo build --release
```

### 2) Initialize Configuration

```bash
./target/release/oclaw config init
# or first-time guided onboarding
./target/release/oclaw onboard
# or full setup wizard
./target/release/oclaw wizard
```

### 3) Start Gateway

```bash
./target/release/oclaw start --port 8080
```

Common local endpoints:

- WebSocket Gateway: `ws://127.0.0.1:8080/ws`
- HTTP API: `http://127.0.0.1:8081`
- Chat UI: `http://127.0.0.1:8081/ui/chat`
- Config UI: `http://127.0.0.1:8081/ui/config`

> Default behavior uses HTTP port = WS port + 1.

## Configuration

- Use `.env` for secrets (API keys, tokens) and reference them from config.
- Keep your runtime config outside the repo (user home/app data path).
- Validate before start:

```bash
./target/release/oclaw config validate
./target/release/oclaw doctor
```

## CLI Overview

Top-level commands:

- `start`
- `config` (`init|show|validate`)
- `wizard`, `onboard`
- `channel` (`setup|list`)
- `provider` (`setup|status`)
- `sessions` (`list|show|delete`)
- `models list`
- `plugin` (`list|info|enable|disable`)
- `system` (`event|heartbeat|presence`)
- `message`, `agent`, `status`, `version`, `tui`, `daemon`, `doctor`

Check real-time CLI help from your current binary:

```bash
./target/release/oclaw --help
./target/release/oclaw <command> --help
```

## API Surface

### OpenAI-Compatible

- `POST /v1/chat/completions`
- `POST /v1/responses`

### Gateway & Runtime

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

### Built-in Web UI

- `GET /ui/chat`
- `GET /ui/config`
- `GET /ui/canvas`

## Repository Structure

- `crates/cli`: binary entrypoint and command wiring.
- `crates/gateway-core`: HTTP/WS server, routes, runtime bridge.
- `crates/agent-core`: agent loop, planning, orchestration internals.
- `crates/llm-core`: model/provider adapters.
- `crates/channel-core`: messaging channel implementations.
- `crates/tools-core`: tool registry, schemas, action intent mapping.
- `crates/memory-core`: memory store/retrieval pipeline.
- `crates/plugin-core`, `crates/skills-core`: extension system.

## Development Workflow

```bash
cargo fmt --all
cargo check --workspace
cargo test --workspace
cargo build --release -p oclaw
```

Recommended before PR:

- Keep changes scoped by domain crate.
- Add regression tests for behavior changes.
- Verify CLI help and endpoint behavior after command/interface changes.

## Compatibility Note

This project continuously aligns behavior with the Node reference implementation (`openclaw`) while preserving Rust-native architecture and operational model.

## License

MIT
