# Project LLM Instructions

akm-rs is a CLI tool for AKM (Agent Kit Manager).
It's a rewrite of akm, the initial mvp in Bash (https://github.com/akm-rs/akm/) 

It has reached feature parity with the Bash version and now extends beyond it
(e.g. `akm skills import` for GitHub URL imports, and Pi harness support).

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

## Supported harnesses

AKM mounts the same library into several coding harnesses. The definitions live in
`src/shell/tools.json` (embedded at compile time, written to
`$XDG_DATA_HOME/akm/tools.json` by `akm setup`) and are loaded through
`src/library/tool_dirs.rs`.

| Harness | Command | Global dir (`ToolDef::dir`) | Staging dir | Session mount | Artifacts |
|---------|---------|------------------------------|-------------|---------------|-----------|
| Claude Code | `claude` | `~/.claude` | `.claude` | `--add-dir <staging>` | symlink in staging |
| GitHub Copilot CLI | `copilot` | `~/.copilot` | `.copilot` | `--add-dir <staging>` | symlink in staging |
| Mistral Vibe | `vibe` | `~/.vibe` | `.vibe` | none (no `--add-dir`) | none |
| OpenCode | `opencode` | `~/.agents` | `.agents` | `OPENCODE_CONFIG_DIR` | symlink in staging |
| Pi | `pi` | `~/.pi/agent` | `.pi` | `--skill <staging>/.pi/skills` | `--append-system-prompt` |

Three places must stay in step when a harness is added or changed:

1. `src/shell/tools.json` + `builtin_tools()` in `src/library/tool_dirs.rs`
2. the staging-dir loop in `src/commands/skills/session_setup.rs` **and** the
   matching loop in `src/shell/akm-init.sh` (`_akm_skills_session_start`)
3. the `case` in `_akm_wrap_tool` and the exported wrapper functions at the
   bottom of `src/shell/akm-init.sh`

Note that `ToolDef::dir` is not always a single path component. Pi's global dir
is `~/.pi/agent` but its staging dir is `.pi`, so anything writing into the
staging tree must use `ToolDef::staging_dir()` / `ToolDirs::staging_names()`
rather than the last component of `ToolDirs::dirs()`.

### Pi

Verified against pi `0.82.1` (`@earendil-works/pi-coding-agent`); docs at
<https://pi.dev/docs/latest>.

- **No `--add-dir`, and none needed.** Pi does not sandbox `read`/`write`/`edit`/
  `bash` to the working directory, so an out-of-tree path such as the artifacts
  directory is already reachable. It only has to be named, which the wrapper does
  with `--append-system-prompt`.
- **`--append-system-prompt` replaces discovery.** Passing it on the CLI
  suppresses the `APPEND_SYSTEM.md` Pi would otherwise load (project
  `.pi/APPEND_SYSTEM.md` if trusted, else `<agent dir>/APPEND_SYSTEM.md`). The
  wrapper re-passes that file first so it is not silently lost. The flag takes
  literal text *or* a path to an existing file.
- **`--skill` follows symlinks.** Pi `statSync`s symlinked entries when walking a
  skills directory, so the AKM staging tree of symlinks works as-is.
- **No subagents.** AKM `agents` specs have no target in Pi; only skills are
  mounted. `<staging>/.pi/agents` is created for layout uniformity and unused.
- **Config dir override.** `PI_CODING_AGENT_DIR` relocates `~/.pi/agent`, but
  `auth.json`, `models-store.json` and `sessions/` live there too — do not hijack
  it the way `OPENCODE_CONFIG_DIR` is hijacked for OpenCode.

## Supported platforms

Release binaries are built for:
- Linux x86_64 (static, musl-linked — no libc dependency)
- macOS aarch64 (Apple Silicon)

The self-update system (`akm update`) and install script both detect the current platform at runtime and select the correct asset. Platform-specific logic lives in:
- `src/update/mod.rs` — `platform_asset_name()` (compile-time asset selection)
- `src/update/download.rs` — `validate_binary()` (ELF + Mach-O magic byte checks)
- `scripts/install.sh` — `detect_platform()` (runtime OS/arch detection)

## Test commands

- `cargo test` — unit + integration tests (`tests/`), including insta snapshots
- `cargo clippy --all-targets -- -D warnings`
- `cargo fmt`

Snapshots live in `tests/snapshots/`. `cargo-insta` is not assumed to be
installed; `INSTA_FORCE_UPDATE=1 cargo test --test <name>` rewrites them, and the
diff should be reviewed before committing.

`tests/shell_test.rs` drives `akm-init.sh` for real: it stubs `akm` and the
harness binary on `PATH`, sources the script inside a temporary git repo, and
asserts the argv the wrapper builds. Add a case there when changing
`_akm_wrap_tool`.
