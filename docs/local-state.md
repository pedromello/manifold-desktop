# Local state

No persistent local state or credentials are stored in this scaffold. UI state remains in memory. Future persistence must document its schema, migration and deletion strategy, use an application-owned directory, and avoid storing credentials in plain text. Opaque desktop session tokens belong in an operating-system credential vault exposed through narrow login, logout, and authenticated-request Rust commands; they must never be returned to the WebView or logged. Signed download URLs are transient and must not be persisted.
