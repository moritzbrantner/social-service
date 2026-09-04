# PostgreSQL conventions

PostgreSQL conventions specialize the parent database conventions.

Rules in this directory use the `POSTGRES-*` prefix and cover PostgreSQL-specific schema design, data types, indexes, constraints, SQL, migrations, transactions, locking, extensions, performance, and operational behavior when those choices affect application development.

Applicable scope:

```text
technologies/databases/postgres/
        -> technologies/databases/
        -> general conventions
        -> principles
```

Keep rules at the parent database level when they are not actually PostgreSQL-specific.
