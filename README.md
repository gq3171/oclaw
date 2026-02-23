# OpenClaw (oclaw)

A modular AI agent gateway framework written in Rust. It provides WebSocket/HTTP gateway services, multi-provider LLM integration, multi-channel messaging, plugin/skill extensions, and a terminal UI.

## Features

- **Gateway** — HTTP + WebSocket server with TLS, rate limiting, security headers, graceful shutdown
- **Multi-LLM** — Anthropic, OpenAI, AWS Bedrock, OpenRouter, Together AI, with model fallback chains
- **Channels** — Telegram, Slack, Discord, Matrix, Signal, LINE, Mattermost, Google Chat, Feishu, MS Teams
- **Agent** — Orchestration with subagent registry, tool execution, loop detection
- **Storage** — SQLite, PostgreSQL, vector search (LanceDB), full-text and hybrid search
- **Security** — OAuth 2.0 (Google/Discord/GitHub/Slack), CSRF protection, HMAC verification, TLS with webpki-roots
- **Extensions** — Plugin system, skill registry with pattern matching, sandboxed execution
- **Observability** — Prometheus metrics, structured JSON logging, health checks, system diagnostics
- **UI** — Terminal UI (ratatui), web chat, control panel

## Quick Start

### Prerequisites

- Rust 1.85+ (edition 2024)
- SQLite (bundled) or PostgreSQL (optional)

### Build & Run

```bash
cargo build --workspace
cargo run -p oclaw-cli -- start --port 8080
```

### Configuration

```bash
# Initialize config
cargo run -p oclaw-cli -- config init

# Interactive setup wizard
cargo run -p oclaw-cli -- wizard

# Setup channels / providers / skills
cargo run -p oclaw-cli -- channel setup
cargo run -p oclaw-cli -- provider setup
cargo run -p oclaw-cli -- skill setup
```

Config location: `~/.oclaws/config.json` (Linux/macOS) or `%APPDATA%\oclaws\config.json` (Windows).

## CLI Commands

| Command | Description |
|---------|-------------|
| `start` | Start the gateway server (`--port`, `--host`, `--http-only`, `--ws-only`) |
| `config init\|show\|validate` | Manage configuration |
| `wizard` | Interactive setup wizard |
| `channel setup\|list` | Manage messaging channels |
| `skill setup\|list` | Manage skills |
| `provider setup\|status` | Manage LLM providers |
| `agent -m "message"` | Send a message to an agent |
| `sessions list\|show\|delete` | Manage sessions |
| `models list` | List available models |
| `doctor` | Run system diagnostics |
| `daemon start\|stop\|status` | Background service management |
| `tui` | Launch terminal UI |
| `status` | Show gateway status |

### Global Flags

```
--log-level <LEVEL>    Log level: trace, debug, info, warn, error (default: info)
--log-format <FORMAT>  Log format: text, json (default: text)
--config <PATH>        Config file path
--gateway-url <URL>    Gateway URL (default: http://127.0.0.1:8081)
```

## Architecture

```
crates/
├── cli/            # CLI binary (oclaws)
├── protocol/       # Frame-based wire protocol
├── gateway-core/   # HTTP + WebSocket server, middleware, webhooks
├── agent-core/     # Agent orchestration, subagents, model fallback
├── llm-core/       # Multi-provider LLM integration
├── channel-core/   # Messaging channel adapters
├── tools-core/     # Tool execution framework
├── storage-core/   # Database abstraction (SQLite/PG/vector)
├── config/         # Configuration management
├── plugin-core/    # Plugin system
├── skills-core/    # Skill registry
├── security-core/  # OAuth, crypto, audit
├── sandbox-core/   # Sandboxed execution
├── doctor-core/    # Health checks & diagnostics
├── voice-core/     # Audio streaming (STT/TTS)
├── media-core/     # Image/audio processing
├── browser-core/   # Browser automation
├── tui-core/       # Terminal UI
└── daemon-core/    # Background service
```

## Development

```bash
cargo test --workspace --all-features    # Run all tests
cargo test -p oclaws-security-core       # Test a single crate
cargo clippy --workspace --all-features -- -D warnings
cargo fmt --all -- --check
```

## API Endpoints

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/health` | GET | Liveness check |
| `/ready` | GET | Readiness check with component health |
| `/ws` | GET | WebSocket connection |
| `/v1/chat/completions` | POST | OpenAI-compatible chat API |
| `/v1/responses` | POST | Response API |
| `/agent/status` | GET | Agent status |
| `/sessions` | GET | List sessions |
| `/config` | GET | Get configuration |
| `/config/reload` | POST | Reload configuration |
| `/models` | GET | List available models |
| `/metrics` | GET | Prometheus metrics |
| `/webhooks/{channel}` | POST | Channel webhooks |

## License

MIT
