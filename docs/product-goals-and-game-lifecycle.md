# Product goals and game lifecycle

## Primary product goal

Manifold Desktop is first and foremost the native game client for a user's Manifold library. The initial product priority is not storefront browsing. The critical experience is that a signed-in user can see the games they own, install them with as little manual work as possible, keep them updated, and launch them directly from the desktop application.

The target happy path is:

1. Sign in to Manifold.
2. Load the user's library for the current platform and architecture.
3. Select an owned game.
4. Download the compatible release.
5. Verify the downloaded files.
6. Install the game automatically into a managed local location.
7. Show installation and update state in the library.
8. Launch the installed game from a single **Play** action.
9. Detect when the launched game exits and return the local installation to an idle state.

Storefront/discovery functionality can be added to the desktop application, but it is secondary to making the owned-game lifecycle reliable.

## Library

The library is the main desktop surface for the MVP. It should expose the user's owned games and enough local state to make each game's current action obvious, for example:

- not installed;
- downloading;
- installing;
- installed and ready to play;
- update available;
- updating;
- running;
- failed, with a recoverable error state.

The desktop client should resolve only releases that are compatible with the current `Platform` and `Architecture` defined by the distribution API v1 contract.

## Download and installation

Downloads and installation are privileged native operations and belong in narrowly scoped Rust/Tauri commands rather than the WebView.

For the MVP, installing a game should be a single user action. The native client is responsible for the rest of the lifecycle: obtaining download authorization, downloading the release, verifying integrity (including the manifest's SHA-256 information), writing files only inside the application's managed installation location, and preparing the executable for launch.

Resumable downloads should be preferred where supported so interrupted transfers do not unnecessarily restart from zero.

## Updates

### Initial update strategy

The first implementation may update a game by downloading a complete new release again. This deliberately favors correctness and a simpler first implementation over bandwidth efficiency.

The desired user experience remains automatic: when an update is required, the desktop client should handle download, verification, replacement/installation, and transition back to a playable state without asking the user to manage individual files.

The client must never launch a partially installed or partially updated release.

### Future incremental/chunked updates

A later version should support incremental updates so the client does not need to download the complete game for every release.

The intended direction is a content/chunk-based update system: releases are represented by deterministic chunks or blocks, the client determines which chunks of the target release are already present locally, downloads only the missing or changed chunks, verifies them, and reconstructs or patches the final installation atomically.

This is a future optimization and should not block the initial complete-release update implementation. The manifest and installation architecture should, however, avoid assumptions that would make chunked/delta delivery unnecessarily difficult to introduce later.

## Launching games

Once an installation is valid and up to date, the user should be able to click **Play** in Manifold Desktop and have the native client launch the correct executable with the appropriate working directory and launch configuration.

Process launching must be implemented as a narrow native command. The WebView must not receive generic shell/process execution capabilities.

The client should track the lifecycle of the process it launches so it can distinguish at least `ready`, `launching`, `running`, and `exited` states and can report launch failures in a recoverable way.

The first Windows implementation tracks the direct entrypoint process launched during the current native app session. This is enough to prevent duplicate launches, keep a reloaded WebView synchronized, and block file mutations while the process is alive. It does not yet rediscover a game launched outside Manifold, survive a full desktop-app restart, or follow a launcher that exits after handing off to another process. Those cases require verified persisted process identity or the server-backed lease model below; PID alone must never be trusted because operating systems reuse it.

## Concurrent-play entitlement rule

Manifold must prevent the same account entitlement for the same game from being actively played on multiple computers at the same time.

Example: if one account is signed in on computers A, B, and C and starts game X on computer A, the same account must not be able to start game X concurrently on computers B or C. The account may still start different owned games on those other computers unless another product rule explicitly forbids it.

This rule is scoped to **account + game**, not to the whole account. The desired behavior is therefore:

- game X running on computer A -> game X cannot start on B or C;
- game X running on computer A -> game Y may start on B or C;
- when the game X play session on A ends (or is otherwise determined to be no longer active), the entitlement becomes available for game X again.

### Planned enforcement model

The authoritative decision must be server-side. A local-only lock is insufficient because multiple devices cannot trust each other's local state.

A future API version or extension should introduce a server-backed play-session or game-lease concept. A robust design is expected to include:

1. the desktop client requests permission to launch a specific game for the authenticated account and device;
2. the server atomically creates a play session/lease only if no active session for that account + game already exists;
3. only after the lease is granted does the client launch the game;
4. while the game is running, the client periodically renews the lease with a heartbeat;
5. when the process exits normally, the client explicitly releases the lease;
6. if the client crashes, loses connectivity, or the machine powers off, the lease expires after a bounded timeout instead of remaining locked forever;
7. another device attempting to launch the same game while a valid lease exists receives a clear conflict response and must not launch it.

The exact heartbeat interval, timeout, offline behavior, device identity model, and recovery UX are intentionally left for a dedicated security/design review before implementation.

The server must not treat the desktop application's claim that a game has stopped as the only source of truth; lease expiration and server-side concurrency guarantees are required to recover safely from ungraceful client termination.

## MVP priority order

The current intended order is:

1. authentication/session creation;
2. library listing for the current platform/architecture;
3. compatible release and manifest resolution;
4. download authorization;
5. resumable native full-release download;
6. integrity verification;
7. automatic installation and managed local state;
8. one-click game launch and process lifecycle tracking;
9. complete-release automatic updates;
10. server-backed concurrent-play protection for the same account + game;
11. incremental/chunked update delivery;
12. richer storefront/discovery experiences in the desktop client.

This ordering expresses product priority, not an immutable implementation dependency graph. Security requirements that affect an earlier step must be addressed before that step ships.
