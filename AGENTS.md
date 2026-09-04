# AGENTS.md

Follow the live `moritzbrantner/coding-agent-conventions` rules applicable to Rust, PostgreSQL, Docker, and TypeScript.

Repository-specific decisions:

- Keep this a modular monolith and one deployable social service. Do not split capabilities into microservices without an explicit architectural decision.
- Authentication remains an external boundary; this repository owns social identities/data, not credential management.
- Keep reusable visual components in the `ui` repository. This repository may own transport/domain SDK code, not presentation components.
- Preserve `app_id` scoping on persisted social data.
- Keep the timeline fan-out-on-read for the MVP. Optimize the current read path first, especially by batch-loading post media instead of N+1 queries. Only introduce a precomputed `timeline_entries` read model when measurements justify it; if very high-follower accounts become relevant, use a hybrid fan-out strategy rather than blindly copying every post to every follower.
- Preserve the minimal implementation of each capability as the default. `SOCIAL_FEATURES` controls capability availability; richer implementation choices belong behind separate validated strategy settings. Do not replace the simple mode when adding an advanced mode, and do not implement planned advanced strategies without a concrete product, scale, safety, or operational need.
- Model capability resolution as implemented -> deployment-supported -> app-requested -> app-effective. Resolve transitive hard requirements deterministically, keep optional integrations distinct from requirements, reject conflicts/unsupported requests, and register a capability only after its behavior exists.
- Feature flags govern behavior, not schema/data existence. Disabling a capability must not delete its data or silently change stored contracts. Keep the existing `enabled` capability response as an effective-set compatibility alias unless a deliberate API version removes it.
- Keep simple modes as permanent first-class fallbacks. Advanced implementations must be additive and must not make the simple implementation impossible to run or test.
- Do not leak implementation strategy into core domain models or public contracts. Posts, messages, media, search results, and other domain concepts should reference logical domain objects; storage backends, media variants/codecs, search engines/index formats, moderation providers, queues, and similar infrastructure details belong behind adapters.
- Add sophistication progressively through stable ports/adapters around the domain. Prefer a minimal built-in adapter first, then add optional enhanced adapters without requiring callers to understand which implementation is active.
- Treat public/private visibility as a baseline safety policy, not a capability flag. The minimal private rule is owner-only until an explicit follow-request/approval capability defines broader access. Apply visibility before returning profiles, posts, derived comments/timelines, follow graphs, or future discovery results; prefer not-found over revealing inaccessible resource existence.
- Keep follows, connections, blocks, and mutes as distinct social relationships. Do not infer connection/approval/block/mute semantics from a unilateral follow edge.
- Treat general-purpose search as external/generic infrastructure, not a social capability. Do not add `search` to `SOCIAL_FEATURES` merely to search posts, comments, or profiles. PostgreSQL FTS may be used as a simple implementation by an application or generic search adapter; richer indexes remain derived read models and must not bypass social visibility/policy.
- Keep comments tree-capable when the comment model is evolved: nullable immutable parent relationship, same post/app, and tombstones for deleted parents with descendants. Do not require loading an unbounded recursive tree in one request.
- Keep reactions, private saves/bookmarks, ratings, votes, reposts, and external shares semantically separate even when their UI controls look similar. See `docs/social-capabilities.md`.
- Keep advanced media/storage/filtering/moderation/timeline planning aligned with `docs/architecture-evolution.md`. Public post/message/media contracts should remain stable across strategy choices where practical.
