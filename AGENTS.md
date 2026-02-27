# Repository Guidelines

## Project Structure & Module Organization
This repository is a Rust workspace centered on the `oclaw` binary (`crates/cli`). Core functionality is split into domain crates under `crates/`, such as `gateway-core`, `agent-core`, `llm-core`, `channel-core`, and `security-core`. Shared protocol and config logic live in `crates/protocol` and `crates/config`.  
Tests are mostly inline (`mod tests`) inside each crate, with integration tests in `crates/gateway-core/tests`. Examples live in `crates/memory-core/examples`. Release helpers are in `scripts/release.sh` and `scripts/release.bat`.

## Build, Test, and Development Commands
- `cargo build --release`: build optimized production binary (`target/release/oclaw`).
- `cargo run -p oclaw -- start --port 8080`: run gateway locally.
- `cargo test --workspace --all-features`: run full workspace test suite.
- `cargo test -p oclaw-security-core`: run tests for a single crate.
- `cargo fmt --all -- --check`: verify formatting.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: lint and treat warnings as errors.

## Coding Style & Naming Conventions
Use Rust 2024 idioms and keep code `rustfmt`-clean. Follow standard Rust naming:
- `snake_case` for files, modules, and functions
- `PascalCase` for types/traits/enums
- `SCREAMING_SNAKE_CASE` for constants

Crate names follow `oclaw-<domain>`; prefer focused crate boundaries and explicit public APIs over cross-crate implicit coupling.

## Testing Guidelines
Place unit tests near implementation (`mod tests`) and async tests under `#[tokio::test]` when needed. Add regression tests with each bug fix and feature-level tests for routing, protocol, or security changes.  
There is no explicit coverage percentage gate, but CI must pass `test`, `fmt`, and `clippy`.

## Commit & Pull Request Guidelines
Commit history follows Conventional Commit prefixes (`feat:`, `fix:`, `docs:`). Keep subjects imperative and specific (e.g., `fix: unify config directory naming`).  
PRs should include:
- concise change summary and motivation
- linked issue/task when available
- test evidence (commands run and results)
- screenshots or sample payloads for UI/API behavior changes

## Security & Configuration Tips
Do not commit secrets. Use `.env` (from `.env.example`) and validate configuration before running:
- `oclaw config validate`
- `oclaw doctor`
