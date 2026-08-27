# Local state

The native layer stores only the minimum state needed by the desktop client:

- `session_id` is stored in the operating-system credential vault through `keyring` and deleted on logout. It is never returned to the WebView or logged.
- `installations.json` records the game slug, release, installation directory, entrypoint, launch settings, and installation time. Missing entrypoints are marked as needing repair when the registry is read.
- `pending-uninstalls.json` is a short-lived uninstall journal. It lets the app finish deleting a game safely after a crash or power loss without touching ownership or account data.
- `installation-preferences.json` contains the optional user-selected installation root.
- `downloads/<artifact>.part` holds resumable partial downloads. A completed archive is deleted after installation.
- `installation.log` records timestamps, game slugs, and non-secret outcomes for support diagnostics.
- `manifold.language` in WebView local storage contains only the selected UI locale.

JSON state is written through a temporary file and rollback backup. Game files are extracted to a staging directory and activated by rename. Uninstall targets are resolved from the trusted registry and must be direct children of a configured Manifold installation root; symlinks and paths outside those roots are rejected. Signed download URLs, OTP codes, session tokens, and API response bodies are never persisted.
