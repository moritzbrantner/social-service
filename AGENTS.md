# AGENTS.md

Follow the live `moritzbrantner/coding-agent-conventions` rules applicable to Rust, PostgreSQL, Docker, and TypeScript.

Repository-specific decisions:

- Keep this a modular monolith and one deployable social service. Do not split capabilities into microservices without an explicit architectural decision.
- Authentication remains an external boundary; this repository owns social identities/data, not credential management.
- Keep reusable visual components in the `ui` repository. This repository may own transport/domain SDK code, not presentation components.
- Preserve `app_id` scoping on persisted social data.
