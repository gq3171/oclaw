# OpenClaw (oclaw)

[中文文档](README_CN.md)

A modular AI agent gateway framework written in Rust. Single binary, zero external dependencies, production-ready.

## Why OpenClaw

- **Single Binary** — One `oclaws` binary does everything. No Node.js, no Python, no Docker required. Deploy anywhere with a single file.
- **Blazing Fast** — Written in pure Rust with async-first architecture. Handles thousands of concurrent connections with minimal memory footprint (~28MB release binary).
- **9 LLM Providers** — Anthropic, OpenAI, Google, Cohere, Ollama, AWS Bedrock, OpenRouter, Together AI, MiniMax. Switch providers with one config change, automatic fallback chains when a provider goes down.
- **13 Messaging Channels** — Telegram, Slack, Discord, WhatsApp, Matrix, Signal, LINE, Mattermost, Google Chat, Feishu, Nostr, IRC, Webchat. Connect your AI to any platform.
- **Built-in Web UI** — Chat interface and full configuration management UI embedded in the binary. No separate frontend deployment needed.
- **OpenAI-Compatible API** — Drop-in replacement for OpenAI's `/v1/chat/completions` and `/v1/responses` endpoints. Works with any OpenAI-compatible client.
- **Enterprise Features** — OAuth 2.0, rate limiting, TLS, Prometheus metrics, OpenTelemetry, structured logging, health checks, cron jobs, plugin system.
- **i18n Config UI** — Visual configuration editor with full English/Chinese support. Edit all settings in the browser, save instantly.

## Quick Start

### Prerequisites

- Rust 1.85+ (edition 2024)

### Install & Run

```bash
# Clone and build
git clone https://github.com/anthropics/oclaw.git
cd oclaw
cargo build --release

# Initialize config
./target/release/oclaws config init

# Or use the interactive wizard
./target/release/oclaws wizard

# Start the gateway
./target/release/oclaws start --port 8080
```

After starting, visit:
- Web Chat: `http://127.0.0.1:8081/ui/chat`
- Config Manager: `http://127.0.0.1:8081/ui/config`
- WebSocket Gateway: `ws://127.0.0.1:8080/ws`

> HTTP port = WebSocket port + 1 (default WS 8080, HTTP 8081)

### Environment Variables

Supports `.env` file auto-loading (via dotenvy). You can also use `${VAR_NAME}` in `config.json` to reference environment variables.

```bash
cp .env.example .env
# Edit .env with your API keys and tokens
```

### Configuration

Config location: `~/.oclaws/config.json` (Linux/macOS) or `%APPDATA%\oclaws\config.json` (Windows).

Three ways to configure:

1. **Web UI** — Visit `http://127.0.0.1:8081/ui/config` for a visual editor with all fields pre-rendered
2. **CLI Wizard** — Run `oclaws wizard` for interactive setup
3. **JSON File** — Edit `config.json` directly. See [`config.example.json`](config.example.json) for reference

```bash
# CLI configuration commands
oclaws config init          # Create default config
oclaws config show          # Display current config
oclaws config validate      # Validate config
oclaws channel setup        # Setup messaging channels
oclaws provider setup       # Setup LLM providers
oclaws skill setup          # Setup skills
```

## Web Interfaces

### Chat UI (`/ui/chat`)

Built-in web chat interface — no separate frontend deployment needed.

- Real-time LLM conversation with streaming responses
- Tool call visualization with collapsible cards
- Markdown rendering (code blocks, quotes, lists, links, one-click copy)
- Session management — switch/create sessions from the header
- Model switching — change models on the fly
- Slash commands — type `/` for autocomplete (`/help`, `/clear`, `/model`, `/session`, `/abort`)
- Keyboard shortcuts — Enter to send, Shift+Enter for newline, Escape/Ctrl+C to abort
- Auto-reconnecting WebSocket (exponential backoff 1s → 15s)

### Config UI (`/ui/config`)

Visual configuration editor with full i18n support (English/Chinese).

- 9 configuration pages: Gateway, Models, Channels, Session, Browser, Cron, Memory, Logging, Advanced
- All fields pre-rendered — no manual field creation needed
- Provider management with add/remove and type selection
- Channel cards with enable/disable toggles and per-channel settings
- Import/Export configuration as JSON
- Real-time save with validation

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

## API Endpoints

### OpenAI-Compatible

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/v1/chat/completions` | POST | Chat completions (streaming & non-streaming) |
| `/v1/responses` | POST | Response API |

### Gateway Management

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/health` | GET | Liveness check |
| `/ready` | GET | Readiness check with component health |
| `/ws` | GET | WebSocket protocol connection |
| `/webchat/ws` | GET | Web chat WebSocket |
| `/agent/status` | GET | Agent status |
| `/sessions` | GET | List sessions |
| `/sessions/{key}` | DELETE | Delete session |
| `/config` | GET | Get gateway config |
| `/config/reload` | POST | Reload configuration |
| `/models` | GET | List available models |
| `/metrics` | GET | Prometheus metrics |
| `/cron/jobs` | GET/POST | List/create cron jobs |
| `/cron/jobs/{id}` | DELETE | Delete cron job |
| `/api/config/full` | GET/PUT | Read/write full configuration |

### Web UI

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/ui/chat` | GET | Web chat interface |
| `/ui/config` | GET | Configuration manager |

### Webhooks

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/webhooks/telegram` | POST | Telegram webhook |
| `/webhooks/slack` | POST | Slack webhook |
| `/webhooks/discord` | POST | Discord webhook |
| `/webhooks/feishu` | POST | Feishu webhook |
| `/webhooks/{channel}` | POST | Generic channel webhook |

## Architecture

```
crates/
├── cli/            # CLI binary (oclaws)
├── protocol/       # Frame-based wire protocol
├── gateway-core/   # HTTP + WebSocket server, middleware, webhooks, Web UI
├── agent-core/     # Agent orchestration, subagents, model fallback, echo detection, compaction
├── llm-core/       # Multi-provider LLM integration (9 providers)
├── channel-core/   # Messaging channel adapters (13 channels)
├── tools-core/     # Tool execution framework
├── storage-core/   # Database abstraction (SQLite/PG/vector), temporal decay, query expansion
├── memory-core/    # Long-term memory, embedding search, file watch indexing
├── config/         # Configuration management and validation
├── plugin-core/    # Plugin system (HTTP routes, tool registration)
├── skills-core/    # Skill registry, discovery, installation
├── cron-core/      # Cron scheduling and persistence
├── security-core/  # OAuth, crypto, audit
├── sandbox-core/   # Sandboxed execution
├── doctor-core/    # Health checks and diagnostics
├── voice-core/     # Audio streaming (STT/TTS)
├── media-core/     # Image/audio processing
├── browser-core/   # Browser automation
├── tui-core/       # Terminal UI (ratatui)
└── daemon-core/    # Background service
```

## Development

```bash
cargo test --workspace --all-features    # Run all tests
cargo test -p oclaws-security-core       # Test a single crate
cargo clippy --workspace --all-features -- -D warnings
cargo fmt --all -- --check
```

## License

MIT
