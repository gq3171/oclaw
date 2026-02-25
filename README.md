# oclaw

[中文文档](README_CN.md)

A modular AI agent gateway framework written in Rust. Single binary, zero external dependencies, production-ready.

## Why oclaw

- **Single Binary** — One `oclaws` binary does everything. No Node.js, no Python, no Docker required. Deploy anywhere with a single file.
- **Blazing Fast** — Pure Rust, async-first architecture. Thousands of concurrent connections with minimal memory footprint.
- **18 LLM Providers** — Anthropic, OpenAI, Google Gemini, Cohere, Ollama, AWS Bedrock, OpenRouter, Together AI, MiniMax, Hugging Face, vLLM, Qwen, Doubao, Moonshot, xAI, Cloudflare AI, LiteLLM, GitHub Copilot. One config change to switch, automatic fallback chains.
- **19 Messaging Channels** — Telegram, Slack, Discord, WhatsApp, Matrix, Signal, LINE, Mattermost, Google Chat, Feishu, Nostr, IRC, Webchat, iMessage/BlueBubbles, Microsoft Teams, Nextcloud Talk, Synology Chat, Twitch, Zalo.
- **Cross-Channel Memory** — Unified memory pipeline: recall relevant context → agent execution → auto-capture key info. Same user gets the same memory across Telegram, Discord, Slack, etc.
- **Workspace & Identity** — Agent personality via `SOUL.md`, first-run hatching conversation for identity discovery, runtime self-awareness (model, OS, tools, version).
- **Built-in Web UI** — Chat interface, configuration manager, and live canvas — all embedded in the binary.
- **OpenAI-Compatible API** — Drop-in replacement for `/v1/chat/completions` and `/v1/responses`. Works with any OpenAI client.
- **48 WebSocket RPC Methods** — Full programmatic control: sessions, agents, cron, TTS, node pairing, config, wizard, and more.
- **Enterprise Features** — OAuth 2.0, rate limiting, TLS, Prometheus metrics, OpenTelemetry, structured logging, health checks, cron jobs, plugin system.
- **i18n Config UI** — Visual configuration editor with full English/Chinese support. Edit all settings in the browser, save instantly.

## Quick Start

### Prerequisites

- Rust 1.85+ (edition 2024)

### Install & Run

```bash
# Clone and build
git clone https://github.com/gq3171/oclaw.git
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
| `/ui/canvas` | GET | Live canvas rendering |

### Webhooks

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/webhooks/telegram` | POST | Telegram webhook |
| `/webhooks/slack` | POST | Slack webhook |
| `/webhooks/discord` | POST | Discord webhook |
| `/webhooks/whatsapp` | POST | WhatsApp webhook |
| `/webhooks/feishu` | POST | Feishu webhook |
| `/webhooks/{channel}` | POST | Generic channel webhook |

## Feature Status

Compared with the [Node OpenClaw](https://github.com/nicepkg/openclaw) reference implementation.

### Gateway & Networking

- [x] HTTP Server + REST API
- [x] WebSocket Server
- [x] TLS / SSL
- [x] Tailscale Integration
- [x] Webhook Support (Telegram, Slack, Discord, WhatsApp, Feishu, Generic)
- [x] OpenAI-Compatible API (`/v1/chat/completions`, `/v1/responses`)
- [x] Rate Limiting
- [x] CORS
- [x] Prometheus Metrics (`/metrics`)
- [x] Web Chat UI (`/ui/chat`)
- [x] Web Config UI (`/ui/config`)
- [x] OpenTelemetry Tracing
- [x] Canvas Host (live canvas rendering)

### LLM Providers

- [x] Anthropic (Claude)
- [x] OpenAI (GPT)
- [x] Google Gemini
- [x] Cohere
- [x] Ollama (local)
- [x] AWS Bedrock
- [x] OpenRouter
- [x] Together AI
- [x] MiniMax
- [x] Hugging Face
- [x] vLLM
- [x] Qwen (Alibaba)
- [x] Doubao / Volcengine (ByteDance)
- [x] Moonshot (Kimi)
- [x] xAI (Grok)
- [x] Cloudflare AI Gateway
- [x] LiteLLM
- [x] GitHub Copilot

### Messaging Channels

- [x] Telegram
- [x] Slack
- [x] Discord
- [x] WhatsApp
- [x] Matrix
- [x] Signal
- [x] LINE
- [x] Mattermost
- [x] Google Chat
- [x] Feishu (Lark)
- [x] Nostr
- [x] IRC
- [x] Webchat (built-in)
- [x] iMessage / BlueBubbles
- [x] Microsoft Teams
- [x] Nextcloud Talk
- [x] Synology Chat
- [x] Twitch
- [x] Zalo

### Agent & Orchestration

- [x] Multi-agent Orchestration
- [x] Subagent System
- [x] Model Fallback Chains
- [x] Loop Detection
- [x] Echo Detection
- [x] Session Persistence (Transcript)
- [x] History Compaction & Pruning
- [x] Tool Mutation Tracking
- [x] Stream Chunking
- [x] Auto-recall (Memory Integration)
- [x] Thread Ownership
- [x] Reply Dispatch
- [x] Thinking Mode (Extended Reasoning)
- [x] Context Window Guard
- [x] Cross-channel Memory Pipeline (recall → agent → capture → reply)
- [x] Workspace Identity (SOUL.md, IDENTITY.md)
- [x] Hatching Bootstrap (first-run identity discovery conversation)
- [x] Runtime Self-awareness (model, OS, arch, tools, version)
- [x] Cross-platform Session Identity (DmScope + IdentityLinks)
- [x] Memory Flush to Workspace Files
- [x] Agent Communication Protocol (ACP)

### Tools & Integrations

- [x] Tool Execution Framework
- [x] Tool Scheduling
- [x] Tool Approval Gates
- [x] Browser Automation (CDP)
- [x] Docker Sandbox Execution
- [x] Web Search (Brave / Perplexity)
- [x] Web Scraping (Firecrawl)
- [x] Playwright Integration

### Storage & Memory

- [x] SQLite
- [x] PostgreSQL
- [x] LanceDB (Vector)
- [x] Vector Search
- [x] Full-text Search
- [x] Hybrid Search
- [x] MMR Reranking
- [x] Query Expansion
- [x] Temporal Decay
- [x] Semantic Memory
- [x] Embedding Search
- [x] Embedding Cache
- [x] File Watch Indexing
- [x] Auto-capture (conversation → memory)
- [x] Memory Flush (durable workspace files)

### Skills & Plugins

- [x] Skill Registry & Discovery
- [x] Skill Installation
- [x] Skill Gating
- [x] Built-in Skills
- [x] Plugin System (load, hooks, HTTP routes)
- [x] Workspace Skills

### Media & Voice

- [x] Image Processing
- [x] Audio Processing
- [x] MIME Detection
- [x] Media Understanding (image/audio/video analysis, multi-provider)
- [x] STT (Speech-to-Text)
- [x] TTS (Text-to-Speech, multi-provider with directives)
- [x] Audio Streaming (WebSocket)
- [x] ElevenLabs TTS
- [x] Deepgram STT
- [x] Voice Wake Detection

### Security

- [x] OAuth 2.0
- [x] Token / Password Auth
- [x] Device Pairing
- [x] Node Pairing (peer-to-peer mesh with allowlist and setup codes)
- [x] HMAC / SHA2 Crypto
- [x] Audit Logging
- [x] Multi-key Rotation (Auth Profiles)

### CLI & UI

- [x] Full CLI (`start`, `config`, `wizard`, `channel`, `skill`, `doctor`, `provider`, …)
- [x] Interactive Config Wizard
- [x] Terminal UI (ratatui)
- [x] Daemon Management
- [x] System Diagnostics (`doctor`)
- [x] Onboarding Command

### Cron & Background

- [x] Cron Scheduling & Persistence
- [x] Backoff & Stagger
- [x] Run Log & Telemetry
- [x] Cron Event System
- [x] Session Reaping
- [x] Heartbeat System
- [x] Process Monitoring
- [x] Signal Handling

## Architecture

23 crates organized as a Cargo workspace (edition 2024, resolver v2):

```
crates/
├── cli/                 # CLI binary (oclaws)
├── protocol/            # Frame-based wire protocol
├── gateway-core/        # HTTP + WebSocket server, middleware, webhooks, Web UI, memory pipeline
├── agent-core/          # Agent orchestration, subagents, model fallback, echo detection, compaction
├── llm-core/            # Multi-provider LLM integration (18 providers)
├── channel-core/        # Messaging channel adapters (19 channels)
├── tools-core/          # Tool execution framework, approval gates, profiles
├── storage-core/        # Database abstraction (SQLite/PG/vector), temporal decay, query expansion
├── memory-core/         # Long-term memory, embedding search, auto-capture, MMR reranking
├── workspace-core/      # Agent workspace (SOUL.md, IDENTITY.md, heartbeat, memory flush, bootstrap)
├── config/              # Configuration management, validation, migration
├── plugin-core/         # Plugin system (HTTP routes, tool registration, hooks)
├── skills-core/         # Skill registry, discovery, installation
├── cron-core/           # Cron scheduling, persistence, backoff, telemetry
├── security-core/       # OAuth, crypto, audit
├── sandbox-core/        # Sandboxed execution
├── doctor-core/         # Health checks and diagnostics
├── voice-core/          # Audio streaming (STT/TTS)
├── media-understanding/ # Image/audio/video analysis with multi-provider support
├── tts-core/            # Text-to-speech synthesis (multi-provider, directives)
├── acp/                 # Agent Communication Protocol (inter-agent messaging)
├── auto-reply/          # Message processing pipeline (normalize → context → agent → dispatch)
├── pairing/             # Device/node pairing with allowlist and setup codes
├── browser-core/        # Browser automation (CDP)
├── tui-core/            # Terminal UI (ratatui)
└── daemon-core/         # Background service
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
