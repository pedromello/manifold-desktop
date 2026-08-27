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

## Distribution and installation

React resolves a typed distribution plan through `distribution.ts`; tests can inject a fixture adapter while normal builds use the production API. Rust independently validates release, artifact, checksum, HTTPS URL, and manifest path relationships before touching disk.

Artifacts download into an application-owned partial file. A retry requests the remaining range, the complete archive is checked with SHA-256, and extraction happens in a staging directory with traversal and symlink protection. The staging directory replaces the previous game directory only after verification. Launching is limited to the manifest entrypoint beneath the recorded installation root.

The download URL is short-lived and never stored. Failed downloads retain only the partial artifact so a freshly authorized request can resume it. The native downloader uses connection and read-idle timeouts rather than a whole-transfer deadline, retries transient transport and storage failures with bounded exponential backoff and jitter, and validates `Content-Range` and `ETag` before appending resumed bytes. Authorization renewal remains invisible to the player: progress stays in the downloading phase and continues from the saved offset. Speed and remaining time are smoothed presentation estimates derived from progress events and are not persisted.
