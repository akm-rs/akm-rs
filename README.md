# AKM — Agent Kit Manager

[![CI](https://github.com/akm-rs/akm-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/akm-rs/akm-rs/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/akm.svg)](https://crates.io/crates/akm)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

A CLI tool for managing reusable LLM skills, artifacts, and instructions across projects and AI coding agents.

AKM is a **client** of skill registries — it fetches, organizes, and wires skills into your development workflow. The community registry ([Skillverse](https://github.com/akm-rs/skillverse)) is a separate project.

## Installation

### Quick install (Linux x86_64)

```sh
curl -fsSL https://akm.raphaelsimon.fr/install | sh
```

This downloads the latest release binary to `~/.local/bin/akm`.

Options:

```sh
# Install a specific version
AKM_VERSION=1.0.0 curl -fsSL https://akm.raphaelsimon.fr/install | sh

# Install to a custom directory
AKM_INSTALL_DIR=/usr/local/bin curl -fsSL https://akm.raphaelsimon.fr/install | sh
```

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

This configures which features to enable (skills, artifacts, instructions), sets up registry remotes, and wires shell integration into your `.bashrc`.

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
1. Runs all CI checks (fmt, clippy, test, build, MSRV)
2. Builds a static Linux x86_64 binary (musl)
3. Creates a GitHub Release with the binary + SHA256 checksum
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
├── config.rs            # TOML config, XDG paths
├── error.rs             # Error hierarchy (thiserror)
├── git.rs               # Git helper (wraps std::process::Command)
├── paths.rs             # XDG path resolution
├── lib.rs               # Library root
├── github.rs            # GitHub URL parser + Contents API client
├── commands/            # CLI command implementations
│   ├── config.rs        # akm config
│   ├── setup.rs         # akm setup
│   ├── sync.rs          # akm sync
│   └── skills/          # akm skills * (sync, list, import, promote, …)
├── library/             # Spec model, libgen, manifest
├── registry/            # RegistrySource trait + GitRegistry
├── artifacts/           # Artifact sync
├── instructions/        # Instructions sync/edit/scaffold
├── update/              # Self-update + version check
├── tui/                 # Interactive views (ratatui)
└── shell/               # Shell init generation + completions
```

## License

[MIT](LICENSE)
