# Moderation signals

Moderation signals are immutable, provider-neutral evidence for human or separately configured policy review. They are part of the existing `moderation` capability; they are not a second moderation authority and they do not introduce an AI-specific public contract.

## Authority boundary

External classifiers, heuristics, rate-abuse detectors, media scanners, and anomaly detectors may submit signals through the trusted moderation client with only `signals.write`. A signal can describe severity, confidence, model identity/version, structured evidence, observation time, and optional case linkage.

Submitting a signal never hides or removes content, restricts a user, changes group membership, suspends an account, or resolves a case. Those mutations continue to require the existing Rust-owned moderation commands and their distinct capabilities. A classifier adapter therefore does not need `content.moderate` or `users.restrict` merely to contribute evidence.

Moderators can inspect signals with `signals.read`. The built-in queue is intentionally provider-neutral and can filter by target, case, minimum severity, or source. Product/admin UIs remain external consumers of this API.

## Provenance and idempotence

Each signal records:

- `source`: a stable adapter/provider namespace;
- `kind`: the provider-neutral finding category chosen by the adapter;
- `severity`: `low | medium | high | critical` for triage;
- optional confidence in the closed range `0..1`;
- optional model and model-version identity;
- bounded JSON evidence for explanations and reproducible observations;
- an adapter-supplied idempotency key;
- observation time, ingestion actor, request correlation id, and creation time.

Idempotency is scoped by `(app_id, source, idempotency_key)`. An exact retry returns the existing signal. Reusing that key for different semantic evidence fails closed. If the caller omitted `observedAt`, a retry does not become different merely because server time advanced.

Signal rows are immutable at the database boundary. Corrections or later classifier runs are new observations with new idempotency keys; history is not rewritten.

## App and case isolation

Signals are always scoped by `app_id`. A case link is accepted only when the case belongs to the same app and targets the same moderation object as the signal. A signal target must exist in the caller's app.

## Integration guidance

Adapters should stay outside the core social domain. Examples include a local rules engine, a Hugging Face text model, an image-safety model, a rate-abuse detector, or a hosted classifier. Convert their output into the stable signal contract at the adapter boundary instead of leaking provider-specific response schemas into posts, messages, media, groups, or moderation state.

Prefer explainable evidence that helps a reviewer reproduce the observation. Do not place secrets, credentials, raw authentication tokens, or unbounded provider payloads into `evidence`.

If automatic enforcement is introduced later, it must be an explicit, separately configured policy that consumes signals and invokes the ordinary audited moderation commands. The existence of a signal alone must never silently grant enforcement authority.
