# social-service

Reusable modular social backend for Next.js, Expo, and other applications.

## MVP

One deployable Rust/Axum service with internal modules for:

- profiles and avatars/media references;
- posts, comments, follows, private saves, and a chronological following timeline;
- first-class user blocks and private mutes through a shared safety-policy boundary;
- first-class groups with group-local roles and an optional linked group conversation;
- conversations, messages, media attachments, and message pins;
- platform moderation with audited enforcement and provider-neutral safety signals;
- deterministic capability resolution;
- shared public/private visibility policy for profiles and posts;
- PostgreSQL persistence scoped by `X-App-Id`;
- framework-independent TypeScript clients for app and trusted-moderation consumers.

The service deliberately does **not** own authentication or UI. A trusted application/auth gateway authenticates the request and injects `X-App-Id` and `X-User-Id`. Do not expose the MVP directly to untrusted clients until a production auth adapter is configured.

Media uploads are represented as registered media assets in the MVP. The API already attaches assets to posts and messages; presigned S3-compatible upload support can be added behind the media module without changing those domain relationships.

## Timeline architecture

The MVP intentionally uses **fan-out on read**: the following timeline is assembled by one indexed PostgreSQL query over `posts` and `follows`. It does not execute one query or use one database per followed user. Block and mute policy is applied in the same read boundary.

Do not introduce multiple databases or Twitter-scale fan-out infrastructure without evidence that timeline reads require it. The first optimization should be eliminating N+1 reads when loading media for timeline posts by batch-loading attachments.

If scale later requires precomputed feeds, evolve toward a `timeline_entries(user_id, post_id, created_at)` read model populated asynchronously when posts are created. At very large scale, prefer a hybrid approach: fan out ordinary authors on write, while high-follower accounts are merged into feeds on read to avoid extreme write amplification. Any derived feed must preserve the same visibility, block, mute, and moderation policy before returning content.

## Architecture notes

`docs/architecture-evolution.md` records the minimal-default/optional-adapter strategy and the boundary that general-purpose search is not a core social capability. PostgreSQL full-text search may still be used by applications or a generic search adapter when useful.

`docs/social-capabilities.md` records social-domain evolution, including tree-shaped comments, reactions, private saves/bookmarks, votes, reposts, blocks/mutes, mentions, and notification boundaries.

`docs/groups-and-commands.md` records the group/conversation ownership boundary and the structured command boundary used by voice, text, assistant, and automation consumers.

`docs/moderation-signals.md` records the provider-neutral evidence boundary for classifiers, heuristics, abuse detectors, and human moderation.

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

Public profile, post, comment-list, and follow-graph reads require `X-App-Id`; `X-User-Id` is optional for those reads and is used when checking owner and user-safety policy. A present user header is always validated. Mutating endpoints, private block/mute lists, groups, chat, and the personal timeline require both headers.

## Visibility and user safety

Profiles and posts have stable `public | private` visibility with `public` as the default for existing and newly created data.

The baseline policy is strict and deterministic:

- public resources are readable inside the same app scope;
- private profiles and posts are readable only by their owner;
- comments inherit their post's visibility boundary;
- a user's follow graph can be inspected only when that user's profile is visible to the caller;
- timelines include public followed posts plus the current user's own posts;
- changing a profile to private does not prevent the current user from unfollowing it;
- a block is stored directionally but creates a bilateral visibility/contact boundary between the two users;
- blocking is idempotent and removes existing follow edges in both directions; unblocking never recreates them;
- blocked users are filtered from profiles, posts, comments, follow-graph reads, timelines, and message/pin reads for the affected viewer;
- new conversations cannot include a blocked pair, and an existing two-person conversation becomes non-writable while either direction is blocked;
- blocking does not silently rewrite shared group membership or destroy conversation history;
- a mute is private and directional: it filters the muted user's posts from the muter's timeline and comments from comment lists, but does not hide direct profile/post access, sever follows, or block chat.

`private` does **not** mean "approved followers can read it." Follow requests/approval are a separate future capability and are not inferred from the unilateral `follows` relation. Visibility and block enforcement are safety policies, not presentation hints.

Groups are private membership-scoped resources. Non-members do not receive group metadata. Public groups/discovery are separate semantics and are not inferred from profile/post visibility.

## Features

Set `SOCIAL_FEATURES` to a comma-separated subset of the capabilities implemented by this deployment. The default is:

```text
profiles,media,posts,comments,follows,saves,blocks,mutes,groups,chat
```

`moderation` remains separately enabled because it introduces trusted staff/service authority rather than ordinary end-user behavior.

The resolver models four layers explicitly:

1. **implemented** - capabilities this version of `social-service` actually implements;
2. **deployment-supported** - the deployment maximum selected by `SOCIAL_FEATURES`, including transitive requirements;
3. **app-requested** - the application subset requested from that deployment;
4. **app-effective** - the deterministic closure after required capabilities are enabled.

The current deployment-wide mode treats `SOCIAL_FEATURES` as both the deployment selection and the application request. The resolver already supports a smaller per-app requested subset without changing capability semantics; persistence/configuration of per-app selections can be added later.

Required capabilities are enabled transitively instead of requiring callers to repeat them manually. For example, requesting `comments` yields effective `profiles,posts,comments`; requesting `blocks` or `mutes` yields the required `profiles` capability without forcing follows/chat; requesting `groups` yields `profiles,groups`, while chat and media remain optional integrations. Unknown capabilities, requests outside the deployment-supported maximum, and declared conflicts fail deterministically.

`GET /v1/features` preserves the existing `enabled` field and also exposes `implemented`, `deploymentSupported`, `appRequested`, and `effective`. `enabled` is the compatibility alias for the effective capability set.

Feature flags govern behavior, not whether tables or stored data exist. Disabling a capability must not delete its data or change the stable contract. Future advanced implementations should use separate strategy settings so the minimal implementation remains permanently available. Add new capability flags only when the capability itself is implemented; do not reserve flags preemptively.

## Groups

Groups own durable membership and group-local roles; conversations own messages. The primary group chat is linked through an association rather than making a conversation double as the group record.

The creator is the owner. Owners and admins can update group metadata and add members. Owners alone manage roles and ownership transfer; admins can remove ordinary members. Owners must transfer ownership before leaving. Creating the primary chat is idempotent, and linked conversation membership follows current group membership without deleting historical messages. Joins, leaves, removals, and role changes are retained in an append-only membership event log.

When moderation is enabled, suspended or banned accounts cannot mutate groups and unavailable accounts cannot be added. Group-local roles remain separate from platform-wide moderation authority.

The TypeScript SDK exposes a `GroupOperation` union and `executeGroupOperation` for already-resolved commands. Speech recognition, natural-language parsing, contact resolution, and assistant behavior stay outside this service.

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
PUT    /v1/posts/:post_id/save
DELETE /v1/posts/:post_id/save
GET    /v1/saved-posts
PUT    /v1/follows/:user_id
DELETE /v1/follows/:user_id
GET    /v1/follows/:user_id/followers
GET    /v1/follows/:user_id/following
GET    /v1/blocks
PUT    /v1/blocks/:user_id
DELETE /v1/blocks/:user_id
GET    /v1/mutes
PUT    /v1/mutes/:user_id
DELETE /v1/mutes/:user_id
GET    /v1/timeline
POST   /v1/groups
GET    /v1/groups
GET    /v1/groups/:group_id
PUT    /v1/groups/:group_id
PUT    /v1/groups/:group_id/members/:user_id
DELETE /v1/groups/:group_id/members/:user_id
PUT    /v1/groups/:group_id/members/:user_id/role
POST   /v1/groups/:group_id/leave
POST   /v1/groups/:group_id/chat
POST   /v1/conversations
GET    /v1/conversations
GET    /v1/conversations/:conversation_id/messages
POST   /v1/conversations/:conversation_id/messages
GET    /v1/conversations/:conversation_id/pins
PUT    /v1/conversations/:conversation_id/pins/:message_id
DELETE /v1/conversations/:conversation_id/pins/:message_id
POST   /v1/reports
GET    /v1/moderation/me
GET    /v1/moderation/cases
PUT    /v1/moderation/cases/:case_id
GET    /v1/moderation/signals
POST   /v1/moderation/signals
GET    /v1/moderation/content/:target_type/:target_id
PUT    /v1/moderation/content/:target_type/:target_id
GET    /v1/moderation/users/:user_id
PUT    /v1/moderation/users/:user_id
GET    /v1/moderation/audit
```

Follow graph reads return bounded `FollowEdge` records rather than profile projections. Block/mute lists return only the current user's own `UserSafetyRelationship` records; there is no public "who blocked me" surface.

The TypeScript client lives in `sdk/typescript`.