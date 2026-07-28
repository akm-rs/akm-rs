# Development

## Source layout

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
├── library/             # Spec model, libgen, manifest, symlinks, tool dirs
├── registry/            # RegistrySource trait + GitRegistry
├── artifacts/           # Artifact repo sync
├── update/              # Self-update + version check
├── completions/         # Completion registration + dynamic completions
├── tui/                 # Interactive views (ratatui)
└── shell/               # Shell init generation
    ├── akm-init.sh      # Session lifecycle + per-harness tool wrappers
    └── tools.json       # Harness definitions (name, command, global dir)
```

See [harnesses.md](harnesses.md) for how `shell/` and `library/tool_dirs.rs`
fit together.

## Tests

```bash
cargo test                                 # unit + integration, including snapshots
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

Snapshots live in `tests/snapshots/`. `cargo-insta` is not assumed to be
installed; `INSTA_FORCE_UPDATE=1 cargo test --test <name>` rewrites them, and the
diff should be reviewed before committing.

`tests/shell_test.rs` drives `akm-init.sh` for real: it stubs `akm` and the
harness binary on `PATH`, sources the script inside a temporary git repo, and
asserts the argv the wrapper builds.

## Releasing

After merging to `main`, tag and push:

```bash
git tag vX.Y.Z
git push origin main --tags
```

The release workflow then:

1. Runs CI (fmt, clippy, test, build, MSRV, installer tests)
2. Builds Linux x86_64 (static, musl-linked) and macOS aarch64 binaries in parallel
3. Creates a GitHub Release with the binaries and SHA256 checksums
4. Publishes to crates.io (requires the `CARGO_REGISTRY_TOKEN` secret)

Platform selection lives in `src/update/mod.rs` (`platform_asset_name()`),
`src/update/download.rs` (`validate_binary()`) and `scripts/install.sh`
(`detect_platform()`).
