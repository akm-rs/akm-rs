# 1.0.0-rc5

- Publishing is batched: one intent is one commit
  - `akm skills core --publish` now promotes *and* pushes, in a single commit,
    staging only sidecars so an in-flight `SKILL.md` is never swept in
  - `akm skills publish` with no id publishes every pending spec at once
  - `--dry-run` on both
- Fix a commit whose push failed never being retried — publish now pushes
  commits already on `HEAD` instead of reporting nothing to do
- Fix the cursor jumping in `akm skills status` when a spec changes section:
  `c`, `a` and `r` now hold the cursor on its screen line and move it to the
  neighbour, instead of scrolling the window to wherever the spec landed
- Sync evicts a stale `library.json` left inside the registry checkout by an
  earlier layout — restored to `HEAD` when tracked, deleted when it is the
  untracked copy rc4 hid with `.git/info/exclude`

# 1.0.0-rc4

**Breaking: the cold library is wiped and re-cloned on first sync.** The
library is now the personal registry's git working tree at
`~/.local/share/akm/library/`, not a copy of one. The first `akm sync` after
upgrading removes the old `~/.local/share/akm/{skills,agents,library.json}` and
the registry cache, then clones fresh. **Anything you never published to your
registry is lost** — publish it before upgrading. `tools.json`, `shell/` and
your config are untouched.

- Sync no longer destroys local edits
  - `git fetch` + `merge --ff-only`, never a real merge, so no conflict marker
    can ever reach a skill that is symlinked live into `~/.claude/skills/`
  - An edit to a skill the update also changed is parked, the fast-forward is
    applied to everything else, and the edit is put back on top — one
    unresolved skill can no longer freeze the other forty-nine
  - Sync reports what needs a decision and never prompts; only
    `akm skills edit` asks anything
- Know which side moved: `*` unpublished, `v` registry ahead, `!` diverged,
  shown per spec in `akm skills list`, `akm skills status` and the sync report
- New commands: `akm skills diff <id>`, `akm skills revert <id> [--remote]`,
  `akm skills core [--adopt|--publish]`, `akm instructions publish`
- `akm skills edit <id>` now opens `SKILL.md`; `--meta` opens the spec's
  metadata sidecar. Both offer to publish when the spec has local changes
- Human-facing metadata moves to a per-spec `akm.json` sidecar
  - `library.json` becomes a derived, locally-excluded index, so it can never
    be the thing that conflicts
  - `core` defaults live in the sidecar and propagate to new machines;
    deviations stay in `~/.local/share/akm/local.json`, machine-local
- Global instructions move into the registry at `instructions/global.md`, and
  gain the same drift, publish and propagation model as a skill. A pre-rc4
  `~/.akm/global-instructions.md` is carried over on first sync
- `registry.url` is the canonical config key; `skills.personal_registry` still
  resolves silently
- The TUI metadata editor gains a real cursor, wrapped multi-line fields and
  Left/Right/Home/End/Delete, and its edits now survive the next sync — they
  previously went to the derived index and were erased

# 1.0.0-rc2

- Fix the session staging directory silently eating agent output
  - Agents told to write to "the additional working directory" wrote into the
    staging dir, which is `rm -rf`'d on session end — the files were lost
  - The artifacts directory is now named by resolved absolute path in the
    system prompt (claude and pi), signposted by a `README.md` at the staging
    root for harnesses with no system-prompt flag, and the staging root is
    made read-only so a stray write fails loudly instead of vanishing later
  - Teardown now removes only the directories it created and rescues anything
    left to `<artifacts>/<repo>/orphaned/<session>/` for the next session to triage
  - `akm update` now rewrites `akm-init.sh` as well as the binary — previously
    an updated binary kept running a months-old shell init
- Offer to publish to the personal registry after an interactive `skills promote`
  or `skills import`
  - Skipped when no personal registry is configured or stdin is not a TTY, so
    non-interactive output and exit codes are unchanged
  - Fix `skills publish` dropping the description and tags entered at the
    promote/import prompts, which left the registry — the source of truth for
    sync and search — carrying the values derived from `SKILL.md` frontmatter
- Make the interactive skills list modal so actions work on a filtered list
  - Previously typing a filter disabled every action key, and the only way out
    cleared the filter along with it
  - `/` starts editing the filter; `Enter` or `Esc` returns to normal mode with
    the filter still applied, where `c`/`e`/`a`/`r` act on the filtered rows
  - `Esc` no longer quits either interactive view — only `q` and `Ctrl+C` do
  - The status dashboard gains `e` (edit) and now matches the list view's keys
- Prune stale narrative from the docs and split maintainer material out of
  README and AGENTS.md into `docs/development.md` and `docs/harnesses.md`

# 1.0.0-rc1

- Add Pi (https://pi.dev) as a first-tier harness
  - `pi` shell wrapper: mounts session skills with `--skill <staging>/.pi/skills`
    (Pi has no `--add-dir`) and names the artifacts directory via
    `--append-system-prompt`, re-passing any `APPEND_SYSTEM.md` the CLI flag
    would otherwise suppress
  - `akm skills sync` symlinks core specs into `~/.pi/agent/skills/`
  - `akm instructions sync` writes `~/.pi/agent/AGENTS.md`
  - Fix session symlink helpers deriving the staging directory name from the
    last component of the global tool dir — wrong for `~/.pi/agent`
  - `tests/shell_test.rs` now exercises the generated wrapper for real
- Remove all references to the retired Bash implementation from source comments
  and documentation

# alpha.11

- Add macOS Apple Silicon (aarch64) binary to releases
  - Release workflow now builds Linux x86_64 and macOS aarch64 in parallel
  - Install script (`install.sh`) supports macOS: platform detection, `shasum` checksum verification, Gatekeeper quarantine removal
  - `akm update` is now platform-aware — picks the correct binary for the current OS/arch
  - Binary validation accepts Mach-O format alongside ELF

# alpha.10

- Fix `akm update` always saying "Already up to date" — the explicit update
  command was trusting a stale cache instead of making a fresh API call

# alpha.9

- Add `akm skills import` — import skills directly from GitHub URLs
  - Supports `/tree/` (directory) and `/blob/` (file) URL formats
  - GITHUB_TOKEN support for private repos and higher rate limits
  - Interactive prompts for metadata (description, tags, core flag)
  - `--force` to skip overwrite confirmation, `--id` to set custom skill ID
  - Source URL persisted in library.json for future update support

# alpha.7

Add a script for automated release checklist issue
Fix version comparison in a`akm update` 

# alpha.6 

Fix `akm skills status` to correctly regenerate symlinks 

# alpha.5

Breaking fix: akm update was broken due to a misconfigured URL. To fix:
  akm config update.url https://api.github.com/repos/akm-rs/akm-rs/releases/latest
  Then akm update works normally. Alternatively, re-run the install script.
  Future installs are unaffected — this release auto-migrates the bad URL on startup.

# alpha.4

- Fix `akm sync` overwriting the changes to core/non-core in skills in the cold storage. Now the cold storage has priority.
- Improved messages for akm update fails due to rate limit

# alpha.3 

Fix : akm instructions no longer replaces existing global instructions with empty new ones.
