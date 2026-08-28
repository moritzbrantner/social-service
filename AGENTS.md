# AGENTS.md

Follow the live `moritzbrantner/coding-agent-conventions` rules applicable to Rust, PostgreSQL, Docker, and TypeScript.

Repository-specific decisions:

- Keep this a modular monolith and one deployable social service. Do not split capabilities into microservices without an explicit architectural decision.
- Authentication remains an external boundary; this repository owns social identities/data, not credential management.
- Keep reusable visual components in the `ui` repository. This repository may own transport/domain SDK code, not presentation components.
- Preserve `app_id` scoping on persisted social data.
- Keep the timeline fan-out-on-read for the MVP. Optimize the current read path first, especially by batch-loading post media instead of N+1 queries. Only introduce a precomputed `timeline_entries` read model when measurements justify it; if very high-follower accounts become relevant, use a hybrid fan-out strategy rather than blindly copying every post to every follower.
