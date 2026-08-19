# Release process

1. Update the versions in `package.json`, `src-tauri/Cargo.toml`, and `src-tauri/tauri.conf.json` together.
2. Regenerate and commit both lockfiles, then run every validation command documented in the README.
3. Build packages from a clean, reviewed commit with `npm run tauri build` and the production environment.
4. Sign and notarize artifacts using CI-managed secrets; never place signing credentials in the repository.
5. Smoke-test the installed artifact, publish checksums and release notes, and retain a rollback artifact.
