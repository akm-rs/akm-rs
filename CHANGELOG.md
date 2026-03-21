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
