---
name: Agent task
about: One PR-sized task for a coding agent (see docs/AGENT_TASKS.md)
title: "<Area>: <what the consumer or operator gains>"
labels: ["agent-task", "spec:draft"]
---

Source: <parent issue #N, `CONTEXT.md` foundation gap, or consumer request link>. Intended implementer: **<Opus|Sol|Sonnet>**. Start after: <#N or "nothing">. One branch (`agent/<topic>`), one PR; follows the `AGENTS.md` **Execution scope** rules.

## Goal

<Two or three sentences: what a consumer, operator or the service can do afterwards.>

## Decisions already made (do not reopen)

- **Rules and numbers:** <limits, defaults, error statuses, …>
- **Contract:** <routes/methods, request/response fields, `SOCIAL_FEATURES` capability changes, SDK methods; or "no public contract change">
- **Schema:** <the one new migration and its tables/indexes; or "no migration">
- **Compatibility:** <what happens to existing data, existing clients and the `enabled` feature alias>
- **Left to the implementer:** <explicitly delegated choices, recorded in the PR>

## Acceptance

- <unit, HTTP and database-backed tests; authorization/visibility decision matrix when access changes>
- <`python3 scripts/load_smoke.py` when relevant (not run by CI)>
- CI green and every Codex review finding addressed or answered.

## Expected changes

- <modules/migrations/`openapi.json`/`sdk/typescript`/docs>

## Out of scope

- <…>
- Convention, tooling, CI, pin and dependency work.

## Parallel work

- <open tasks touching the same modules or adding migrations, or "none">
