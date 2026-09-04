# Database conventions

This directory contains conventions that arise specifically from relational or other database technologies.

General database rules that are independent of a specific engine use the `DB-*` prefix and live directly in this directory. Engine-specific rules live in child scopes and inherit applicable database-wide rules.

Current child scopes:

```text
databases/
  postgres/   # POSTGRES-*
```

Do not place application ORM conventions here unless the rule is fundamentally a database rule. ORM- or framework-specific behavior should live at its own appropriate technology scope.

## DB-001 — Keep durable records migration-friendly

- Complex durable records should normally include creation/update timestamps and a version when those fields materially help model changes, synchronization, or migration.
- Add history tables or equivalent revision storage when the domain needs old revisions; do not create history machinery speculatively.

## DB-002 — Evolve persistent schemas through explicit migrations

- Production, staging, and other persistent environments change schema through committed migrations rather than implicit startup mutation.
- Disposable local development databases may be recreated or initialized directly from the current model when explicitly configured as disposable.
- Do not replace a required migration with instructions to delete and recreate persistent data.

## DB-003 — Keep test and seed data minimal and deterministic

- Tests create only the records they need through reusable builders/factories and deterministic IDs, clocks, and ordering where relevant.
- Avoid one giant shared mutable seed database as the default test fixture.
- Version larger reference datasets separately when the dataset itself is part of the product or behavior under test.
