# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build & Development Commands

```bash
cargo build --workspace              # Build all crates
cargo build -p oclaw-cli             # Build just the CLI binary
cargo test --workspace --all-features # Run all tests
cargo test -p oclaw-protocol         # Run tests for a single crate
cargo fmt --all -- --check           # Check formatting
cargo clippy --workspace --all-targets --all-features -- -D warnings  # Lint
```

Release builds use `lto = true`, `opt-level = 3`, `codegen-units = 1`, `strip = true`.

## Architecture

OpenClaw (oclaw) is a Rust agent framework organized as a Cargo workspace (edition 2024, resolver v2) with 19 crates under `crates/`.

### Communication Flow

Clients connect via WebSocket to **gateway-core**, which handles sessions and routing. All communication uses a frame-based **protocol** with typed messages: Hello (handshake), Request/Response (RPC), Event (server push), and Error frames. Authentication supports None, Token, Password, and Device modes.

### Core Pipeline

- **cli** — Entry point binary (`oclaws`). Commands: `start`, `config`, `wizard`, `channel`, `skill`, `doctor`, `provider`, `version`.
- **gateway-core** — HTTP + WebSocket server (HTTP on port N+1, WS on port N). Includes TLS, Tailscale integration, webhooks, and web chat.
- **agent-core** — Agent orchestration with subagent registry and model fallback chains.
- **llm-core** — Multi-provider LLM integration (chat completions, embeddings, tokenization).
- **tools-core** — Tool execution framework for agents.
- **channel-core** — Channel-based communication layer between components.

### Data & Storage

- **storage-core** — Abstraction over SQLite (rusqlite), PostgreSQL (tokio-postgres), and vector search (lancedb). Supports vector, full-text, and hybrid search.
- **config** — Configuration management with validation and interactive setup.

### Extension Points

- **plugin-core** — Plugin system for extending functionality.
- **skills-core** — Skill registration and pattern-matched execution.
- **sandbox-core** — Sandboxed execution environment.

### Supporting Crates

- **security-core** — Auth providers, token handling, HMAC/SHA2 crypto.
- **voice-core** — Audio streaming over WebSocket.
- **media-core** — Image/audio processing (feature-gated: `image`, `audio`).
- **tui-core** — Terminal UI via ratatui (backends: `crossterm`, `termion`).
- **daemon-core** — Background service and process monitoring.
- **doctor-core** — System diagnostics (categories: system, network, config, deps, storage, security, performance).
- **browser-core** — Browser automation integration.

## Key Patterns

- Async-first with tokio. All I/O goes through async interfaces.
- Errors use `thiserror` for typed errors per crate, `anyhow` for ad-hoc context.
- Structured logging via `tracing`/`tracing-subscriber`.
- Workspace dependencies are declared in root `Cargo.toml` `[workspace.dependencies]` and inherited by crates.
