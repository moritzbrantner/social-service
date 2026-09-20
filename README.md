# social-service

Reusable modular social backend for Next.js, Expo, and other applications.

## Current baseline

One modular-monolith social service, currently deployed as a single Rust/Axum HTTP process, with internal modules for:

- profiles and avatars/media references;
- posts with explicit audiences, tree-native comments, public reactions, votes, follows, explicit follow requests/approvals, private saves, and a chronological following timeline;
- first-class user blocks and private mutes through a shared safety-policy boundary;
- first-class groups with group-local roles and an optional linked group conversation;
- conversations, messages, media attachments, and message pins;
- platform moderation with audited enforcement and provider-neutral safety signals;
- deterministic capability resolution;
- profile public/private visibility plus explicit post audience policy;
- PostgreSQL persistence scoped by `X-App-Id`;
- framework-independent TypeScript clients for app and trusted-moderation consumers.

The service deliberately does **not** own authentication or UI. A trusted application/auth gateway authenticates the request and injects `X-App-Id` and `X-User-Id`. Do not expose the service directly to untrusted clients until a production auth adapter is configured.

Media uploads are represented as registered media assets in the current baseline. The API already attaches assets to posts and messages; presigned S3-compatible upload support can be added behind the media module without changing those domain relationships.

## Timeline architecture

The current baseline intentionally uses **fan-out on read**: the following timeline is assembled by one indexed PostgreSQL query over `posts`, `follows`, and—only for approved-follower posts—the durable `follow_approvals` relation. It does not execute one query or use one database per followed user. Post audience, block, mute, and moderation policy are applied in the same read boundary.

Do not introduce multiple databases or Twitter-scale fan-out infrastructure without evidence that timeline reads require it. The first optimization should be eliminating N+1 reads when loading media for timeline posts by batch-loading attachments.

If scale later requires precomputed feeds, evolve toward a `timeline_entries(user_id, post_id, created_at)` read model populated asynchronously when posts are created. At very large scale, prefer a hybrid approach: fan out ordinary authors on write, while high-follower accounts are merged into feeds on read to avoid extreme write amplification. Any derived feed must reapply the current post audience, block, mute, and moderation policy before returning content; a stale derived row must never preserve access after approval is revoked.

## Architecture notes

The modular-monolith boundary is about **authority**, not a requirement that all work execute forever in one operating-system process. If asynchronous work later justifies it, an API process and a background worker may be built from this same repository, reuse the same Rust domain modules, and share the same PostgreSQL authority. That remains one social service.

Docker Compose is used for local infrastructure. Today it runs PostgreSQL; future replaceable infrastructure such as object storage, a search engine, or observability components may also be composed locally. Social capabilities themselves remain internal modules rather than per-feature containers.

`CONTEXT.md` is the concise current-state map for capability status, authority boundaries, and known foundation gaps. Detailed documents below own the corresponding semantics and strategy decisions.

`docs/architecture-evolution.md` records the minimal-default/optional-adapter strategy and the boundary that general-purpose search is not a core social capability. PostgreSQL full-text search may still be used by applications or a generic search adapter when useful.

`docs/social-capabilities.md` records social-domain evolution, including post audiences, tree-shaped comments, reactions, follow requests/approvals, private saves/bookmarks, votes, reposts, blocks/mutes, mentions, and notification boundaries.

`docs/groups-and-commands.md` records the group/conversation ownership boundary and the structured command boundary used by voice, text, assistant, and automation consumers.

`docs/moderation-signals.md` records the provider-neutral evidence boundary for classifiers, heuristics, abuse detectors, and human moderation.

## Run

```bash
cp .env.example .env
docker compose up -d postgres
cargo run
```

The server applies `migrations/` on startup and listens on `127.0.0.1:8080` by default. JSON timestamps are emitted as RFC 3339 strings.

The Compose topology intentionally contains infrastructure, not separate containers for posts, comments, follows, groups, chat, moderation, or other social capabilities.

## Headers

Authenticated endpoints expect UUID values:

```text
X-App-Id: 00000000-0000-0000-0000-000000000001
X-User-Id: 00000000-0000-0000-0000-000000000002
```

Public profile, public-post, comment, reaction-summary, and follow-graph reads require `X-App-Id`; `X-User-Id` is optional for those reads and is used when checking owner, post audience, current-user reaction state, and user-safety policy. Approved-follower and owner-only posts return not-found when the viewer does not satisfy their audience. A present user header is always validated. Mutating endpoints, private follow-request/approval and block/mute lists, groups, chat, and the personal timeline require both headers.

## Visibility, post audiences, and user safety

Profiles retain stable `public | private` visibility. A private profile remains readable only by its owner; follow approval does not broaden profile visibility.

Posts have an authoritative `audience`:

- `public` — readable publicly inside the app scope;
- `owner_only` — readable only by the author;
- `approved_followers` — readable by the author and users with a current durable `follow_approvals` relationship to the author.

The existing post `visibility` field remains for compatibility rather than as a second policy authority. `audience=public` projects to `visibility=public`; `owner_only` and `approved_followers` both project to `visibility=private`. Legacy clients that send only `visibility=private` therefore continue to create owner-only posts. Conflicting `visibility` and `audience` inputs are rejected.

The baseline policy is strict and deterministic:

- public profiles and public-audience posts are readable inside the same app scope;
- private profiles and owner-only posts are readable only by their owner;
- approved-follower posts require a current durable approval; a unilateral follow alone never grants access;
- comments inherit their post's current audience boundary;
- reaction reads and writes reapply the target post/comment audience boundary;
- vote reads and writes reapply the same target boundary while keeping score semantics separate from reactions;
- saved-post reads reapply the post's current audience rather than trusting the fact that a save was previously valid;
- a user's follow graph can be inspected only when that user's profile is visible to the caller;
- timelines include visible posts from followed authors plus the current user's own posts; approved-follower posts additionally require durable approval;
- changing a profile to private does not prevent the current user from unfollowing it;
- a unilateral follow, a pending follow request, and an explicit approval are separate relationships;
- accepting a follow request creates the normal follow edge plus a durable approval record;
- unfollowing removes the associated approval and pending same-direction request; the target can explicitly revoke an approval;
- disabling `follow_requests` does not delete stored approvals, but approved-follower posts fail closed to non-owners until that capability is enabled again;
- a block is stored directionally but creates a bilateral visibility/contact boundary between the two users;
- blocking is idempotent and removes existing follow edges, follow approvals, pending follow requests, direct cross-pair reactions, and direct cross-pair votes; unblocking never recreates them;
- blocked users are filtered from profiles, posts, comments, reactions, vote aggregates, follow-graph reads, timelines, and message/pin reads for the affected viewer;
- new conversations cannot include a blocked pair, and an existing two-person conversation becomes non-writable while either direction is blocked;
- blocking does not silently rewrite shared group membership or destroy conversation history;
- a mute is private and directional: it filters the muted user's posts from the muter's timeline and comments from comment lists, but does not hide direct profile/post access, sever follows, or block chat.

Post `visibility=private` does **not** mean "approved followers can read it." It is the compatibility projection for any non-public audience. Only the explicit `audience=approved_followers` policy consults durable approval.

Groups are private membership-scoped resources. Non-members do not receive group metadata. Public groups/discovery are separate semantics and are not inferred from profile visibility or post audiences.

## Features

Set `SOCIAL_FEATURES` to a comma-separated subset of the capabilities implemented by this deployment. The default is:

```text
profiles,media,posts,comments,reactions,votes,follows,follow_requests,saves,blocks,mutes,groups,chat
```

`moderation` remains separately enabled because it introduces trusted staff/service authority rather than ordinary end-user behavior.

Post audience itself is baseline post policy and does not add another capability flag. The `approved_followers` audience composes with the optional `follow_requests` capability because that capability owns the durable approval relation. Creating an approved-follower post therefore requires `follow_requests`; existing such posts fail closed to non-owners when it is disabled.

The resolver models four layers explicitly:

1. **implemented** - capabilities this version of `social-service` actually implements;
2. **deployment-supported** - the deployment maximum selected by `SOCIAL_FEATURES`, including transitive requirements;
3. **app-requested** - the application subset requested from that deployment;
4. **app-effective** - the deterministic closure after required capabilities are enabled.

The current deployment-wide mode treats `SOCIAL_FEATURES` as both the deployment selection and the application request. The resolver already supports a smaller per-app requested subset without changing capability semantics; persistence/configuration of per-app selections can be added later.

Required capabilities are enabled transitively instead of requiring callers to repeat them manually. For example, requesting `comments`, `reactions`, or `votes` yields the required `profiles,posts` capabilities; requesting `follow_requests` yields `profiles,follows,follow_requests`; requesting `blocks` or `mutes` yields the required `profiles` capability without forcing follows/chat; requesting `groups` yields `profiles,groups`, while chat and media remain optional integrations. Comment-target reactions and votes additionally require the `comments` capability at the operation boundary. Unknown capabilities, requests outside the deployment-supported maximum, and declared conflicts fail deterministically.

`GET /v1/features` preserves the existing `enabled` field and also exposes `implemented`, `deploymentSupported`, `appRequested`, and `effective`. `enabled` is the compatibility alias for the effective capability set.

Feature flags govern behavior, not whether tables or stored data exist. Disabling a capability must not delete its data or change the stable contract. Future advanced implementations should use separate strategy settings so the minimal implementation remains permanently available. Add new capability flags only when the capability itself is implemented; do not reserve flags preemptively.

## Comments, reactions, and votes

Comments are tree-native. Root comments are paged separately from direct replies, parent relationships are immutable and constrained to the same app/post, and clients can recursively expand branches without requiring the server to materialize an unbounded tree. Deleting a leaf removes it; deleting a comment with descendants preserves an empty tombstone so the branch remains structurally valid.

Comments always inherit the current post audience. Losing approved-follower access hides the comment tree through the same post-access boundary even if the viewer created comments there previously.

Public reactions are normalized PostgreSQL relations over visible posts and comments. The initial allowed reaction type is `like`. PUT and DELETE are idempotent, aggregate counts are derived from authoritative rows, and the current user's own reaction state is returned separately. Reaction DELETE deliberately remains visibility-independent so a user can clean up a stale relation after losing access.

Votes are a separate `votes` capability rather than reaction types. Each user has at most one `up | down` vote per visible post or comment. Repeating the same vote is idempotent; changing direction replaces the existing vote. Summaries derive `upvotes`, `downvotes`, and `score = upvotes - downvotes` directly from PostgreSQL while applying current moderation and block policy. Vote removal remains visibility-independent so stale votes can be cleaned up after access is revoked. Ranking/hotness remains a later read-model concern rather than part of vote authority.

## Follow requests and approvals

`follows` remains the ordinary directional graph. `follow_requests` adds a separate consent workflow rather than changing what a follow means.

A requester can create or cancel a pending request; the target can accept or decline it. Acceptance atomically creates the directional follow and a durable `follow_approvals` row. Approval is deliberately distinguishable from a public unilateral follow, so post audience authorization never has to guess from the follow graph. Repeated request/cancel/accept/decline/revoke operations are idempotent.

Approved followers are a private management surface for the target. Revoking approval removes the approved follow edge; an ordinary unfollow also removes approval through the database relationship. Blocking removes both pending requests and approved follow relationships under the same user-pair lock.

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
DELETE /v1/posts/:post_id/comments/:comment_id
GET    /v1/posts/:post_id/comments/:comment_id/replies
POST   /v1/posts/:post_id/comments/:comment_id/replies
GET    /v1/reactions/:target_type/:target_id
PUT    /v1/reactions/:target_type/:target_id/:reaction_type
DELETE /v1/reactions/:target_type/:target_id/:reaction_type
GET    /v1/votes/:target_type/:target_id
PUT    /v1/votes/:target_type/:target_id/:vote_value
DELETE /v1/votes/:target_type/:target_id
PUT    /v1/posts/:post_id/save
DELETE /v1/posts/:post_id/save
GET    /v1/saved-posts
PUT    /v1/follows/:user_id
DELETE /v1/follows/:user_id
GET    /v1/follows/:user_id/followers
GET    /v1/follows/:user_id/following
GET    /v1/follow-requests/incoming
GET    /v1/follow-requests/outgoing
PUT    /v1/follow-requests/outgoing/:user_id
DELETE /v1/follow-requests/outgoing/:user_id
DELETE /v1/follow-requests/incoming/:user_id
PUT    /v1/follow-requests/incoming/:user_id/accept
GET    /v1/follow-approvals
DELETE /v1/follow-approvals/:user_id
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

`POST /v1/posts` accepts the optional `audience` field (`public`, `owner_only`, or `approved_followers`). Existing `visibility` input remains supported for compatibility. Ordinary `Post` responses include both the authoritative `audience` and the compatibility `visibility` projection.

Follow graph reads return bounded `FollowEdge` records rather than profile projections. Pending follow-request and approved-follower lists are private to the current user and return relationship records rather than profile projections. Block/mute lists return only the current user's own `UserSafetyRelationship` records; there is no public "who blocked me" surface.

The TypeScript client lives in `sdk/typescript` and exposes the matching post-audience, reaction, and vote contracts.
