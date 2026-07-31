# Project LLM Instructions

akm-rs is a CLI tool for AKM (Agent Kit Manager). It manages a shared library of
skills, agents and instructions and wires them into coding harnesses per project
and per session.

## Tech stack

Rust (MSRV 1.88).

Packages: clap, clap_complete, serde, toml, serde_json, ratatui, crossterm,
ureq (HTTP client for the GitHub API), thiserror, dirs, tempfile, assert_cmd,
predicates, insta.

No runtime dependencies beyond git — the release artifact is a single binary.

## Review Criteria

All implementation must satisfy these criteria:

1) Proper error handling (Result<T>, no .unwrap(), IoContext for wrapping IO errors)
2) Registry layering (`src/git.rs` is the only module that runs git; `registry`
   and `library::drift` are the only callers of `Git` for the library; commands
   go through `Registry`)
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

## Harnesses

Read [docs/harnesses.md](docs/harnesses.md) before changing anything under
`src/shell/` or `src/library/tool_dirs.rs`. It carries the per-harness mount
mechanisms and the constraints each one imposes.

Three places must stay in step when a harness is added or changed:

1. `src/shell/tools.json` + `builtin_tools()` in `src/library/tool_dirs.rs`
2. the staging-dir loop in `src/commands/skills/session_setup.rs` **and** the
   two matching loops in `src/shell/akm-init.sh` — `_akm_skills_session_start`
   (creates them) and `_akm_skills_session_end` (removes them). Teardown
   removes only the dirs it knows about, so a dir missing from the second loop
   is left behind and quarantined as a stray on every session.
3. the `case` in `_akm_wrap_tool` and the exported wrapper functions at the
   bottom of `src/shell/akm-init.sh`

Add a `tests/shell_test.rs` case when changing `_akm_wrap_tool`.

## Tests

```bash
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt
```

Source layout, snapshot handling and the release process: [docs/development.md](docs/development.md).
