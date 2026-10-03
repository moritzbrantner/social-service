# Public HTTP operation evidence

`openapi.json` is the source-owned operation inventory for the current Axum app.
It declares 74 method/path identities. It is intentionally an inventory rather
than a complete request/response schema or a server/SDK compatibility guarantee.
`tests/http_inventory.rs` rejects drift from the current literal registrations
and probes every declaration through the public composed `app`, using Axum's
matched path and method response rather than importing private handlers.
Dynamic registration needs an explicit adapter before this guard can accept it.
These structural probes do not count as behavioral evidence.

`.coding-tooling.contracts.json` explicitly associates 11 native cases with 10
operations and owner-chosen behavior dimensions. Each case ID is
`integration_binary::exact_native_test_name`; the `reason` points to its test
artifact. The existing assertions exercise the public app. This declaration
does not infer meaning from test names or synthesize assertions.

| Operations | Declared dimensions | Native test artifacts |
| --- | --- | --- |
| `GET /health` | availability | `tests/http_smoke.rs` |
| `GET /ready` | availability, cancellation/timeout | `tests/http_smoke.rs` |
| `GET /v1/features` | success (effective feature document) | `tests/http_smoke.rs` |
| `GET /v1/profiles/{user_id}`, `GET /v1/posts/{post_id}` | availability (disabled feature) | `tests/get_profile_feature_gate.rs`, `tests/get_post_feature_gate.rs` |
| `PUT /v1/profiles/me`, `POST /v1/media`, `POST /v1/posts`, `POST /v1/posts/{post_id}/comments`, `POST /v1/conversations` | validation | `tests/*_validation.rs` |

The remaining 64 operations have no declared behavioral mapping. Authorization,
persistence, ownership/not-found, concurrency and idempotency are also unverified
dimensions here, including on mapped routes. Some have database-backed tests;
their results are not attributed to operations by this pilot. A disabled-feature
case or validation case is not evidence for those other dimensions. Existing
database isolation and full validation remain the authority for their own tests.

## Repeatable commands

Run these from the repository root after acquiring its locked dependencies:

```sh
cargo test --locked --test http_inventory --example http_case_evidence
cargo run --locked --example http_case_evidence
coding-tooling findings --json
coding-tooling contract discover --json
coding-tooling contract verify --json --report .artifacts/coding-tooling/http-contract.json
```

The root `test` capability runs `examples/http_case_evidence.rs`; `test:unit`,
`test:integration` and the full tier retain their broader meanings. Overrides use
the component path `.` so a worktree or renamed checkout behaves identically.

The bridge invokes native libtest for explicitly declared binaries with a
300-second bound per invocation. It preserves passed, failed and ignored
outcomes, rejects empty, incomplete, duplicate or unknown native reports, and
does not treat a zero-case run as success. Logs stay under ignored `.artifacts/`.
When coding-tooling supplies the exact-run environment, the bridge writes a
fresh artifact with those run, revision, component and capability identities.
Direct invocation validates the cases without producing reusable evidence.
Missing or ignored case IDs remain unverified; capability success alone cannot
verify them. CI uses the immutable coding-tooling action to repeat exact-run
verification and retain the report.

`http-route-contract-evidence` findings identify declaration debt. A mapping
removes that advisory finding but still needs a passed current case to become
verified. Neither source reachability nor the inventory probe removes that debt.
