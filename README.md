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

## Run

```bash
cp .env.example .env
docker compose up -d postgres
cargo run
```

The server applies `migrations/` on startup and listens on `127.0.0.1:8080` by default.

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
