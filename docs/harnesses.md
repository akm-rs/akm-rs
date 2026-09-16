# Harness wiring

How AKM mounts the same library of skills, agents and instructions into each
supported harness. Definitions live in `src/shell/tools.json` (embedded at
compile time, written to `$XDG_DATA_HOME/akm/tools.json` by `akm setup`) and are
loaded through `src/library/tool_dirs.rs`. Each entry also carries a `mount`
kind (`symlink`, the default, or `tree`) and an optional `project_dir`.
Mount operations read them through `ToolDirs::mounts()`.

| Harness | Command | Global dir (`ToolDef::dir`) | Staging dir | Session mount | Artifacts |
|---------|---------|------------------------------|-------------|---------------|-----------|
| Claude Code | `claude` | `~/.claude` | `.claude` | `--add-dir <staging>` | symlink + `--append-system-prompt` |
| GitHub Copilot CLI | `copilot` | `~/.copilot` | `.copilot` | `--add-dir <staging>` | symlink in staging |
| Mistral Vibe | `vibe` | `~/.vibe` | `.vibe` | none (no `--add-dir`) | none |
| OpenCode | `opencode` | `~/.agents` | `.agents` | `OPENCODE_CONFIG_DIR` | symlink in staging |
| Pi | `pi` | `~/.pi/agent` | `.pi` | `--skill <staging>/.pi/skills` | `--append-system-prompt` |
| Posit Assistant | `pa` | `~/.posit/assistant` | — | — (sidecar) | — |

## The staging dir is ephemeral, the artifacts dir is not

Harnesses that take `--add-dir` are handed the staging dir, which is destroyed
at session end. Agents are told to put non-code outputs in "the additional
working directory" — so left to itself that instruction points them at a
directory that will be deleted under them. That cost a user ~700 lines of
research output (issue #23). Four things guard it, in order of when they fire:

1. **Named.** Harnesses with a system-prompt flag (`claude`, `pi`) get the
   artifacts dir by resolved absolute path via `_akm_artifacts_prompt`. A
   literal path is far more reliable than a rule for deriving one, and the
   symlink only makes the durable path *reachable*, never *obvious*.
2. **Signposted.** `<staging>/README.md` says the same thing, for the harnesses
   with no system-prompt flag (`copilot`, `opencode`).
3. **Sealed.** The staging *root* is `chmod a-w` after setup, so a stray write
   fails with EACCES the agent sees as a tool error and can correct, instead of
   succeeding and vanishing later. Tool subdirs stay writable — `opencode`
   keeps state in the one `OPENCODE_CONFIG_DIR` points at.
4. **Rescued.** Teardown removes only the dirs it created, then `rmdir`s the
   parent. If that fails, something unexpected is inside: it moves to
   `<artifacts>/<repo>/orphaned/<session_id>/` (git-synced, so it is committed
   on the way out) and the next session's system prompt tells the agent to
   triage it. The agent that wrote the files is gone by then, so the next one
   is the first that can act.

There is deliberately no watcher process. Nothing outside a live agent can
reach its context — terminal output is not something the model sees, only tool
results are — so the filesystem's own EACCES is the only in-band, harness-
agnostic write-time signal available. `inotify` would also breach the
no-runtime-dependencies rule.

## Global dir vs staging dir

`ToolDef::dir` is not always a single path component — Pi's global dir is
`~/.pi/agent` while its staging dir is `.pi`. Anything writing into the staging
tree must use `ToolDef::staging_dir()` / `ToolDirs::staging_names()` rather than
the last component of `ToolDirs::dirs()`.

## Pi

Pi CLI surface as of `0.82.1` (`@earendil-works/pi-coding-agent`); docs at
<https://pi.dev/docs/latest>.

- **No `--add-dir`, and none needed.** Pi does not sandbox `read`/`write`/`edit`/
  `bash` to the working directory, so an out-of-tree path such as the artifacts
  directory is already reachable. It only has to be named.
- **`--append-system-prompt` replaces discovery.** Passing it on the CLI
  suppresses the `APPEND_SYSTEM.md` Pi would otherwise load (project
  `.pi/APPEND_SYSTEM.md` if trusted, else `<agent dir>/APPEND_SYSTEM.md`), so the
  wrapper re-passes that file first. The flag takes literal text *or* a path to
  an existing file.
- **`--skill` follows symlinks.** Pi `statSync`s symlinked entries when walking a
  skills directory, so the staging tree of symlinks works as-is.
- **Subcommands must be argv[1].** `pi install|remove|uninstall|update|list|
  config` are only recognised as the first argument; anything injected ahead of
  them is parsed as options and the subcommand becomes the opening chat message.
  `_akm_wrap_tool` passes those invocations straight through to `command pi` —
  they start no agent, so there is nothing to mount anyway.
- **No subagents.** AKM `agents` specs have no target in Pi; only skills are
  mounted. `<staging>/.pi/agents` is created for layout uniformity and unused.
- **Do not hijack `PI_CODING_AGENT_DIR`.** It relocates `~/.pi/agent`, but
  `auth.json`, `models-store.json` and `sessions/` live there too — unlike
  `OPENCODE_CONFIG_DIR`, it is not free to repoint at a staging directory.

## Posit Assistant

Posit Assistant is built into the Positron IDE; config dir `~/.posit/assistant/`.

- **No wrapper, no session.** Posit runs inside the IDE, so there is no `pa`
  command to shadow, no staging dir and no `SessionStart` hook. `pa` in
  `tools.json` names the harness only. The staging loops in
  `session_setup.rs` and `akm-init.sh` are deliberately not extended for it.
- **Tree mount, not symlink.** Posit's skill discovery skips symlinked
  directories (verified against the installed bundle and its
  `extension-host.log`), so core skills are mounted as real directories at
  `~/.posit/assistant/skills/<id>/` whose entries (`SKILL.md`, `references/`,
  …) are symlinks into the library — `"mount": "tree"` in `tools.json`, where
  every other harness uses the `symlink` default. Agents are not mounted;
  Posit has no custom agent files. AKM only recognises a tree as its own when
  every entry inside it is a symlink into the library, so a hand-authored
  skill dir sitting in the same folder is never touched by `akm skills
  clean`, `akm skills sync`, `akm disable` or `akm uninstall`.
- **`skills.paths` replaces, it does not extend.** Posit's `skills.paths`
  setting replaces its defaults rather than adding to them, so AKM does not
  edit `settings.json` and instead relies on the default roots
  (`~/.agents/skills`, `~/.posit/assistant/skills`, project `.agents/skills`,
  `.positai/skills`, `.posit/assistant/skills`). `~/.agents/skills` already
  holds AKM's OpenCode symlinks; Posit silently ignores them.
- **Instructions via include line.** `akm instructions sync` writes
  `~/.posit/assistant/akm-instructions.md` and ensures
  `~/.posit/assistant/AGENTS.md` contains one `@akm-instructions.md` line,
  appended once and never overwriting the file — Posit's own `/savememory`
  command appends memories to `AGENTS.md`.
- **Project sidecar.** Project skills declared in `.agents/akm.json` are
  materialized as tree mounts into `<project>/.posit/assistant/skills/<id>/`,
  with a `.gitignore` containing `*` inside `skills/` so the sidecar is
  invisible to git (only `.agents/akm.json` is tracked). Refreshed by `akm
  skills add`/`remove`, the interactive TUI, `skills delete`/`rename`, and at
  session setup. Empty sidecar dirs
  are removed with `rmdir` (never recursively), so a user's own
  `.posit/assistant/settings.json` survives. `clean --project` looks at
  `<root>/assistant/` — the last dir component — not the sidecar path itself.
- **Discovery is per conversation.** Posit scans skills when a conversation
  starts, not live. After `akm skills sync` or `akm skills add`, reload the
  Positron window (or start a new conversation) to see new skills.
