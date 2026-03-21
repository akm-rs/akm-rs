# Project LLM Instructions

akm-rs is a CLI tool for AKM (Agent Kit Manager).
It's a rewrite of akm, the initial mvp in Bash (https://github.com/akm-rs/akm/) 

It has reached feature parity with the Bash version and now extends beyond it (e.g. `akm skills import` for GitHub URL imports).

## Tech stack

Rust
Packages:
clap, clap_complete, serde, toml, serde_json, ratatui, crossterm, ureq (HTTP client for GitHub API), thiserror, dirs, tempfile, assert_cmd, predicates, insta

## Review Criteria

All implementation must satisfy these criteria:

1) Proper error handling (Result<T>, no .unwrap(), IoContext for wrapping IO errors)
2) Registry abstraction integrity (no git leakage)
3) Testability (DI, trait objects, temp dirs)
4) CLI contract (snapshot tests, --plain, non-TTY detection)
5) Config safety (typed structs, sane defaults)
6) XDG compliance
7) Idempotency
8) Shell init correctness (bash 4+)
9) No runtime dependencies (single binary, only git)
10) TUI graceful degradation
11) Documentation (rustdoc, --help, README)
12) ureq 3.x API patterns where applicable (match-by-value on errors, body_mut().read_json())

## Test commands

- cargo test, cargo clippy, cargo fmt
