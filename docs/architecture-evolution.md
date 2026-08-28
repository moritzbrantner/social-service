# Architecture evolution

The service should keep the simplest useful implementation as the default. More sophisticated storage, processing, filtering, and scaling strategies should remain opt-in and should not complicate deployments that do not need them.

## Configuration rule

Keep two kinds of configuration separate:

- **capability flags** answer _what is enabled?_ and remain in `SOCIAL_FEATURES` (`profiles`, `media`, `posts`, `comments`, `follows`, `chat`);
- **strategy settings** answer _how is an enabled capability implemented?_ and should use explicit configuration values with a minimal default.

Strategy settings should be validated at startup. Prefer deployment-time configuration over runtime flag systems until there is a concrete need for dynamic rollout. Advanced strategies should preserve the same public domain/API contracts where practical so applications do not need to know which implementation is active.

None of the advanced strategies below are required for the MVP. They are planned extension points, not implementation requirements.

## Planned strategy switches

| Concern | Minimal default | Optional advanced strategy | Planned configuration |
| --- | --- | --- | --- |
| Timeline | indexed PostgreSQL fan-out on read | precomputed timeline entries; later hybrid fan-out for high-follower accounts | `SOCIAL_TIMELINE_MODE=read|fanout|hybrid` |
| Media ownership | register an externally stored URL and content type | managed S3-compatible upload flow with presigned URLs | `SOCIAL_MEDIA_STORAGE=reference|object` |
| Images | serve the original asset | asynchronously derive thumbnails and efficient image variants | `SOCIAL_IMAGE_VARIANTS=off|standard` |
| Video | reference/serve the original asset | async metadata extraction, poster generation, and transcoded renditions | `SOCIAL_VIDEO_PROCESSING=off|transcode` |
| GIFs | preserve the uploaded GIF | derive efficient video/animated-image representations while retaining one logical media asset | `SOCIAL_GIF_PROCESSING=passthrough|optimize` |
| Media inspection | trust registered metadata subject to basic validation | inspect dimensions, duration, codecs, size, orientation, and sanitize unnecessary metadata | `SOCIAL_MEDIA_INSPECTION=basic|inspect` |
| Moderation | no automatic classification | attach non-destructive moderation labels such as spam, nudity, violence, or spoiler; applications decide policy | `SOCIAL_MODERATION_MODE=off|labels` |
| Social filtering | direct relationship/query rules only | centralized policy evaluation for blocks, mutes, visibility, muted keywords, and similar filters | `SOCIAL_FILTERING_MODE=basic|policy` |
| Deduplication | each media asset is independent | content hashes may share underlying stored bytes while keeping distinct logical ownership/attachments | `SOCIAL_MEDIA_DEDUP=off|hash` |
| Media lifecycle | explicit assets remain until deleted by product logic | orphan cleanup and derived-variant lifecycle management | `SOCIAL_MEDIA_GC=off|orphaned` |

Names are intentionally provisional until each strategy is implemented. When implementing one, keep the minimal mode supported and tested rather than replacing it with the advanced mode.

## Media model boundary

A post or message should reference a **logical media asset**, not a particular thumbnail, codec, resolution, or storage backend. Advanced media processing may add derived variants behind that asset without changing the post/message relationship.

For managed storage, keep large bytes outside PostgreSQL. PostgreSQL owns social metadata, ownership, attachment relationships, processing state, and variant metadata; object storage/CDN owns the media bytes.

If asynchronous processing is introduced, use a small lifecycle such as `uploaded -> processing -> ready | failed`. Creating a post should not synchronously wait for expensive image/video processing unless a product explicitly requires it.

## Adoption rule

Do not implement an advanced strategy merely because the extension point exists. Add it when a real application, measured bottleneck, safety requirement, or operational need justifies the extra machinery. The intended progression is always:

`minimal implementation -> measure/need -> enable advanced strategy -> retain minimal fallback`
