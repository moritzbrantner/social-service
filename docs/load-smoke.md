# Bounded service load smoke

The optional `load:smoke` capability invokes runtime-profiler's native `http-curl`
collector. It measures real profile and timeline reads through this repository's
Axum router and PostgreSQL queries. It has no second HTTP measurement engine.

Install the immutable source separately, then execute the declared command:

```bash
cargo install --git https://github.com/moritzbrantner/runtime-profiler.git --rev 7ba8e3a9a80ed4911f49a07b4b4848945f6e6fdb --locked --root .artifacts/tools/runtime-profiler
cargo fetch --locked
export PATH="$PWD/.artifacts/tools/runtime-profiler/bin:$PATH"
python3 scripts/load_smoke.py
python3 scripts/test_load_smoke.py
```

Prerequisites are the repository Rust toolchain, Python 3, Unix process groups,
curl 8.4+ with HTTP support, and Docker Compose supporting `!override` (2.24.4+).
The local daemon socket is `/var/run/docker.sock`. Other daemon/device topologies
are not part of this pilot. Installation may acquire source/dependencies; capture
never downloads the profiler. Build uses Cargo's locked, offline graph after explicit acquisition.

`.performance/load-smoke.json` owns the workload: two GET paths, 32 requests per
batch, concurrency four, three-second request timeouts, one warmup and three
measured batches, each bounded to 15 seconds. Thus 96 measured transfers exclude
32 warmups and health probes. Setup is bounded to 60 seconds and teardown to 30.
`scripts/load_fixture.py` reuses `compose.yaml` with only a private project name
and loopback OS-assigned port override. Its isolated volume contains two fixed
profiles, one follow edge, and eight posts with fixed UUIDs/timestamps. It never
uses `DATABASE_URL`, a development database, or a production endpoint. The
Rust example checks seeded profile and timeline responses before publishing its
own OS-assigned HTTP port. Native capture owns/reaps the foreground process group;
its teardown removes only that private Compose project's container and volume,
including startup/capture failure. External cancellation cleanup follows the
profiler's first-interrupt contract; force-killing the owner cannot promise cleanup.

Measurements are immutable `.artifacts/load-smoke/run-*` bundles. Native validation
must pass before the wrapper interprets samples. Reported latency, success/error
rates, response byte counts, and completed-request throughput are observations.
Throughput includes curl startup/polling and is not a capacity claim. Collector
setup/teardown/wall times have `overhead_status: not-isolated`; no overhead is
subtracted. Shared-runner timings do not gate changes or establish a controlled
performance baseline. This pilot has no timing/regression threshold. Only an
empty/failed transfer set fails smoke; policy remains owned here or by an evaluator.

Exit 0 means a nonempty validated capture with all expected statuses. Exit 1 means
invalid workload, build/capture/validation failure, or a failed measured transfer.
Exit 2 means missing/unsupported native collector or local infrastructure. stdout
is one JSON result; diagnostics remain out of captured evidence. App/user header
values are disposable fixture identities, not credentials, and are omitted from
bundle artifacts. The wrapper accepts a repository-local `--scenario` for explicit
diagnostics; the native collector still permits only an owned loopback fixture.

Fixture semantics form part of the workload contract. Change the scenario ID when
changing seed shape or measured endpoint semantics; do not compare changed fixtures
as the same workload. Native comparisons additionally check scenario/environment,
curl build and adapter identity. Source revisions stay separate from workload and
environment identity. CI records captures and the selected capability result,
including the real invalid-identity negative case; these facts do not imply coverage
of every service route, correctness rule, concurrency behavior, or production load.
