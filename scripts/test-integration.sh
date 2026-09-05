#!/usr/bin/env bash
set -euo pipefail

cleanup() {
  docker compose down --volumes --remove-orphans >/dev/null 2>&1 || true
}
trap cleanup EXIT

cleanup
docker compose up -d --wait postgres
export DATABASE_URL="${DATABASE_URL:-postgres://social:social@localhost:5432/social}"
cargo test --locked --tests
cargo test --locked --test database_visibility -- --ignored --nocapture
cargo test --locked --test database_moderation -- --ignored --nocapture
