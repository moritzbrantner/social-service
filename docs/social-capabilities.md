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

## Saves / bookmarks / stars

A private "star", bookmark, or saved-post action is not the same concept as a public reaction. Model it as a private per-user save/favorite relation. Other users should not infer it from reaction APIs or counts.

If a product later needs a 1-5 star score, model that separately as a **rating**. Do not overload the same `star` concept for both bookmarks and numeric ratings.

## Votes

Reddit-style upvotes/downvotes have ranking and score semantics that differ from likes/emotions. If a product needs voting, add a separate vote capability rather than treating `upvote` and `downvote` as ordinary reactions.

Start with direct PostgreSQL aggregation. More advanced score/hotness/ranking calculations may later be read-model strategies while the individual vote remains authoritative.

## Reposts / shares

A repost/reshare is social content structure, not merely a reaction. If introduced, represent the relationship to the original post explicitly so attribution, deletion, visibility, and counters remain well-defined. External share-sheet behavior belongs to clients and does not require a server-side social capability.

## Blocks and mutes

Blocks and mutes are social-domain relationships and belong here when needed. They must feed a shared visibility/policy boundary so timelines, comments, reactions, profile access, and later discovery do not each reinvent filtering rules.

Keep their first implementation as direct PostgreSQL relationship checks. A policy engine is an optional later strategy.

## Mentions and notifications

Mentions are social relationships in authored content and may be modeled here when a product needs them. The social service can emit domain events such as comment replies, reactions, mentions, or follows.

Notification delivery itself (push, email, SMS, digest scheduling) is a broader infrastructure concern and should remain behind a notification adapter/service rather than becoming part of the social domain model.

## Semantic rule

Do not merge concepts merely because they use similar UI controls:

- like/love/laugh/etc. -> public **reaction**;
- bookmark/private star -> private **save**;
- 1-5 stars -> **rating**;
- upvote/downvote -> **vote**;
- repost -> content relationship;
- share to another app -> client/integration behavior.

Stable domain semantics are more important than minimizing the number of tables or enums.
