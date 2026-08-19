# Desktop API contract

The integration source of truth is the Manifold website repository's [Desktop API v1 TypeScript contract](https://github.com/pedromello/manifoldpowered.com/blob/main/contracts/desktop/v1.ts), [OpenAPI document](https://github.com/pedromello/manifoldpowered.com/blob/main/docs/openapi/manifold-desktop-v1.yaml), and [compatibility policy](https://github.com/pedromello/manifoldpowered.com/blob/main/docs/manifold-desktop-api-compatibility.md). This scaffold was validated against upstream commit `d165dda5fa518a97bfe7a3e5dc856a2ed7b1e6cf`.

`src/contracts/desktop-v1.ts` is a vendored snapshot of the application-independent Zod contract. It deliberately retains the API and manifest version literals, uppercase platform and architecture vocabulary, string byte sizes, SHA-256 rules, and path-confinement checks. When updating it:

1. Review the upstream compatibility policy and OpenAPI diff.
2. Replace the snapshot from `contracts/desktop/v1.ts` rather than importing website-internal models.
3. Update the pinned upstream commit above and run the contract tests.
4. Treat new API or manifest versions as explicit implementations; never silently accept them as v1.

The native API root is `/api/v1/desktop`. Production uses `https://manifoldpowered.com`; development uses the website's conventional local origin, `http://localhost:3000`. Because upstream does not publish a canonical staging hostname, staging requires an explicit HTTPS `MANIFOLD_API_BASE_URL` origin. API calls, bearer-token handling, downloads, installation, filesystem mutation, and game launching belong in narrow Rust commands. The WebView must not receive a generic HTTP client or bearer token.

The planned MVP sequence is session creation, library listing for a `Platform`/`Architecture` target, compatible-release resolution, manifest retrieval, download authorization, resumable native download, and hash verification. This initial scaffold only establishes the versioned contract and target vocabulary; it does not pretend that placeholder screens implement those security-sensitive operations.
