# Architecture

The Vite-powered React application in `src/` owns presentation and navigation. The Tauri 2 core in `src-tauri/` owns native and privileged behavior. IPC is a trust boundary: commands validate every frontend-supplied value and expose one narrow operation rather than generic shell, network, credential, installation, process, or filesystem primitives.

API targets are selected at build time with `VITE_APP_ENV`, but their origins are resolved and validated by Rust. Production-like environments require HTTPS. New privileged integrations require a threat review, a typed Rust command, input validation, tests, and the smallest applicable capability permission.

The desktop integration follows the website's independent API v1 contract at `/api/v1/desktop`. Its supported native targets are `WINDOWS`, `MAC`, and `LINUX`, paired with `X86_64` or `AARCH64`. See the [contract synchronization policy](desktop-api-contract.md).
