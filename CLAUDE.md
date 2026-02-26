# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build & Development Commands

```bash
cargo build --workspace                                                   # Build all crates
cargo build -p oclaw-cli                                                  # Build just the CLI binary
cargo test --workspace --all-features                                     # Run all tests
cargo test -p oclaw-protocol                                              # Run tests for a single crate
cargo test -p oclaw-agent-core -- context_guard                          # Run a single test by name
cargo fmt --all -- --check                                                # Check formatting
cargo clippy --workspace --all-targets --all-features -- -D warnings      # Lint (warnings are errors)
```

Release profile: `lto = "thin"`, `opt-level = 3`, `strip = true`.

Config files live in `~/.oclaw/` (Linux/macOS) or `%APPDATA%/oclaw` (Windows). Session transcripts persist as JSONL in `~/.oclaw/sessions/{session_id}.jsonl`.

## Architecture

OpenClaw (oclaw) is a Rust agent framework organized as a Cargo workspace (edition 2024, resolver v2) with **23 crates** under `crates/`.

### Communication Flow

```
CLI (oclaw binary)
  → Gateway-Core (WS port N / HTTP port N+1)
    → Pipeline (message → agent → reply → channel)
      → Agent-Core (LLM calls, tool execution, memory recall)
      → Channel-Core (fan-out to 20+ platforms)
      → Memory-Core (hybrid search + embeddings)
      → Workspace-Core (identity/personality persistence)
```

All wire communication uses **protocol** crate frames:
- `HelloFrame`/`HelloOk` — handshake with server info and auth negotiation
- `RequestFrame`/`ResponseFrame` — RPC calls
- `EventFrame` — server push (tick, presence, message, session)
- `ErrorFrame` — typed errors with optional retry metadata

Auth modes: `None`, `Token`, `Password`, `DeviceAuth` (ed25519 public key + signature).

### Core Pipeline

- **cli** — Entry point binary (`oclaw`). Commands: `start`, `config`, `wizard`, `channel`, `skill`, `doctor`, `provider`, `version`.
- **gateway-core** — HTTP + WebSocket server. Key modules: `pipeline.rs` (message routing), `session.rs` (lifecycle), `memory_bridge.rs`, `heartbeat_runner.rs`, `http/` (routes, webhooks, cron, metrics, agent_bridge). Includes TLS and Tailscale integration.
- **agent-core** — 25-module agent orchestration layer. Key capabilities:
  - Context window guarding (`context_guard.rs`): 128K default, 75% input budget, 50% tool result limit, hard min 16K
  - Compaction (`compaction.rs`): summarizes old turns when token budget exceeded; reserve 16KB, keep 20KB
  - Model fallback chains (`model_fallback.rs`): cooldown-aware provider switching
  - Extended thinking support (`thinking.rs`): Off/Low/Medium/High reasoning levels
  - Loop and echo detection (`loop_detect.rs`, `echo_detect.rs`)
  - Tool result pruning (`pruning.rs`) and history limiting (`history.rs`)
  - Subagent registry (`subagent.rs`): hierarchical spawning, status enum Pending→Ready→Running→Completed/Failed
  - Transcript repair (`transcript_repair.rs`): repairs orphaned tool use/result pairs in JSONL
- **llm-core** — Multi-provider LLM (chat completions, embeddings, tokenization via tiktoken-rs).
- **tools-core** — Tool execution framework for agents.
- **channel-core** — Unified abstraction over 20+ platforms (Telegram, WhatsApp, Discord, Slack, Signal, Line, Matrix, IRC, Teams, Feishu, Twitch, Zalo, Nostr, and more). Implements `MessageRouter`, `ChannelManager`, `ChannelFactory`, `ChannelRegistry`.

### Memory & Identity

- **memory-core** — Long-term memory: hybrid search (embeddings + SQLite FTS5), MMR reranking, query expansion with BM25, CJK support, embedding cache. Key types: `MemoryStore`, `HybridSearchConfig`, `AutoCaptureConfig`.
- **workspace-core** — Agent personality persistence. Manages: `SOUL.md` (personality template), `IDENTITY.md` (name, creature, vibe, emoji), `HEARTBEAT.md` (task checklist), `MEMORY.md` (user-editable facts). First-run bootstrap flow (hatching) prompts for agent identity. `memory_flush.rs` handles reactive memory capture (supports `SILENT_REPLY_TOKEN`).

**Memory pattern**: Agents use tool-based recall — calling `memory_search`/`memory_get` tools explicitly, not auto-capture. Memory writes happen via reactive flush from `workspace-core::memory_flush`. `DmScope=Main` ensures cross-channel sessions share the same memory scope.

**Agent binding routing**: `config.bindings` (array of `AgentBinding`) routes messages to named agents using 8-tier priority: `peer > guild-roles > guild > team > account > channel > default`. Resolved in `gateway-core::pipeline::resolve_agent_for_message()`. Metadata keys: `user_id`, `guild_id`, `team_id`, `account_id`, `role_ids` (comma-separated).

### Data & Storage

- **storage-core** — Abstraction over SQLite (rusqlite, bundled), PostgreSQL (tokio-postgres), and vector search (lancedb). Supports vector, full-text, and hybrid search.
- **config** — Config management: `load_or_create` pattern, env overrides, validation. Config at `~/.oclaw/config.json`.

### Extension & Scheduling

- **plugin-core** — Plugin system for extending functionality.
- **skills-core** — Skill registration and pattern-matched execution.
- **cron-core** — Cron job scheduling (`cron = 0.15`). Jobs executed via gateway's `http/cron_executor.rs`.
- **auto-reply** — Pattern-matched automatic replies (regex, SHA2-based, UUID tracking).

### Supporting Crates

- **security-core** — Auth providers, token handling, HMAC/SHA2, ed25519 signatures.
- **acp** — Lightweight async protocol abstraction (tokio, UUID, tracing-based).
- **pairing** — Device pairing (UUID, random token exchange).
- **tts-core** — Text-to-speech via HTTP.
- **media-understanding** — Image/audio analysis (base64, multipart).
- **tui-core** — Terminal UI via ratatui. Modules: `app.rs` (TuiApp), `chat.rs` (message model), `commands.rs` (slash commands), `gateway.rs` (WS client), `render.rs` (stdout formatting), `theme.rs` (colors).
- **daemon-core** — Background service and process monitoring.
- **doctor-core** — System diagnostics (categories: system, network, config, deps, storage, security, performance).
- **browser-core** — Browser automation integration.

## Key Patterns

- **Async-first** — tokio everywhere; all I/O via `async-trait`.
- **Typed errors** — `thiserror` per crate, `anyhow` for ad-hoc context.
- **Structured logging** — `tracing`/`tracing-subscriber` with optional OpenTelemetry (`opentelemetry-otlp`).
- **Workspace dependencies** — Declared in root `Cargo.toml` `[workspace.dependencies]`, inherited via `workspace = true`.
- **Crate naming** — Workspace member path crates are named `oclaw-{name}` (e.g., `oclaw-agent-core`) even though directories are `crates/{name}`.
