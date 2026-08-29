# Butler / Wharf runtime notice

Manifold Desktop bundles the target-specific runtime files from Butler v15.30.0, commit/tag source at <https://github.com/itchio/butler/releases/tag/v15.30.0>.

Butler is copyright itch corp. and contributors and is distributed under the MIT license reproduced in `LICENSE`.

The official Butler archives also contain the target-specific 7-Zip integration (`7z.*` and `libc7zip.*`/`c7zip.dll`). Upstream documents components under LGPL-2.1 and MPL-2.0 plus the 7-Zip unRAR restriction. Source and notices are available from:

- <https://github.com/itchio/butler>
- <https://github.com/itchio/boar>
- <https://www.7-zip.org/license.txt>

Only offline `diff`, `apply`, and `verify` commands are invoked. The bundled runtime has no update mechanism, receives no Manifold or itch.io credential, and is never allowed to perform network operations. Archive and runtime SHA-256 values are pinned in `manifest.json` and verified both during bundle preparation and at every runtime launch.
