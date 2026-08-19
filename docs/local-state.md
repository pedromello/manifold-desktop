# Local state

The native layer stores only the minimum state needed by the desktop client:

- `session_id` is stored in the operating-system credential vault through `keyring` and deleted on logout. It is never returned to the WebView or logged.
- `installations.json` records the game slug, release, installation directory, entrypoint, launch settings, and installation time. Missing entrypoints are reconciled out when the registry is read.
- `installation-preferences.json` contains the optional user-selected installation root.
- `downloads/<artifact>.part` holds resumable partial downloads. A completed archive is deleted after installation.
- `installation.log` records timestamps, game slugs, and non-secret outcomes for support diagnostics.
- `manifold.language` in WebView local storage contains only the selected UI locale.

JSON state is written through a temporary file and rollback backup. Game files are extracted to a staging directory and activated by rename. Signed download URLs, OTP codes, session tokens, and API response bodies are never persisted.
