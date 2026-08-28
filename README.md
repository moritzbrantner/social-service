# social-service

Reusable modular social backend for Next.js, Expo, and other applications.

## MVP

One deployable Rust/Axum service with internal modules for:

- profiles and avatars/media references;
- posts, comments, follows, and a chronological following timeline;
- conversations and messages with media attachments;
- per-deployment feature flags;
- PostgreSQL persistence scoped by `X-App-Id`;
- a small framework-independent TypeScript SDK.

The service deliberately does **not** own authentication or UI. A trusted application/auth gateway authenticates the request and injects `X-App-Id` and `X-User-Id`. Do not expose the MVP directly to untrusted clients until a production auth adapter is configured.

Media uploads are represented as registered media assets in the MVP. The API already attaches assets to posts and messages; presigned S3-compatible upload support can be added behind the media module without changing those domain relationships.

## Timeline architecture

The MVP intentionally uses **fan-out on read**: the following timeline is assembled by one indexed PostgreSQL query over `posts` and `follows`. It does not execute one query or use one database per followed user.

Do not introduce multiple databases or Twitter-scale fan-out infrastructure without evidence that timeline reads require it. The first optimization should be eliminating N+1 reads when loading media for timeline posts by batch-loading attachments.

If scale later requires precomputed feeds, evolve toward a `timeline_entries(user_id, post_id, created_at)` read model populated asynchronously when posts are created. At very large scale, prefer a hybrid approach: fan out ordinary authors on write, while high-follower accounts are merged into feeds on read to avoid extreme write amplification.

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

## Features

Set `SOCIAL_FEATURES` to a comma-separated subset of:

```text
profiles,media,posts,comments,follows,chat
```

Dependencies are validated at startup. `posts`, `follows`, `media`, and `chat` require `profiles`; `comments` requires `posts`.

`SOCIAL_FEATURES` controls which capabilities exist. Future advanced implementations should use separate strategy settings so the minimal implementation remains the default and richer media, filtering, moderation, storage, and timeline behavior can be enabled per deployment. See `docs/architecture-evolution.md` for the planned switches and adoption rules.

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
GET    /v1/timeline
POST   /v1/conversations
GET    /v1/conversations
GET    /v1/conversations/:conversation_id/messages
POST   /v1/conversations/:conversation_id/messages
```

The TypeScript client lives in `sdk/typescript`.
