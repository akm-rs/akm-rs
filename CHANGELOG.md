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
