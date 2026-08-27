# Publisher MVP backend dependencies and gaps

Audited against manifoldpowered.com PR #244 at
acefa721b11eeec874a0cfa90d74db018b7bf0cf.

The direct upload path is available and is used by the desktop publisher:
draft creation, idempotent upload authorization, direct storage PUT, and
transactional confirmation/publication. The desktop does not send ZIP bytes
through Next/Vercel and never persists signed URLs, authorization headers, or
session cookies.

## Release listing and draft updates

Backend PR #244 adds the authenticated contracts consumed by this desktop PR:

    GET /api/v1/games/:slug/releases?page=1&limit=20
    PATCH /api/v1/games/:slug/releases/:release_id

The GET response is ordered by release_number descending and contains a safe
game projection, releases in every status, allow-listed artifact metadata, and
pagination. It omits storage object keys, signed URLs, required headers,
cookies, tokens, and manifest environment values. The desktop uses it as the
source of truth, supports pagination, and can turn a draft created on another
device into a local resumable publication without asking for technical IDs.

The PATCH accepts version and/or release_notes only while the release is DRAFT.
An explicit release_notes null clears notes, while an omitted field preserves
them. The desktop uses this route when the user goes back from File to Details,
so it updates the same draft instead of creating an orphan draft.

Both routes require an authenticated owner or studio member with the effective
create:game_release permission for the game. The desktop handles 401, 403, 404,
loading, error, and empty states, but the backend remains authoritative.

PR #244 must be merged and deployed before shipping this desktop change. It
contains no additional migration or runtime dependency.

## Publisher game discovery and effective capabilities

GET /api/v1/studios returns every studio the user owns or belongs to, but it
does not return the member's scoped permissions. GET
/api/v1/studios/:slug/games requires update:studio. A member granted only
create:game_release and create:game_artifact can therefore publish a known
game but cannot discover it through the desktop.

Minimum compatible options:

1. Allow the studio games read for owners and members with update:game,
   create:game_release, or create:game_artifact; or
2. Add a publisher-specific game projection; and
3. Add an effective capability projection to each item from GET /studios:

   {
   "access": {
   "role": "MEMBER",
   "permissions": ["create:game_release", "create:game_artifact"]
   }
   }

Until this exists, the desktop shows Studio after /studios returns at least
one item, calls the protected games endpoint, and presents a localized 403
state. Server-side authorization remains authoritative.

## Failed verification recovery

An upload declaration is reusable only while its artifact is PENDING.
Confirmation failures mark the artifact/release FAILED, but there is no public
remove/replace endpoint. A failed object can therefore leave the target slot
unavailable for a clean retry. A safe remove/replace operation for unpublished
artifacts, or identical reauthorization from FAILED, remains a backend gap.
