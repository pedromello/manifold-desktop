# Distribution API contract

The integration source of truth is the Manifold website repository's [distribution API v1 TypeScript contract](https://github.com/pedromello/manifoldpowered.com/blob/main/contracts/desktop/v1.ts), [OpenAPI document](https://github.com/pedromello/manifoldpowered.com/blob/main/docs/openapi/manifold-desktop-v1.yaml), and [compatibility policy](https://github.com/pedromello/manifoldpowered.com/blob/main/docs/manifold-desktop-api-compatibility.md). This snapshot is synchronized with upstream commit `08c8452f238f85dfadb2ce4fe3b75c6d7b4bad74`.

`src/contracts/desktop-v1.ts` is a vendored snapshot of the application-independent Zod contract. It deliberately retains the API and manifest version literals, uppercase platform and architecture vocabulary, string byte sizes, SHA-256 rules, and path-confinement checks. When updating it:

1. Review the upstream compatibility policy and OpenAPI diff.
2. Replace the snapshot from `contracts/desktop/v1.ts` rather than importing website-internal models.
3. Update the pinned upstream commit above and run the contract tests.
4. Treat new API or manifest versions as explicit implementations; never silently accept them as v1.

The native API root is `/api/v1`. Production uses `https://manifoldpowered.com`; development uses the website's conventional local origin, `http://localhost:3000`. Because upstream does not publish a canonical staging hostname, staging requires an explicit HTTPS `MANIFOLD_API_BASE_URL` origin. API calls, cookie-jar handling, downloads, installation, filesystem mutation, and game launching belong in narrow Rust commands. The WebView must not receive a generic HTTP client or the `session_id` cookie.

The backend is API-first and does not expose client-specific endpoint trees.
Authentication reuses `/otp`, `/otp/sessions`, and `/sessions`; catalog and
ownership reuse `/games` and `/library`; release distribution uses general
`/games/:slug/releases`, `/releases`, and `/artifacts` resources.

Catalog game responses are mode-aware. The desktop accepts a nullable local
`price` and consumes `status`, `ownership_status`, `purchase_mode`, and
`external_offer`. `STEAM_ONLY` and `ONLY_DISPLAY` entries are informational:
they may show the captured Steam offer and an HTTPS Steam link, but they must
never be presented as free, locally purchasable, downloadable, installable, or
manageable. A missing external amount means the Steam price is unavailable.

Authentication uses the existing passwordless OTP sequence. A narrow Rust
command requests the one-time code, another verifies it, and the native HTTP
layer retains the secure, HTTP-only cookie returned by the backend. Later
authenticated commands reuse that cookie and logout clears it after the server
revokes the session. Direct WebView requests are intentionally excluded to
avoid cross-origin, CSP, SameSite, and platform-specific WebView cookie
behavior.

API v1's existing session response also contains the opaque token for backward
compatibility. Rust must treat that field as a credential, discard it after the
cookie jar accepts `Set-Cookie`, and never return it through IPC.

The planned MVP sequence is OTP request, OTP verification and cookie-session creation, library listing for a `Platform`/`Architecture` target, compatible-release resolution, manifest retrieval, download authorization, resumable native download, and hash verification. This initial scaffold only establishes the versioned contract and target vocabulary; it does not pretend that placeholder screens implement those security-sensitive operations.
