# First production installation checklist

Use this runbook for the small Windows x64 fixture tracked by backend issues
[#217](https://github.com/pedromello/manifoldpowered.com/issues/217) and
[#218](https://github.com/pedromello/manifoldpowered.com/issues/218). Keep the
artifact, account, and game dedicated to integration testing.

## 1. Build and validate the ZIP

- Build the game executable for Windows x64.
- Put the executable and every runtime dependency beneath one archive root.
- Keep the fixture small enough for repeated download tests.
- Run the preflight validator from the desktop repository:

  ```powershell
  npm run artifact:preflight -- C:\path\game.zip --entrypoint bin\Game.exe --output upload-declaration.json
  ```

- Add `--working-directory`, repeatable `--executable`, repeatable
  `--launch-argument`, or repeatable `--environment KEY=VALUE` options only when
  the game needs them. Never put credentials in manifest environment values.
- Treat any non-zero exit as a publication blocker. The validator reads every
  ZIP entry, rejects traversal, symlinks and duplicate Windows paths, verifies
  the declared entrypoint, and calculates the lowercase SHA-256 and exact byte
  sizes.
- Review `upload-declaration.json`. It is the complete request body for the
  signed-upload endpoint; release and artifact IDs are assigned by the backend.

## 2. Publish through the production API

Use an authenticated studio-owner session. Keep the `session_id` cookie and all
signed URLs out of shell history, screenshots, tickets, and source control.

1. `POST /api/v1/games/:slug/releases` with a version and optional release
   notes. Record the returned draft `release.id` and `release_number`.
2. `POST /api/v1/releases/:release_id/artifacts/upload-url` with the exact JSON
   emitted by preflight. Record `artifact.id`.
3. Upload the unchanged ZIP directly to the returned object-storage URL using
   the exact method and headers in the upload authorization. Do not send the
   Manifold session cookie to object storage.
4. `POST /api/v1/artifacts/:artifact_id/confirm` without recomputing or editing
   metadata client-side. Confirmation is retry-safe.
5. Require the response to report artifact state `READY`, release state
   `PUBLISHED`, and `published: true` before continuing.
6. Resolve the release, manifest, and signed download through the three player
   delivery endpoints. Confirm that their release ID, artifact ID, SHA-256,
   sizes, target, manifest version, and entrypoint match the declaration.

If upload confirmation fails, preserve the response request ID but never the
signed URL. Do not alter an immutable or published release. For replacement,
create a fresh draft/release number, run preflight again, and retain the former
release until the replacement completes the full smoke test.

## 3. Prepare the entitled test account

- Use a dedicated activated account that can complete OTP login.
- Grant that account entitlement to the fixture game through the intended
  outlet and verify the outlet attribution returned by `/api/v1/library`.
- Confirm the fixture appears in the desktop library with a compatible
  `WINDOWS` / `X86_64` release before clicking Install.

## 4. Exercise the desktop lifecycle

- Start a production-configured development build or install the latest CI
  Windows package.
- Sign in with OTP and open Library.
- Install the fixture and confirm visible transitions through resolving,
  downloading, verifying, extracting, installing, and installed.
- During one run, interrupt the transfer and retry. Confirm that progress
  resumes from the partial download.
- Launch the game from the desktop app and verify the declared working
  directory, arguments, and environment behavior.
- Restart Manifold Desktop, confirm the installation remains registered, and
  launch it again without downloading.
- In Settings, load and copy Installation diagnostics. Confirm the evidence has
  stable IDs/codes and contains no session token, signed URL, or local path.

## 5. Record evidence and close the milestone

- Record desktop commit/build, backend deployment, game slug, release ID,
  artifact ID, version, release number, SHA-256, and test result.
- Attach only sanitized diagnostics and redacted screenshots to issues #217 and
  #218.
- Keep the validated ZIP and preflight declaration in controlled release
  storage so the test can be reproduced.
- After the signed Windows build is verified, re-enable Windows Smart App
  Control/WDAC protections that were relaxed for local Cargo development.
