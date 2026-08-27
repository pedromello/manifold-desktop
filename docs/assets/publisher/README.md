# Publisher flow screenshots

These screenshots were captured from the real Tauri desktop window using the
development-only publisher fixture adapter. They exercise the production UI
and recovery state without calling the production API or uploading an artifact.

1. `01-studio.png` — authorized studio and game selection
2. `02-release-data.png` — release version and notes
3. `03-file-preflight.png` — selected Windows x86-64 ZIP
4. `04-entrypoint-manifest.png` — native preflight results and manifest review
5. `05-upload.png` — direct-storage upload progress
6. `06-verification.png` — storage verification and publication
7. `07-success.png` — published release confirmation

The displayed account, studio, game, release, and upload progress are fixture
data. The archive sizes, checksum, and executable paths mirror the provided
Peggy's Post sample and were independently verified by the native preflight CLI.
