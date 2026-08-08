---
name: zed-local
description: Build, run, and manage local Zed instances on Windows - release-fast builds, independent parallel instances from git worktrees with isolated data dirs, and app-icon recoloring to tell builds apart. Use when asked to build or run local Zed, create a worktree for parallel feature work, or change the local build's icon color.
---

# Local Zed development on Windows

## Key facts (verified against this repo)

- **Fast build**: `cargo build --profile release-fast --package zed` → `target\release-fast\zed.exe`. The `release-fast` profile (root `Cargo.toml`) is optimized like release but links fast: `lto = false`, `codegen-units = 16`, `debug = "full"`. This is the sanctioned "doesn't lag, builds quickly" profile; `.zed/tasks.json` has a matching task.
- **Never set a global `RUSTFLAGS` env var** — it replaces the required flags from `.cargo/config.toml` (`-C target-feature=+crt-static`, `--cfg windows_slim_errors`) and breaks the Windows build.
- **sccache** is installed (user env: `RUSTC_WRAPPER=sccache`, `SCCACHE_CACHE_SIZE=40G`). Measured reality: cross-worktree hit rate is only ~27% — workspace crates hash their absolute path into the compilation identity, so a new worktree's first build still takes ~35-40 min. sccache mainly helps re-builds after `cargo clean` in the same directory and registry deps. The incremental dev profile bypasses it entirely. In a fresh shell the vars come from user env; when scripting, set `$env:RUSTC_WRAPPER = 'sccache'` explicitly. `sccache --show-stats` to inspect.
- **Release channel** is `dev` (`crates/zed/RELEASE_CHANNEL`). For Dev: auto-update is disabled and the Windows single-instance check is skipped entirely (`crates/zed/src/main.rs` ~line 359) — multiple local instances run concurrently out of the box. Do not change the channel: preview/stable builds re-enable the single-instance mutex (`Zed-Editor-<Channel>-Instance-Mutex`) and auto-update.
- **Icon embedding**: the exe/taskbar icon is embedded at build time from `crates/zed/resources/windows/app-icon-dev.ico` by `crates/windows_resources/src/windows_resources.rs` (resource ID 1; channel unset → dev arm). The About window uses `include_bytes!` of `crates/zed/resources/app-icon-dev.png` (`crates/zed/src/zed.rs`, `about_window_icon`). No code changes needed to swap icons.
  - **Gotcha**: after replacing icon files, update the mtime of `crates/zed/build.rs` (`(Get-Item crates\zed\build.rs).LastWriteTime = Get-Date`) — its build script only emits `rerun-if-env-changed`, so cargo may not re-embed resources otherwise.
- **Local-only commits**: this branch carries local commits (crimson dev icons, `script/new-worktree.ps1`, this skill). Never push them; when preparing a PR, branch from `origin/main` or cherry-pick around them.
- **Data isolation**: by default all instances share `%LOCALAPPDATA%\Zed` (sqlite with 500ms busy timeout and a silent in-memory fallback, window state, extensions — they race). For independent instances pass `--user-data-dir <dir>`; config is then read from `<dir>\config` (a junction to `%APPDATA%\Zed` keeps settings/keymaps shared while DB/state/extensions stay isolated). `state_dir`/`temp_dir` remain global but are PID-keyed — no conflicts.

## Workflows

### Build & run the main checkout

```powershell
cargo run --profile release-fast   # or: cargo build --profile release-fast --package zed
```

Binary: `target\release-fast\zed.exe` (crimson icon = local build).

### Parallel work: new worktree with an independent instance

```powershell
.\script\new-worktree.ps1 <name>            # new branch <name> from HEAD
.\script\new-worktree.ps1 <name> <branch>   # existing branch
```

This creates `..\zed-<name>` plus data dir `%LOCALAPPDATA%\Zed-Local\<name>` with a `config` junction to `%APPDATA%\Zed`. Then, from the worktree:

```powershell
cargo run --profile release-fast -- --user-data-dir "$env:LOCALAPPDATA\Zed-Local\<name>"
```

First build of a new worktree is a near-cold ~35-40 min (see sccache note above); afterwards incremental rebuilds in that worktree are fast. Cleanup: `git worktree remove --force ..\zed-<name>` and delete the data dir (`core.longpaths=true` is set in global git config — without it removal fails on the deep `target\` paths).

### Recolor the icon (e.g. a unique color per worktree)

Use `recolor.py` in this skill directory (needs Pillow, present in system Python). It hue-shifts the saturated blue preview icon; the white Z logo is unaffected. Then verify visually by Reading the output PNG.

**Important**: build the `.ico` from the ORIGINAL Windows preview `.ico` (full-canvas design), NOT from the macOS-style PNGs — those have ~16% transparent margins and render undersized in the taskbar. Get the pristine source with `git show <upstream-commit>:crates/zed/resources/windows/app-icon-preview.ico > $env:TEMP\orig-preview.ico` (any upstream commit, e.g. `origin/main`).

```powershell
python .claude/skills/zed-local/recolor.py recolor crates/zed/resources/app-icon-preview.png crates/zed/resources/app-icon-dev.png <shift>
python .claude/skills/zed-local/recolor.py recolor crates/zed/resources/app-icon-preview@2x.png crates/zed/resources/app-icon-dev@2x.png <shift>
python .claude/skills/zed-local/recolor.py ico $env:TEMP\orig-preview.ico crates/zed/resources/windows/app-icon-dev.ico <shift>
```

Shift is in 0–255 hue units. Source blue ≈ 220°; formula: `shift = round(((target_deg - 220) % 360) * 256 / 360)`.

| Color | shift |
|---|---|
| crimson (current main) | 89 |
| pure red | 96 |
| orange | 121 |
| green | 185 |
| teal | 213 |
| purple | 43 |
| magenta | 57 |

Occupied by official channels: black = stable, blue = preview, dark violet = nightly, gray = original dev.

Remember the build.rs mtime gotcha above, then rebuild for the icon to take effect.
