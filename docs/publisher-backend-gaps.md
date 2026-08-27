# Publisher MVP backend gaps

Audited against manifoldpowered.com origin/main at
323baa386c927ff759a3eecee89d4fcf5e108fc0 (PR #243).

The direct upload path is available and is used by the desktop publisher:
draft creation, idempotent upload authorization, direct storage PUT, and
transactional confirmation/publication. The desktop does not send ZIP bytes
through Next/Vercel and never persists signed URLs, authorization headers, or
session cookies.

## Missing release listing

The /api/v1/games/:slug/releases route currently exposes only POST. This means
the desktop cannot list releases created on another device or recover a draft
after its local recovery record is lost. The MVP therefore labels its release
list as device-local and persists only release metadata, the local archive
path, inspection results, and manifest.

Expected minimum contract:

    GET /api/v1/games/:slug/releases?page=1&limit=50
    Cookie: session_id=...

    {
      "releases": [
        {
          "id": "uuid",
          "game_id": "uuid",
          "version": "1.0.0",
          "release_number": 1,
          "status": "DRAFT",
          "release_notes": null,
          "published_at": null,
          "created_at": "ISO-8601",
          "updated_at": "ISO-8601",
          "artifacts": [
            {
              "id": "uuid",
              "platform": "WINDOWS",
              "architecture": "X86_64",
              "archive_format": "ZIP",
              "status": "PENDING",
              "compressed_size_bytes": "71059858",
              "installed_size_bytes": "305505736",
              "sha256": "lowercase-hex",
              "manifest": {}
            }
          ]
        }
      ],
      "pagination": {
        "page": 1,
        "limit": 50,
        "total": 1,
        "pages": 1
      }
    }

The response must omit storage object keys, signed URLs, required headers, and
actor/session data. Authorization should accept the game owner or a studio
member with release/artifact publishing permission.

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

## Failed verification and draft correction

An upload declaration is reusable only while its artifact is PENDING.
Confirmation failures mark the artifact/release FAILED, but there is no public
remove/replace endpoint. A failed object can therefore leave the target slot
unavailable for a clean retry.

The backend model has gameRelease.updateDraft, but no API route exposes it. If
product requirements include changing version or release notes after draft
creation, add:

    PATCH /api/v1/releases/:release_id
    { "version"?: "1.0.1", "release_notes"?: "..." }

restricted to DRAFT, plus a safe remove/replace operation for unpublished
artifacts or identical reauthorization from FAILED.
