# Architecture

The Vite-powered React application in `src/` owns presentation and navigation. The Tauri 2 core in `src-tauri/` owns native and privileged behavior. IPC is a trust boundary: commands validate every frontend-supplied value and expose one narrow operation rather than generic shell, network, credential, installation, process, or filesystem primitives.

API targets are selected at build time with `VITE_APP_ENV`, but their origins are resolved and validated by Rust. Production-like environments require HTTPS. New privileged integrations require a threat review, a typed Rust command, input validation, tests, and the smallest applicable capability permission.

Authentication reuses Manifold's passwordless OTP flow and server-side
`session_id` cookie. Narrow Rust commands request and verify the OTP, own the
native HTTP cookie jar, and make authenticated API requests. The React WebView
never receives the session cookie or a generic cross-origin HTTP capability.

The desktop application consumes the same client-neutral API v1 resources as
the website at `/api/v1`. Its supported native targets are `WINDOWS`, `MAC`, and
`LINUX`, paired with `X86_64` or `AARCH64`. See the
[contract synchronization policy](desktop-api-contract.md).
