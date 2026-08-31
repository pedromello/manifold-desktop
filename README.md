# Manifold Desktop

Tauri 2 desktop shell for Manifold, with a React, TypeScript, and Vite frontend.

## Prerequisites

- Node.js 22 and npm 10 or newer
- Rust stable with `rustfmt` and `clippy`
- [Tauri 2 platform prerequisites](https://v2.tauri.app/start/prerequisites/) (WebKitGTK development packages on Linux, Xcode tools on macOS, or WebView2 and C++ build tools on Windows)

## Local development

Install exactly what is recorded in the lockfiles and launch the native application:

```sh
npm ci
npm run sidecar:prepare
npm run tauri dev
```

Use `npm run dev` for the browser-only frontend. The app defaults to `production` and the store catalog connects directly to `https://manifoldpowered.com/api/v1`. Other application environments can still be selected explicitly with one of:

```sh
VITE_APP_ENV=development npm run tauri dev
MANIFOLD_API_BASE_URL=https://staging.example.com VITE_APP_ENV=staging npm run tauri dev
VITE_APP_ENV=production npm run tauri build
```

Only `development`, `staging`, and `production` are accepted. Production uses `https://manifoldpowered.com/api/v1`. Upstream does not define a staging hostname, so staging requires an explicit HTTPS origin in `MANIFOLD_API_BASE_URL`. The environment value is embedded during the frontend build, while Rust owns API-origin validation. Do not put secrets in Vite environment variables.

The vendored [distribution API v1 contract](src/contracts/desktop-v1.ts) and [incremental-update implementation guide](docs/incremental-game-updates.md) is synchronized from the website repository; see the [contract policy and provenance](docs/desktop-api-contract.md). Network calls, the native session cookie jar, downloads, installation, filesystem mutation, and process launching are implemented only as narrow Rust commands.

The UI ships with `pt-BR` and `en-US`. It initially follows the operating-system language and stores the user's explicit selection locally. New UI strings belong in both resource trees in `src/i18n.ts`.

To observe incremental publication and updates in a separate, sanitized terminal, use `npm run desktop:debug`. See the [incremental distribution debug console guide](docs/debug-console.md) for the ASCII dashboard, replay files, and account-free demos.

## Validation

```sh
npm run format
npm run lint
npm run typecheck
npm test
npm run build
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features --locked -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --locked
cargo check --manifest-path src-tauri/Cargo.toml --locked
```

## Game artifact preflight

Before requesting a publisher upload URL, validate the Windows x64 ZIP and emit
the exact API declaration:

```powershell
npm run artifact:preflight -- C:\path\game.zip --entrypoint bin\Game.exe --output upload-declaration.json
```

The command fails on unsafe or inconsistent archives and calculates the
lowercase SHA-256 plus compressed and installed sizes. Follow the
[first production installation checklist](docs/first-production-install.md)
for publication, entitlement, installation, launch, and evidence collection.

## Packaging

After validation, set the production environment and let Tauri create platform-native packages:

```sh
VITE_APP_ENV=production npm run tauri build
```

Signing and notarization credentials must be supplied by the release environment, never committed. See [the release process](docs/release-process.md) and [architecture](docs/architecture.md).

The Windows installer is written to `src-tauri/target/release/bundle/nsis/` and the MSI, when available on the runner, to `src-tauri/target/release/bundle/msi/`. Distribute the signed installer rather than the loose executable. Windows code-signing is still required before a public release to avoid reputation and publisher warnings.
