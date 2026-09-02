# HTTP validation test generator

This repository-local generator materializes router-level tests for a small committed set of request-validation contracts that fail before PostgreSQL is touched.

The generator intentionally accepts a bounded `case` enum rather than arbitrary method/path/body/error strings. The semantic request fixture lives in `tests/support/mod.rs`; generation only selects an already-reviewed case and creates the standard test wrapper.

```bash
coding-tooling generate http-validation-test --input case=profile-empty-display-name
coding-tooling generate http-validation-test --input case=media-empty-url
coding-tooling generate http-validation-test --input case=post-empty-body
coding-tooling generate http-validation-test --input case=comment-empty-body
coding-tooling generate http-validation-test --input case=conversation-single-member
```

Do not add a case merely because an endpoint accepts JSON. A case belongs here only when its expected rejection is deterministic through the public router and occurs before persistence or other external state is required. Database-backed behavior belongs in the repository's explicit integration/database test layer.
