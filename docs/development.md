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
├── library/             # Spec model, sidecars, libgen, drift, local overrides,
│                        #   manifest, symlinks, tool dirs
├── registry/            # The personal registry, checked out as the library
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

## The library is a git working tree

`$XDG_DATA_HOME/akm/library/` is the personal registry's checkout, not a copy of
one. Everything the sync model needs follows from that:

| need | implementation |
|---|---|
| local drift | `git status --porcelain` |
| remote drift | `git diff --name-only HEAD @{upstream}` |
| sync | `git fetch` + `git merge --ff-only`, parking blocking edits and putting them back |
| publish | `git add -- <spec paths> && git commit && git push` |
| revert | `git restore <paths>`, or `git checkout @{upstream} -- <paths>` |

Layering, in one direction only:

* `src/git.rs` is the only module that executes git;
* `src/registry/` and `src/library/drift.rs` are the only modules that call
  `Git` for the library;
* commands go through `Registry`, so no command has to know what a
  fast-forward is.

Two files are not what they look like:

* **`library.json` is derived *and* machine-local.** libgen regenerates it from
  the specs on disk and their `akm.json` sidecars on every sync, and
  `LocalOverrides::apply` then folds this machine's `core` deviations into it.
  Anything written to it directly is erased on the next sync — human metadata
  belongs in the spec's sidecar. Because it is machine-local it lives *outside*
  the working tree, at `$XDG_DATA_HOME/akm/library.json`, beside `local.json`
  and `tools.json`: nothing in the checkout can then commit it, whatever a
  future code path stages.
* **`local.json` is machine-local.** It holds only the `core` flags that
  deviate from the registry's defaults, so a newly published core skill still
  propagates while a local toggle stays put. It lives *outside* the working
  tree, beside `tools.json` and `shell/`.

Registries seeded while the index was still committed carry a `library.json` in
their history.
Sync restores that copy to `HEAD` and leaves it there: nothing writes it any
more, so it stays clean and inert. Removing it for good is one manual
`git rm --cached library.json` in the registry — the registry's owner's call,
not a session-start side effect.

## Tests

```bash
cargo test                                 # unit + integration, including snapshots
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

Snapshots live in `tests/snapshots/`. `cargo-insta` is not assumed to be
installed; `INSTA_FORCE_UPDATE=1 cargo test --test <name>` rewrites them, and the
diff should be reviewed before committing.

Tests that build a fixture library must write to `<data>/akm/library/…`. The
git-backed suites (`git`, `drift`, `skills_sync`, `skills_publish`,
`skills_drift_commands`, `tui_drift_actions`, `instructions`) each carry their
own small repo fixture; integration test files cannot share a module without a `mod common`,
and the duplication is cheaper than adding one.

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
