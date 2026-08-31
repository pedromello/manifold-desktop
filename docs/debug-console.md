# Incremental distribution debug console

Manifold Desktop includes an opt-in companion terminal for observing release publication and game updates. It is intended for development, troubleshooting, and learning. The console is read-only and is disabled by default.

## Start it

```bash
npm run desktop:debug
```

The command sets `MANIFOLD_DEBUG_CONSOLE=1` and starts the normal Tauri development app. A second terminal opens with the Manifold ASCII wordmark and waits for a real publish or update operation.

Two safe, synthetic demonstrations are available without an account or game data:

```bash
npm run desktop:debug:publisher-demo
npm run desktop:debug:updater-demo
```

You can also set the variable when starting an already-built executable. Values `1`, `true`, `yes`, and `on` enable the console. `NO_COLOR=1` disables ANSI colors.

## What it shows

- API decisions, route templates, update strategy, and fallback availability;
- predecessor download or verified cache reuse;
- Butler `diff`, `apply`, and `verify` stages and aggregate progress;
- an exact, bounded PWR operation map for the largest files (`R` for local `BLOCK_RANGE`, `D` for fresh `DATA`);
- patch/full size ratio and the inclusive 80% decision;
- `.pwr` and signature upload/download progress;
- staging, canonical-signature verification, activation, and automatic full fallback.

Wharf compares content in 64 KiB blocks, but it does **not** download each changed block as a separate request. `BLOCK_RANGE` operations reuse bytes from the installed/source build, `DATA` operations carry fresh bytes, and the network downloads one `.pwr` payload plus its signature. The console uses those terms so its visualization remains technically accurate.

The map is decoded locally from the generated or downloaded PWR stream. It samples the real ordered operations into 24 terminal cells per file; it is not an estimate derived from ZIP sizes. Inspection is bounded and diagnostic-only, so an unsupported future encoding merely hides the map and never blocks publication or update.

## Privacy and failure isolation

The debug stream never intentionally contains signed URLs, cookies, authorization headers, tokens, secrets, or raw local paths. Sensitive field names are redacted centrally. Raw Butler lines containing path or URL markers are discarded.

The app sends versioned NDJSON events through a bounded, non-blocking queue to a one-time token-authenticated loopback socket. If the console is slow, closed, or crashes, debug events may be dropped and the game operation continues normally.

A sanitized replay is written to:

```text
%LOCALAPPDATA%/Manifold/debug-sessions/dbg-<session>.jsonl
```

On platforms without `LOCALAPPDATA`, the system temporary directory is used. Signed authorizations are never persisted in this replay.

## Reading a typical update

1. `resolve_update`: the backend chooses `PATCH` or `FULL` for the exact installed release.
2. `downloading_update`: Desktop downloads one patch payload and one signature; Range/ETag resume remains active underneath.
3. `staging`: the active installation is copied into an isolated workspace.
4. `apply`: Butler combines local `BLOCK_RANGE` data and fresh `DATA` operations.
5. `verify`: Butler proves staging matches the canonical target signature.
6. activation: Desktop swaps the verified staging directory into place and persists the registry.
7. `full_fallback`: if patch download/apply/verification fails, Desktop preserves the old version and automatically uses the complete ZIP.
