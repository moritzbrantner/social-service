# social-service

Reusable modular social backend for Next.js, Expo, and other applications.

## MVP

One deployable Rust/Axum service with internal modules for:

- profiles and avatars/media references;
- posts, comments, follows, follow-graph reads, and a chronological following timeline;
- conversations and messages with media attachments;
- deterministic capability resolution;
- shared public/private visibility policy for profiles and posts;
- PostgreSQL persistence scoped by `X-App-Id`;
- a small framework-independent TypeScript SDK.

The service deliberately does **not** own authentication or UI. A trusted application/auth gateway authenticates the request and injects `X-App-Id` and `X-User-Id`. Do not expose the MVP directly to untrusted clients until a production auth adapter is configured.

Media uploads are represented as registered media assets in the MVP. The API already attaches assets to posts and messages; presigned S3-compatible upload support can be added behind the media module without changing those domain relationships.

## Timeline architecture

The MVP intentionally uses **fan-out on read**: the following timeline is assembled by one indexed PostgreSQL query over `posts` and `follows`. It does not execute one query or use one database per followed user.

Do not introduce multiple databases or Twitter-scale fan-out infrastructure without evidence that timeline reads require it. The first optimization should be eliminating N+1 reads when loading media for timeline posts by batch-loading attachments.

If scale later requires precomputed feeds, evolve toward a `timeline_entries(user_id, post_id, created_at)` read model populated asynchronously when posts are created. At very large scale, prefer a hybrid approach: fan out ordinary authors on write, while high-follower accounts are merged into feeds on read to avoid extreme write amplification.

## Architecture notes

`docs/architecture-evolution.md` records the minimal-default/optional-adapter strategy and the boundary that general-purpose search is not a core social capability. PostgreSQL full-text search may still be used by applications or a generic search adapter when useful.

`docs/social-capabilities.md` records planned social-domain evolution, including tree-shaped comments, reactions, private saves/bookmarks, votes, reposts, blocks/mutes, mentions, and notification boundaries.

## Run

```bash
cp .env.example .env
docker compose up -d postgres
cargo run
```

The server applies `migrations/` on startup and listens on `127.0.0.1:8080` by default. JSON timestamps are emitted as RFC 3339 strings.

## Headers

Authenticated endpoints expect UUID values:

```text
X-App-Id: 00000000-0000-0000-0000-000000000001
X-User-Id: 00000000-0000-0000-0000-000000000002
```

Public profile, post, comment-list, and follow-graph reads require `X-App-Id`; `X-User-Id` is optional for those reads and is used when checking owner access to private resources. A present user header is always validated. Mutating endpoints and the personal timeline still require both headers.

## Visibility

Profiles and posts have stable `public | private` visibility with `public` as the default for existing and newly created data.

The minimal policy is intentionally strict and deterministic:

- public resources are readable inside the same app scope;
- private profiles and posts are readable only by their owner;
- comments inherit their post's visibility boundary;
- a user's follow graph can be inspected only when that user's profile is visible to the caller;
- timelines include public followed posts plus the current user's own posts;
- changing a profile to private does not prevent the current user from unfollowing it.

`private` does **not** currently mean "approved followers can read it." Follow requests/approval are a separate future capability and will not be inferred from the existing unilateral `follows` relation. Visibility is a baseline safety policy rather than a feature flag, so configuration cannot accidentally disable privacy and expose data.

## Features

Set `SOCIAL_FEATURES` to a comma-separated subset of the capabilities implemented by this deployment:

```text
profiles,media,posts,comments,follows,chat
```

The resolver models four layers explicitly:

1. **implemented** - capabilities this version of `social-service` actually implements;
2. **deployment-supported** - the deployment maximum selected by `SOCIAL_FEATURES`, including transitive requirements;
3. **app-requested** - the application subset requested from that deployment;
4. **app-effective** - the deterministic closure after required capabilities are enabled.

The current deployment-wide mode treats `SOCIAL_FEATURES` as both the deployment selection and the application request. The resolver already supports a smaller per-app requested subset without changing capability semantics; persistence/configuration of per-app selections can be added later.

Required capabilities are enabled transitively instead of requiring callers to repeat them manually. For example, requesting `comments` yields effective `profiles,posts,comments`. Unknown capabilities, requests outside the deployment-supported maximum, and declared conflicts fail deterministically. Optional relationships such as media attached to posts/chat are represented as integrations, not hard requirements.

`GET /v1/features` preserves the existing `enabled` field and also exposes `implemented`, `deploymentSupported`, `appRequested`, and `effective`. `enabled` is the compatibility alias for the effective capability set.

Feature flags govern behavior, not whether tables or stored data exist. Disabling a capability must not delete its data or change the stable contract. Future advanced implementations should use separate strategy settings so the minimal implementation remains permanently available and richer media, filtering, moderation, storage, and timeline behavior can be enabled without strategy leakage. Add new capability flags only when the capability itself is implemented; do not reserve flags preemptively.

## API

```text
GET    /health
GET    /v1/features
GET    /v1/profiles/:user_id
PUT    /v1/profiles/me
POST   /v1/media
POST   /v1/posts
GET    /v1/posts/:post_id
DELETE /v1/posts/:post_id
GET    /v1/posts/:post_id/comments
POST   /v1/posts/:post_id/comments
PUT    /v1/follows/:user_id
DELETE /v1/follows/:user_id
GET    /v1/follows/:user_id/followers
GET    /v1/follows/:user_id/following
GET    /v1/timeline
POST   /v1/conversations
GET    /v1/conversations
GET    /v1/conversations/:conversation_id/messages
POST   /v1/conversations/:conversation_id/messages
```

Follow graph reads return bounded `FollowEdge` records rather than profile projections. This keeps relationship ownership separate from profile presentation and lets clients batch or compose profile reads explicitly.

The TypeScript client lives in `sdk/typescript`.
