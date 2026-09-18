# Social capabilities

This document records social-domain capabilities that may be added progressively. A capability should only enter `SOCIAL_FEATURES` when it is implemented and has genuinely social semantics. Keep the smallest useful implementation permanently available; add richer behavior later behind compatible configuration or adapters.

## Comments

Comments should become tree-native rather than remaining flat.

### Minimal model

- Add nullable `parent_comment_id` to comments.
- `NULL` means a root comment; otherwise the parent must belong to the same `app_id` and `post_id`.
- A comment's parent is immutable after creation. This keeps the structure acyclic without needing a general graph/cycle-management subsystem.
- Keep chronological ordering as the simple/default ordering.
- Preserve a deleted comment as a tombstone when it has descendants so deleting a parent does not destroy an entire discussion branch.
- Reuse the existing comment identity/body/author/version model; threading is structure around the same logical `Comment` domain object.

### Minimal loading strategy

Do not require loading an arbitrarily large recursive tree in one request. The simple API can page root comments and fetch/page replies by `parent_comment_id`. Clients may recursively expand branches. More sophisticated subtree materialization or ranking can be added later without changing comment identity.

### Later enhancements

Possible later additions include configurable sorting (`new`, `old`, `top`, `best`), collapsed/deep-thread summaries, per-comment media attachments using the existing logical media boundary, moderation state, and denormalized reply/reaction counts. These are enhancements, not prerequisites for tree-shaped comments.

## Reactions

Public reactions represent a user's lightweight response to a post or comment.

The minimal reaction can be `like`. The model should leave room for an application-defined allowed set such as `like`, `love`, `laugh`, `sad`, or `angry` without making emoji/provider details part of the core domain. A deployment may choose whether a user has one active reaction per target or multiple kinds only when that behavior is actually needed.

Keep simple counts as PostgreSQL aggregates first. Denormalized counters or cached aggregates are later read optimizations and must not become the source of truth.

## Follow requests and approvals

`follow_requests` is the explicit consent capability layered beside the ordinary directional `follows` graph. A pending request and a follow edge are different facts, and an accepted approval remains a separately stored fact rather than being inferred from the presence of a follow.

The minimal implementation uses app-scoped PostgreSQL `follow_requests` and `follow_approvals` relations. Request/cancel/accept/decline/revoke operations are idempotent. Acceptance atomically creates the normal requester-to-target follow edge and a durable approval row. Existing public unilateral follows do not count as approval, but a user who already follows can still request explicit approval.

Approval is tied to the current follow edge: unfollowing cascades the approval away and cancels any same-direction pending request, while the target may explicitly revoke approval, which removes the approved follow edge. Blocking uses the same user-pair serialization boundary, removes pending requests in both directions, and removes approvals through follow-edge cleanup. Unblocking never reconstructs requests, approvals, or follows.

Approval does **not** redefine profile privacy. Private profiles remain owner-only. Instead, post audiences may explicitly opt into approved-follower access as a separate post-level policy.

## Post audiences

Posts use an explicit audience policy with three stable values:

- `public` — readable without a user identity inside the app scope, subject to block/moderation policy where a viewer exists;
- `owner_only` — readable only by the author;
- `approved_followers` — readable by the author and users with a current durable `follow_approvals` relationship to that author.

The ordinary unilateral `follows` relation is never enough to satisfy `approved_followers`. Access is derived only from the durable approval relation created by the follow-request flow. Revoking approval, unfollowing, or blocking therefore removes audience access immediately without rewriting the post.

The existing post `visibility` field is retained as a compatibility projection, not as a second policy authority. `public` audience projects to `visibility=public`; both `owner_only` and `approved_followers` project to `visibility=private`. Legacy create-post requests containing only `visibility=private` continue to mean owner-only. Conflicting `visibility` and `audience` inputs are rejected.

Approved-follower reads fail closed when the `follow_requests` capability is disabled, while the post author always retains access. Every derived surface that exposes post content—timeline, comments, reactions, votes, and saved-post reads—must reapply the current audience relation rather than trusting stale cached visibility or a previously valid save/reaction/vote.

Profile `public | private` visibility remains unchanged and independent from post audiences.

## Saves / bookmarks / stars

A private "star", bookmark, or saved-post action is not the same concept as a public reaction. Model it as a private per-user save/favorite relation. Other users should not infer it from reaction APIs or counts.

The minimal post-save capability is implemented as `saves`. Saving and unsaving are idempotent. Saved-post reads are private to the current user, retain the time the post was saved, and reapply the post's current audience, visibility-compatibility, moderation, and safety boundaries before returning content. Removing a save does not require the post to remain visible, so a user can always clean up a stale private relation.

If a product later needs a 1-5 star score, model that separately as a **rating**. Do not overload the same `star` concept for both bookmarks and numeric ratings.

## Message pins

Conversation message pins are shared chat state, not private favorites. The minimal implementation keeps pins inside the existing `chat` capability: current conversation members can pin or unpin a message, and current members can list the pinned messages. Pinning and unpinning are idempotent and do not reorder the conversation list.

Pinned-message reads reapply message/account moderation and user-block boundaries. The database relation includes both conversation and message identity so a message cannot be pinned into a different conversation. More restrictive pin authority for particular products or linked group chats can be added later without changing message identity or turning pins into reactions.

## Votes

Votes are implemented as a separate `votes` capability because up/down score semantics differ from likes or emotional reactions. The initial target set is posts and comments.

Each user has one authoritative vote per target: `up` or `down`. Repeating the same vote is idempotent, changing direction replaces the existing vote, and removing a vote is allowed even after the target becomes invisible so stale relations can be cleaned up.

Vote reads and writes reuse the target's current post/comment audience, moderation, and block boundary. Aggregates are derived directly from PostgreSQL as upvotes, downvotes, and `score = upvotes - downvotes`; blocked or moderation-unavailable voters do not contribute to the affected viewer's summary. Physical target deletion cascades vote rows, while blocking removes direct cross-pair votes without deleting unrelated votes.

More advanced score decay, hotness, controversy, ranking, or denormalized counters remain optional read-model strategies. The individual vote row stays authoritative.

## Reposts / shares

A repost/reshare is social content structure, not merely a reaction. If introduced, represent the relationship to the original post explicitly so attribution, deletion, visibility, and counters remain well-defined. External share-sheet behavior belongs to clients and does not require a server-side social capability.

## Blocks and mutes

`blocks` and `mutes` are implemented first-class capabilities backed by app-scoped PostgreSQL relationships and shared policy predicates. They remain distinct from follows, follow requests/approvals, moderation, group roles, and each other.

A block is stored as a directional user action, but enforcement is intentionally bilateral between the two users. Profile/post access, follow operations and follow-graph reads, timelines, comment lists, and message/pin reads apply the same block predicate. Creating a conversation containing a blocked pair is rejected. Existing non-group two-person conversations become non-writable while a block exists, while history is retained. Group-linked conversations keep group semantics even when membership later shrinks to two people.

Blocking removes existing follow edges in both directions and pending follow requests in both directions in the same user-pair transaction; approval rows disappear with their approved follow edges. Direct reactions and votes between the blocked pair and each other's authored post/comment content are removed, while unrelated feedback remains intact. Unblocking is idempotent and never reconstructs those social relationships. Blocking does not silently remove either user from a shared group or destroy conversation history. The public API exposes only the current user's outgoing block list; it does not provide a "who blocked me" oracle.

A mute is directional and private. It filters the muted user's authored content from the muter's derived timeline and comment-list views, but it does not sever follows, hide direct profile/post access, prevent chat, affect the muted person's view, or grant moderation authority. Unmuting simply restores those derived reads.

The first implementation deliberately uses direct PostgreSQL relationship checks. If scale later justifies a policy cache or derived graph, it must reproduce the same semantics and app isolation rather than becoming a second source of truth.

## Mentions and notifications

Mentions are social relationships in authored content and may be modeled here when a product needs them. The social service can emit domain events such as comment replies, reactions, mentions, or follows.

Notification delivery itself (push, email, SMS, digest scheduling) is a broader infrastructure concern and should remain behind a notification adapter/service rather than becoming part of the social domain model.

## Semantic rule

Do not merge concepts merely because they use similar UI controls:

- like/love/laugh/etc. -> public **reaction**;
- unilateral follow -> directional **follow edge**;
- accepted follow request -> separate durable **follow approval** plus its follow edge;
- approved-followers post -> explicit **post audience** consulting durable approval;
- bookmark/private star -> private **save**;
- 1-5 stars -> **rating**;
- upvote/downvote -> **vote**;
- block -> bilateral safety/contact policy from a directional user action;
- mute -> private viewer-side filtering;
- repost -> content relationship;
- share to another app -> client/integration behavior.

Stable domain semantics are more important than minimizing the number of tables or enums.
