# HTTP feature-gate test generator

This repository-local generator creates a real Axum integration test for the common bodyless endpoint contract: when the endpoint's capability is disabled, the public router returns the structured `feature_disabled` 404 response before authentication or persistence is touched.

The generator intentionally supports only `GET`, bodyless `PUT`, and `DELETE` endpoints. Routes with a JSON extractor need a valid request body before the handler runs, so they should not be forced through this scaffold unless a separate deterministic body-aware pattern is defined.

Example:

```bash
coding-tooling generate http-feature-gate-test \
  --input name=get-profile \
  --input method=GET \
  --input path=/v1/profiles/00000000-0000-0000-0000-000000000010 \
  --input feature=profiles
```

Generated files are one-shot scaffolds. After creation they are ordinary repository-owned tests.
