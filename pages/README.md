# GitHub Pages showcase

This directory contains a static, representational UI for `social-service`.

It is intentionally **not** a product frontend and is not imported by the Rust service or TypeScript SDK. It uses fictional, in-memory mock data only and makes no HTTP requests. Its purpose is to help people understand what consumers of the service could build: a following timeline, direct/group chat, saved content, and other social surfaces.

The showcase follows the same presentation principles as `github-pages-template`: accessible semantic markup, responsive layout, visible light/dark and language controls, and project/source navigation. Deployment remains GitHub Pages-only.

Build and verify locally with:

```sh
node --test ./pages/tests/showcase.test.mjs
node ./pages/build.mjs
```
