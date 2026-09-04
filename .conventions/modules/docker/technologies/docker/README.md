# Docker conventions

This directory contains conventions that arise specifically from Docker images, build context, image composition, and related Docker artifacts.

Docker Compose development/test orchestration is documented separately as the general environment convention `ENV-002`. Do not duplicate that orchestration rule here.

Current child scopes:

```text
docker/
  dockerfile/   # DOCKERFILE-*
```

Rules that apply to Docker broadly may live directly here with the `DOCKER-*` prefix. Rules specifically about authoring Dockerfiles belong under `dockerfile/`.

## DOCKER-001 — Pin container inputs according to reproducibility risk

- Local development may use exact version tags when readable version selection is useful.
- CI, reproducible builds, release, deployment, and other proof/shipping paths pin external images to immutable digests where practical.
- Do not use `latest` or similarly floating image tags in deterministic paths.
