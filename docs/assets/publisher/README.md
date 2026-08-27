# Publisher flow screenshots

These screenshots were captured from the real Tauri desktop window at the same
1200 × 800 viewport using the development-only publisher fixture adapter. They
exercise the production UI and recovery state without calling the production
API or uploading an artifact.

0. `00-home-reference.png` — current Home used as the visual reference
1. `01-studio.png` — authorized studio and compact game list
2. `02-release-data.png` — version details and player-facing notes
3. `03-file-preflight.png` — selected Windows 64-bit ZIP before local analysis
4. `04-entrypoint-manifest.png` — native analysis results, main executable, and
   collapsed technical details
5. `05-upload.png` — direct-storage upload progress
6. `06-verification.png` — final verification and publication
7. `07-success.png` — version ready for players

The displayed account, studio, game, release, and upload progress are fixture
data in pt-BR. The archive sizes, checksum, and executable paths mirror the
provided Peggy's Post sample and were independently verified by the native
preflight CLI. No signed URL, cookie, token, or local user path is displayed.
