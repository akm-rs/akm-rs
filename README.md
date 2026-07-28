# AKM — Agent Kit Manager

[![CI](https://github.com/akm-rs/akm-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/akm-rs/akm-rs/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/akm.svg)](https://crates.io/crates/akm)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

A CLI tool for managing reusable LLM skills, artifacts, and instructions across projects and AI coding agents.

AKM is a **client** of skill registries — it fetches, organizes, and wires skills into your development workflow. The community registry ([Skillverse](https://github.com/akm-rs/skillverse)) is a separate project.

## Supported harnesses

AKM wires the same library of skills, agents and instructions into every harness it supports. Each one exposes a different mounting mechanism, so the shell wrapper adapts per tool:

| Harness | Command | Global dir | Session mount | Artifacts |
|---------|---------|------------|---------------|-----------|
| [Claude Code](https://claude.ai/code) | `claude` | `~/.claude` | `--add-dir <staging>` | symlinked into staging |
| [GitHub Copilot CLI](https://github.com/features/copilot) | `copilot` | `~/.copilot` | `--add-dir <staging>` | symlinked into staging |
| [OpenCode](https://opencode.ai) | `opencode` | `~/.agents` | `OPENCODE_CONFIG_DIR` | symlinked into staging |
| [Pi](https://pi.dev) | `pi` | `~/.pi/agent` | `--skill <staging>/.pi/skills` | named in the system prompt |
| Mistral Vibe | `vibe` | `~/.vibe` | — (no wrapper) | — |

`akm setup` installs shell functions that shadow `claude`, `copilot`, `opencode` and `pi`. Each one builds a per-session staging directory of symlinks into the cold library, hands it to the tool in whatever form that tool understands, and tears it down on exit.

### Pi specifics

Pi is supported as a first-tier harness, with two differences from the others:

- **Skills mount with `--skill`, not `--add-dir`.** Pi has no `--add-dir` flag. `--skill` is repeatable, accepts a directory, and follows symlinked skill directories, so the standard AKM staging tree works unchanged.
- **The artifacts directory is announced, not mounted.** Pi does not sandbox its file tools to the working directory, so the artifacts path is already reachable; the wrapper appends a short note naming it via `--append-system-prompt`. Because a CLI `--append-system-prompt` replaces the `APPEND_SYSTEM.md` Pi would otherwise discover, the wrapper re-passes that file (project `.pi/APPEND_SYSTEM.md`, else `~/.pi/agent/APPEND_SYSTEM.md`) so your own append prompt survives.

Pi has no subagent concept, so AKM `agents` specs are not mounted for it — only skills. `akm instructions sync` writes global instructions to `~/.pi/agent/AGENTS.md`, and `akm skills sync` symlinks core specs into `~/.pi/agent/skills/`.

The tool list lives in `~/.local/share/akm/tools.json` and can be edited to add harnesses without recompiling.

## Installation

### Quick install (Linux x86_64 / macOS ARM)

```sh
curl -fsSL https://akm.raphaelsimon.fr/install | sh
```

This downloads the latest release binary to `~/.local/bin/akm`. The installer auto-detects your platform and downloads the correct binary.

**Supported platforms:**

| Platform | Architecture | Asset |
|----------|-------------|-------|
| Linux | x86_64 | `akm-linux-x86_64` (static, musl) |
| macOS | Apple Silicon (M1+) | `akm-macos-aarch64` |

Options:

```sh
# Install a specific version
AKM_VERSION=1.0.0 curl -fsSL https://akm.raphaelsimon.fr/install | sh

# Install to a custom directory
AKM_INSTALL_DIR=/usr/local/bin curl -fsSL https://akm.raphaelsimon.fr/install | sh
```

Other platforms (Linux ARM, Intel Mac) can install via `cargo install akm`.

### From source

```bash
cargo install akm
```

Or build from the repo:

```bash
git clone https://github.com/akm-rs/akm-rs.git
cd akm-rs
cargo install --path .
```

### Prerequisites

- **git** — the only runtime dependency

## Getting Started

After installation, run the interactive setup:

```bash
akm setup
```

This configures which features to enable (skills, artifacts, instructions), sets up registry remotes, and wires shell integration into your `.bashrc`. The shell integration is what makes `claude`, `copilot`, `opencode` and `pi` session-aware — open a new shell (or `source ~/.bashrc`) once setup finishes.

## Usage

```
akm [COMMAND]

Commands:
  setup         Interactive feature configuration
  config        View, get, or set configuration values
  sync          Sync all enabled domains
  update        Check for and install updates
  skills        Skills management
  artifacts     Artifact sync
  instructions  Global instruction management
  completions   Generate shell completion script
```

### Skills

```bash
akm skills sync                  # pull registries → rebuild library
akm skills list                  # interactive browsable list (TUI)
akm skills list --plain          # plain text output (scripting)
akm skills search "testing"      # search by keyword
akm skills add vitest tdd        # add specs to project manifest
akm skills remove vitest         # remove from project manifest
akm skills load debugging        # load into active session
akm skills unload debugging      # remove from session
akm skills loaded                # show active session specs
akm skills status                # full status dashboard (TUI)
akm skills edit my-skill         # edit metadata in $EDITOR
akm skills promote ./my-skill     # import local skill to cold storage
akm skills import <github-url>   # import skill from a GitHub URL
akm skills publish my-skill      # publish to personal registry
akm skills clean --dry-run       # preview stale spec removal
```

#### Importing skills from GitHub

You can import any skill directory from a GitHub repository:

```bash
# Import from a directory URL
akm skills import https://github.com/user/repo/tree/main/skills/my-skill

# Import with a custom ID
akm skills import https://github.com/user/repo/tree/main/skills/my-skill --id custom-name

# Overwrite without confirmation
akm skills import https://github.com/user/repo/tree/main/skills/my-skill --force
```

Both `/tree/` (directory) and `/blob/` (file) GitHub URLs are supported. For private repos, set the `GITHUB_TOKEN` environment variable.

### Artifacts

```bash
akm artifacts sync               # bidirectional git sync
```

### Instructions

```bash
akm instructions sync            # distribute global instructions to tool dirs
akm instructions edit            # edit global-instructions.md in $EDITOR
akm instructions scaffold-project  # create AGENTS.md + CLAUDE.md in project root
```

`instructions sync` writes `~/.akm/global-instructions.md` out under each tool's expected name: `~/.claude/CLAUDE.md`, `~/.copilot/copilot-instructions.md`, `~/.vibe/prompts/cli.md`, `~/.agents/AGENTS.md` and `~/.pi/agent/AGENTS.md`.

### Configuration

```bash
akm config                       # print all config
akm config skills.enabled        # get a single value
akm config artifacts.auto-push false  # set a value
```

### Self-Update

```bash
akm update                       # download and install latest version
akm update --check               # check without installing
```

### Shell Completions

```bash
akm completions bash >> ~/.bashrc
akm completions zsh  >> ~/.zshrc
akm completions fish > ~/.config/fish/completions/akm.fish
```

## Configuration

Config lives at `~/.config/akm/config.toml` (XDG-compliant). Created by `akm setup` or on first run with defaults.

## Creating a Release

After merging to `main`:

```bash
git tag v1.0.0-alpha.1
git push origin main --tags
```

This triggers the release workflow which:
1. Runs all CI checks (fmt, clippy, test, build, MSRV, installer tests)
2. Builds platform binaries in parallel:
   - Linux x86_64 (static, musl-linked)
   - macOS aarch64 (Apple Silicon)
3. Creates a GitHub Release with all binaries + SHA256 checksums
4. Publishes to crates.io (requires `CARGO_REGISTRY_TOKEN` secret)

## Development

```bash
cargo test                        # run all tests
cargo clippy --all-targets -- -D warnings  # lint
cargo fmt --check                 # format check
cargo build --release             # release build
```

### Project Structure

```
src/
├── main.rs              # Entry point, clap CLI
├── lib.rs               # Library root
├── config.rs            # TOML config
├── paths.rs             # XDG path resolution
├── error.rs             # Error hierarchy (thiserror)
├── git.rs               # Git helper (wraps std::process::Command)
├── github.rs            # GitHub URL parser + Contents API client
├── editor.rs            # $EDITOR invocation
├── commands/            # CLI command implementations
│   ├── config.rs        # akm config
│   ├── setup.rs         # akm setup
│   ├── sync.rs          # akm sync
│   ├── update.rs        # akm update
│   ├── completions.rs   # akm completions
│   ├── artifacts/       # akm artifacts sync
│   ├── instructions/    # akm instructions sync/edit/scaffold-project
│   └── skills/          # akm skills * (sync, list, import, promote, …)
├── library/             # Spec model, libgen, manifest, symlinks, tool dirs
├── registry/            # RegistrySource trait + GitRegistry
├── artifacts/           # Artifact repo sync
├── update/              # Self-update + version check
├── completions/         # clap_complete generation + dynamic completions
├── tui/                 # Interactive views (ratatui)
└── shell/               # Shell init generation
    ├── akm-init.sh      # Session lifecycle + per-harness tool wrappers
    └── tools.json       # Harness definitions (name, command, global dir)
```

### How a session works

1. The wrapper function for a harness (`claude`, `copilot`, `opencode`, `pi`) runs `_akm_session_start`.
2. That creates a staging directory under `$XDG_CACHE_HOME/akm/<repo>-<ts>-<pid>` with one subdirectory per harness.
3. `akm skills session-setup` reads the project manifest (`.agents/akm.json`) and symlinks each declared spec from the cold library into every harness subdirectory.
4. If the artifacts feature is on, the project's artifacts directory is symlinked in at the staging root (and per harness), except for Pi which is given the absolute path instead.
5. The harness is launched with whatever flag or environment variable it uses to pick the staging tree up.
6. On exit the staging directory is removed and artifacts are optionally committed and pushed.

## License

[MIT](LICENSE)
