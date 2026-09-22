# social-service context

## Purpose

`social-service` is a reusable social-domain backend for Next.js, Expo, and other applications. It is intentionally one modular-monolith social authority backed by PostgreSQL. The current baseline runs as one Rust/Axum HTTP process, but runtime topology is not an authority boundary.

The service owns social-domain data, invariants, visibility, safety, moderation policy, and app-scoped persistence. Authentication and product UI remain external boundaries.

## Authority boundaries

- A trusted application or authentication gateway supplies app/user identity. The service does not own credentials or login flows.
- Rust and PostgreSQL own social-domain truth. Clients and SDKs do not infer authorization, visibility, membership, moderation, or relationship state.
- Persisted social data is scoped by `app_id`.
- Groups own durable group membership and group-local roles; chat owns conversations and messages. Their association is explicit.
- User blocks/mutes are ordinary social safety relationships. Platform moderation is a separate authority plane with trusted capabilities, audited actions, and app-scoped state.
- Post `audience` is authoritative for post access. Legacy post `visibility` is only a compatibility projection.
- General-purpose search, authentication, notification delivery, speech/NLU, and presentation components stay outside this repository. They may consume stable social-service APIs or derived events/projections without becoming social authority.
- A future API process and worker process may both be built from this repository and share the same domain modules/PostgreSQL authority. Separating execution for background work does not create a second social service or move domain ownership.
- Docker Compose is the local infrastructure topology. PostgreSQL and future replaceable infrastructure adapters may run there; social capabilities must not be split into per-feature containers merely because Compose can host them.

## Implemented baseline

The ordinary default capability set is:

`profiles,media,posts,comments,reactions,votes,follows,follow_requests,saves,blocks,mutes,groups,chat`

`moderation` is implemented but remains separately enabled because it introduces trusted staff/service authority.

The current baseline includes:

- public/private profiles;
- public, owner-only, and approved-follower post audiences;
- tree-native comments with tombstones and bounded reply loading;
- normalized likes and separate up/down votes;
- directional follows plus explicit follow requests and durable approvals;
- private saved posts;
- bilateral block policy and private directional mutes;
- private groups with owner/admin/member authority and linked group chat;
- conversations, messages, media attachments, and message pins;
- reports, moderation cases, content/account enforcement, scoped restrictions, role bindings, audit events, group moderation, and provider-neutral moderation signals;
- deterministic capability resolution and a framework-independent TypeScript SDK.

## Current implementation strategies

- Timeline delivery uses indexed PostgreSQL fan-out on read, with post media batch-loaded for each bounded result set.
- Chat collection reads batch-load conversation membership and message media rather than issuing one follow-up query per item.
- Media is a logical registered asset referencing externally stored bytes.
- Collection reads use bounded limits but do not yet have continuation cursors.
- Visibility, audience, block/mute, group, and moderation policy are enforced against authoritative PostgreSQL state through shared Rust/SQL boundaries.
- Notification delivery and generic search are external concerns; there is not yet a durable general domain-event/outbox contract for them.
- The current runtime has one HTTP process. If asynchronous workloads justify it, prefer a same-repository worker consuming transactional outbox work before considering independently authoritative services.

## Foundation gaps

These are architecture/infrastructure gaps rather than missing social semantics:

- continuation/cursor pagination for unbounded collections;
- eliminating remaining N+1 collection materialization, especially group materialization;
- a transactional domain-event/outbox boundary for notifications, search projections, and future derived feeds;
- an optional same-repository worker runtime once outbox-backed background work exists; this is execution separation, not domain decomposition;
- managed media upload/inspection/lifecycle adapters behind the existing logical media model;
- a machine-readable public HTTP contract and executable server/SDK compatibility checking;
- production service hardening such as configurable pool/resource limits, timeouts/cancellation, request correlation, and operational metrics;
- persisted per-app capability selection if deployments need different app subsets.

Do not introduce caches, queues, precomputed feeds, microservices, or advanced media processing solely because these extension points exist. Add them when a measured product, scale, reliability, or operational need justifies the extra machinery.

## Planned social semantics

The documented social backlog includes reposts/quote-post structure, mentions plus domain events, consentful group invitations/join requests, and optional richer comment ordering/reaction/rating behavior. These should reuse the existing visibility, safety, moderation, app-scope, and SDK boundaries rather than creating parallel policy paths.

## Repository map

- `README.md`: public usage and API overview.
- `AGENTS.md`: repository-specific implementation and authority rules.
- `docs/social-capabilities.md`: implemented and planned social semantics.
- `docs/architecture-evolution.md`: optional strategy evolution and infrastructure boundaries.
- `docs/groups-and-commands.md`: group/chat ownership and structured command boundary.
- `docs/moderation-signals.md`: non-authoritative safety-signal boundary.
- `.coding-tooling.json` and `.conventions/`: deterministic repository validation contract.

Open issues should describe genuinely outstanding work. Completed foundation epics should be closed rather than retained as an alternative roadmap.
