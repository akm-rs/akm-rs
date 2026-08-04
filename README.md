# AKM — Agent Kit Manager

[![CI](https://github.com/akm-rs/akm-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/akm-rs/akm-rs/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/akm.svg)](https://crates.io/crates/akm)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

A CLI tool for managing reusable LLM skills, artifacts, and instructions across projects and AI coding agents.

AKM is a **client** of a skill registry — it fetches, organizes, and wires skills into your development workflow. The registry itself is a separate git repository you own, configured via `registry.url`.

That repository is checked out as your library rather than copied out of it, so a skill you edit locally survives the next sync, AKM can tell you which side has moved, and publishing one skill is one commit.

## Supported harnesses

AKM wires the same library of skills, agents and instructions into every harness it supports. Each one exposes a different mounting mechanism, so the shell wrapper adapts per tool:

| Harness | Command | Global dir | Session mount | Artifacts |
|---------|---------|------------|---------------|-----------|
| [Claude Code](https://claude.ai/code) | `claude` | `~/.claude` | `--add-dir <staging>` | symlinked into staging, named in the system prompt |
| [GitHub Copilot CLI](https://github.com/features/copilot) | `copilot` | `~/.copilot` | `--add-dir <staging>` | symlinked into staging |
| [OpenCode](https://opencode.ai) | `opencode` | `~/.agents` | `OPENCODE_CONFIG_DIR` | symlinked into staging |
| [Pi](https://pi.dev) | `pi` | `~/.pi/agent` | `--skill <staging>/.pi/skills` | named in the system prompt |
| Mistral Vibe | `vibe` | `~/.vibe` | — (no wrapper) | — |

`akm setup` installs shell functions that shadow `claude`, `copilot`, `opencode` and `pi`. Each one builds a per-session staging directory of symlinks into the cold library, hands it to the tool in whatever form that tool understands, and tears it down on exit.

### Pi

Pi has no `--add-dir`. Session skills mount with `--skill`, and the artifacts directory — which Pi's file tools can already reach — is named in the system prompt with `--append-system-prompt`. Your own `.pi/APPEND_SYSTEM.md` (project, else `~/.pi/agent/APPEND_SYSTEM.md`) is passed alongside it.

Pi has no subagents, so only skills are mounted for it.

The tool list lives in `~/.local/share/akm/tools.json` and can be edited to add harnesses without recompiling.

## Installation

### Quick install (Linux x86_64 / macOS ARM)

```sh
curl -fsSL https://akm.raphaelsimon.fr/install | sh
```

Installs the latest release binary to `~/.local/bin/akm`, detecting the platform:

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
  disable       Turn off shell integration so harnesses run vanilla
  enable        Re-enable shell integration after akm disable
  uninstall     Remove akm from this machine
```

### Skills

```bash
akm skills sync                  # fast-forward the registry → rebuild library
akm skills list                  # interactive browsable list (TUI)
akm skills list --plain          # plain text output (scripting)
akm skills search "testing"      # search by keyword
akm skills add vitest tdd        # add specs to project manifest
akm skills remove vitest         # remove from project manifest
akm skills status                # full status dashboard (TUI)
akm skills edit my-skill         # edit SKILL.md in $EDITOR
akm skills edit my-skill --meta  # edit the skill's akm.json sidecar
akm skills diff my-skill         # what changed here, and what changed on the registry
akm skills revert my-skill       # discard local changes (--remote: take the registry's copy)
akm skills core                  # show core flags (--adopt / --publish to reconcile)
akm skills promote ./my-skill     # import local skill to cold storage
akm skills import <github-url>   # import skill from a GitHub URL
akm skills list acme             # browse a shared registry
akm skills import acme tdd       # take one skill from it (--all for everything)
akm skills share acme my-skill   # offer one of yours back, as a pull request
akm skills publish my-skill      # publish one spec to personal registry
akm skills publish               # publish everything pending, in one commit
akm skills clean --dry-run       # preview stale spec removal
```

#### Keeping in step with the registry

Your library is the registry's git working tree, so AKM answers "who is newer"
from git rather than from bookkeeping of its own. `akm skills sync` fetches and
fast-forwards; an uncommitted edit of yours to a skill the update does not touch
survives untouched, and one it *does* touch is set aside and put back on top.

Sync never merges and never prompts — it reports, and you decide:

```
Diverged from the registry (1):
  grill-me
  Review with 'akm skills diff <id>', then publish or revert.

Not yet published (2):
  my-skill
  tdd
  Publish with 'akm skills publish <id>'.
```

One intent is one commit. `akm skills publish <id>` sends a single spec;
`akm skills publish` with no id sends everything pending as one commit and one
push, however many specs that is. Add `--dry-run` to either to see what would
go without touching the remote.

The same states show as markers in `akm skills list` and `akm skills status`:
`*` edited here and unpublished, `v` the registry is ahead, `!` both moved.

Each skill carries its human-facing metadata in an `akm.json` sidecar beside its
`SKILL.md`, so two machines editing two different skills never collide.
`library.json` is a derived, machine-local index kept outside the library
checkout, regenerated on every sync — edit the sidecar (or
`akm skills edit --meta`), never the index. `core` defaults live in the
sidecar and propagate; a local `c` toggle in the TUI stays on this machine, and
`akm skills core --publish` promotes it for every machine — committing and
pushing every promoted sidecar together, and nothing else, so a `SKILL.md` you
have open stays behind.

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

`--all` takes every skill a URL offers instead of one:

```bash
akm skills import https://github.com/user/repo/tree/main/skills --all
```

#### Shared registries

A shared registry is somebody else's skills repository — a team's, a
colleague's, a community's. AKM treats it as a trove to pick from, not a second
library: nothing in it is ever mounted into a tool directory. You browse it,
take what you want, and the copy becomes an ordinary skill of your own.

```bash
akm config shared.acme git@github.com:acme/skills.git   # add one
akm config shared.acme ""                                # remove it

akm skills list acme              # what it offers, marking what you already have
akm skills import acme tdd        # take one
akm skills import acme --all      # take everything usable
```

Checkouts live in `$XDG_CACHE_HOME/akm/shared/<name>` and are refreshed by
`akm skills sync` and by any of the commands above. A registry that cannot be
reached is reported, and browsing falls back to the copy on disk.

Only directories holding a `SKILL.md` with a `name` and a `description` are
importable; anything else is named and skipped. An id you already have is a
conflict, and interactively you choose per skill:

```
  tdd — already in your library, and it differs.
    [m]ine  [t]heirs  [b]oth as 'acme-tdd'  (add 'a' for all):
```

`both` keeps yours and stores theirs under the prefixed id. Without a terminal,
`--all` keeps yours and says so, while a single import fails unless you pass
`--force`. Identical content is skipped silently, so re-running an import is a
no-op until the registry actually moves.

A registry's `core: true` is never inherited — that flag is its owner's
statement about their machines. Imported skills land unmounted, and making one
core here is `akm skills edit <id> --meta`.

#### Contributing back

```bash
akm skills share acme my-skill
```

This pushes the spec to the shared registry on branch `akm/<id>` and relays the
URL the remote prints for opening a pull request. Nothing is merged — the
registry's owners decide. It needs permission to push a branch, not to write to
the default branch, and re-running it updates the same branch so an open pull
request follows along.

`publish` and `share` are not the same act: `publish` writes to *your* registry
on your own authority, while `share` offers a copy to someone else's and waits
for their review.

#### Publishing after promote or import

When run interactively, `promote` and `import` end by offering to publish the
skill to your personal registry:

```
Publish to personal registry? [y/N]:
```

The prompt is skipped when `registry.url` is unset or when stdin is
not a terminal. The description and tags you entered at the metadata prompts are
carried into the registry. If publishing fails, the skill still stays in cold
storage — retry with `akm skills publish <id>`.

### Artifacts

```bash
akm artifacts sync               # bidirectional git sync
```

### Instructions

```bash
akm instructions sync            # distribute global instructions to tool dirs
akm instructions edit            # edit global instructions in $EDITOR
akm instructions publish         # push them to your personal registry
akm instructions scaffold-project  # create AGENTS.md + CLAUDE.md in project root
```

Global instructions live in the registry, at `instructions/global.md`, so they
propagate between your machines through the same clone, drift and publish flow
as a skill. `instructions sync` writes that file out under each tool's expected
name: `~/.claude/CLAUDE.md`, `~/.copilot/copilot-instructions.md`,
`~/.vibe/prompts/cli.md`, `~/.agents/AGENTS.md` and `~/.pi/agent/AGENTS.md`.

A pre-rc4 `~/.akm/global-instructions.md` is carried into the registry the first
time the new file is needed; the old file is left where it is.

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

`akm update` also rewrites `akm-init.sh` from the new binary, so shell-side
changes land without a second `akm setup`. Restart your shell to pick them up.

### Disabling and Uninstalling

```bash
akm disable            # new shells get vanilla claude/copilot/opencode/pi; nothing deleted
akm enable             # restore wrappers and global core symlinks
akm uninstall          # remove akm — preserves artifacts and the library checkout
akm uninstall --purge  # remove everything, including artifacts and the library
```

`akm disable` is the way to check default harness behavior without akm: it
removes the global core-spec symlinks and makes new shells skip the tool
wrappers, while leaving config, library, and manifests intact. For a one-off
bypass in the current shell, `command claude` runs the real binary without
the wrapper (the wrappers are shell functions, and `command` skips function
lookup).

`akm uninstall` preserves `~/.akm/artifacts` and the library — the checkout
of your personal registry, which may hold unpublished local edits. Published
content is recoverable by re-cloning; anything marked `*` or `!` in
`akm skills status` is not. `--purge` deletes both, unpublished changes
included.

Instruction files previously distributed to tool dirs (e.g. `~/.claude/CLAUDE.md`)
are never deleted by any of these commands — they may contain your own edits.

### Shell Completions

```bash
akm completions bash >> ~/.bashrc
akm completions zsh  >> ~/.zshrc
akm completions fish > ~/.config/fish/completions/akm.fish
```

## Configuration

Config lives at `~/.config/akm/config.toml` (XDG-compliant). Created by `akm setup` or on first run with defaults.

```toml
features = ["skills", "artifacts", "instructions"]

[registry]
url = "git@github.com:you/your-registry.git"

[shared]
acme = "git@github.com:acme/skills.git"
```

`skills.personal_registry` is the pre-rc4 spelling of `registry.url` and still
works; the canonical key wins when both are set.

`[shared]` holds read-only registries to import from, one line per registry,
named however you like. Set them with `akm config shared.<name> <git-url>`.

On disk:

```
~/.local/share/akm/
  library/          the registry's git working tree — skills, agents, instructions
  library.json      derived index of the specs above, rebuilt on every sync
  local.json        this machine's core deviations
  tools.json        harness definitions
  shell/            generated shell init

~/.cache/akm/
  shared/<name>/    checkout of a shared registry — browsable, never mounted
```

## How a session works

1. The wrapper function for a harness (`claude`, `copilot`, `opencode`, `pi`) creates a staging directory under `$XDG_CACHE_HOME/akm/<repo>-<ts>-<pid>`, with one subdirectory per harness.
2. `akm skills session-setup` reads the project manifest (`.agents/akm.json`) and symlinks each declared spec from the cold library into every harness subdirectory.
3. If the artifacts feature is on, the project's artifacts directory is symlinked in at the staging root and per harness — except for Pi, which is given the absolute path instead.
4. A `README.md` naming the artifacts directory is written at the staging root, and the root is made read-only. The staging tree is deleted on exit, so a write landing there would be lost; the harness sees a permission error instead.
5. The harness is launched with whatever flag or environment variable it uses to pick the staging tree up. Harnesses that accept a system prompt (`claude`, `pi`) are also told the artifacts path outright.
6. On exit the staging directory is removed and artifacts are optionally committed and pushed. Anything AKM did not create is moved to `<artifacts>/<repo>/orphaned/<session>/` rather than deleted, and the next session tells its agent to triage it.

## Development

```bash
cargo test                        # run all tests
cargo clippy --all-targets -- -D warnings  # lint
cargo fmt --check                 # format check
cargo build --release             # release build
```

See [docs/development.md](docs/development.md) for the source layout and the release process, and [docs/harnesses.md](docs/harnesses.md) for how harnesses are wired.

## License

[MIT](LICENSE)
