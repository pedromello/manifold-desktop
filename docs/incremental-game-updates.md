# Incremental game updates (Desktop)

The protocol source of truth is the backend's official [incremental game updates document](https://github.com/pedromello/manifoldpowered.com/blob/main/docs/incremental-game-updates.md), executable [desktop v1 contract](https://github.com/pedromello/manifoldpowered.com/blob/main/contracts/desktop/v1.ts), and [OpenAPI document](https://github.com/pedromello/manifoldpowered.com/blob/main/docs/openapi/manifold-desktop-v1.yaml). This Desktop implementation deliberately keeps wire schemas in `src/contracts/desktop-v1.ts` and app-specific hydrated execution plans in `src/distribution.ts`.

## Player update transaction

1. `GET /api/v1/games/:slug/updates/latest` receives `source_release_id`, `platform`, and `arch`. Its `UpdatePlan` contains no signed URL.
2. PATCH authorizations come from `POST /api/v1/patches/:patch_id/download`. Patch and signature have independent URLs, sizes, SHA-256 values, ETags, and resumable partial files. An expired or retry-exhausted authorization returns once to React for `resolveUpdate + prepareUpdate`; the renewed native attempt reuses the same `<patch_id>.pwr.part` with `Range`/`If-Range`, and a second failure enters FULL fallback.
3. FULL and fallback authorization uses `POST /api/v1/artifacts/:fallback_artifact_id/download`.
4. A patch is copied and applied only in `.<slug>.staging`. Butler validates the target signature and the manifest entrypoint before promotion. The active installation is never patched in place.
5. Any patch-path failure other than cancellation switches automatically to the complete ZIP. The currently installed game remains playable unless and until a fully verified staging tree is promoted.
6. Promotion renames the active tree to `.<slug>.backup`, renames staging into place, persists `installations.json`, persists the completed journal phase, and only then removes the backup. A registry failure restores the backup.

`pending-updates.json` records no URL or credential. Startup recovery derives paths from the managed installation registry: pre-promotion phases discard staging, an interrupted activation rolls back when the registry still names the source, and a registry-persisted target finalizes the retained backup. Running games block update and uninstall. Cancellation flips the operation token and the Butler supervisor kills and reaps its child process.

The UI checks at library load/login, manual refresh, and every 15 minutes. It exposes `preparing_update`, `downloading_update`, `applying_update`, `verifying_update`, and `full_fallback`, plus target version, patch size, and savings when the resolver selects PATCH.

## Publisher transaction

The existing publisher is currently Windows/X86_64-only: ZIP preflight requires a Windows `.exe`, and the artifact declaration fixes `WINDOWS`/`X86_64`. Incremental publication derives those values from that same declaration rather than introducing a second target decision.

For N→N+1, the publisher declares the complete artifact but does not upload/finalize it yet. It downloads or reuses the SHA-verified predecessor cache, checks conservative temporary-space requirements, and runs pinned Butler `diff --verify`, `apply --dir ... --signature ...`, and `verify`. A patch is used only when `patch_size * 100 <= full_zip_size * 80`.

An eligible patch and signature are uploaded as streams, confirmed READY, and only then is the complete ZIP uploaded and confirmed. Any required patch generation, validation, upload, or confirmation failure returns before ZIP transfer, leaving the release draft retryable. `publisher-patches/<release>/recovery.json` stores only release/source identities, local paths, SHA-256 declarations, sizes, required temporary bytes, and the last completed phase—never signed URLs. A READY retry with `uploads: null` continues directly to the ZIP. Recovery files are removed only after the ZIP confirmation publishes the release.

## Pinned Butler sidecar

`src-tauri/vendor/butler/v15.30.0/manifest.json` pins official v15.30.0 release archives and every runtime file for Windows x86_64, Linux x86_64/aarch64, and macOS x86_64/aarch64. `npm run sidecar:prepare` downloads the one archive for the current build target, verifies the archive SHA-256, extracts only the declared runtime files, verifies each file SHA-256, and writes the generated target directory under `src-tauri/resources/butler/15.30.0/`.

Tauri maps that generated directory to `$RESOURCE/butler/`; each bundle therefore contains exactly one target's `butler` executable and its two 7-Zip runtime libraries. CI runs `npm run sidecar:test` to verify the closed platform/architecture mapper, and must run `npm run sidecar:prepare` before `npm run tauri build` (the Tauri `beforeBuildCommand` enforces preparation). Node `x64` maps to `x86_64`, `arm64` maps to `aarch64`, and every other architecture is rejected. Generated binaries are ignored by Git. Runtime startup verifies all bundled files again, clears the child environment except temporary-directory hints, invokes only the fixed offline commands with JSON output, and offers no update, network, credential, or generic-shell command.

The bundled Butler MIT license and third-party notice are mapped to `$RESOURCE/licenses/` from `src-tauri/vendor/butler/v15.30.0/`.

### Optional real Butler smoke test

The default suites exercise argv, JSON diagnostics, cancellation, hashes, transactions, and patch policy without network or an installed sidecar. After `npm run sidecar:prepare`, run the opt-in local integration fixture with:

```sh
cargo test --manifest-path src-tauri/Cargo.toml butler::tests::prepared_sidecar_real_diff_apply_verify -- --ignored
```

It verifies the prepared runtime hashes, generates a real patch from two temporary local trees, applies it to a separate destination, and verifies the resulting signature. It makes no network call.
