# Project LLM Instructions

akm-rs is a CLI tool for AKM (Agent Kit Manager).
It's a rewrite of akm, the initial mvp in Bash (https://github.com/akm-rs/akm/) 

This is a greenfield project !

## Tech stack

Rust
Packages : 
clap, clap_complete, serde, toml, serde_json, ratatui, crossterm, ureq, thiserror, dirs, assert_cmd, predicates, insta, tempfile

## Review Criteria

All implementation must satisfy these 12 criteria (details in spec):

1) Feature parity with Bash version
2) Proper error handling (Result<T>, no .unwrap())
3) Registry abstraction integrity (no git leakage)
4) Testability (DI, trait objects, temp dirs)
5) CLI contract (snapshot tests, --plain, non-TTY detection)
6) Config safety (typed structs, sane defaults)
7) XDG compliance
8) Idempotency
9) Shell init correctness (bash 4+)
10) No runtime dependencies (single binary, only git)
11) TUI graceful degradation
12) Documentation (rustdoc, --help, README)

## Test commands

- cargo test, cargo clippy, cargo fmt
