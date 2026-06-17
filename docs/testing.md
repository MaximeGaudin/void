# Testing

The suite is ~700 tests across the workspace, run by CI on Linux, macOS, and Windows. This page documents how it's organized, the conventions to follow when adding tests, and the known coverage gaps.

## Running

```bash
./scripts/check.sh        # fmt + clippy (-D warnings) + tests, mirrors CI
cargo test                # all tests
cargo test -p void-core   # one crate
cargo test --locked       # exactly what CI runs
```

CI (`.github/workflows/ci.yml`) also runs, on every push and PR:

- **fmt** — `cargo fmt --all --check`
- **check** matrix — clippy `-D warnings` + tests on ubuntu / windows / macOS, all `--locked`
- **msrv** — `cargo check` on the declared MSRV (pinned in `Cargo.toml` `rust-version`); fails when a dependency raises the real floor, signalling a bump is needed
- **deny** — `cargo deny check` (license allow-list + RUSTSEC advisories)
- **coverage** — `cargo llvm-cov` → Codecov, non-blocking

## Layout

Tests are inline `#[cfg(test)] mod` modules next to the code (so they can reach private items), except `void-cli` binary tests which live in `crates/void-cli/tests/` as integration tests. Shared DB seed fixtures (`make_conversation`, `make_message`, …) live in `void_core::test_fixtures` (feature `test-fixtures`).

| Area | Where | What |
|------|-------|------|
| Binary CLI contract | `void-cli/tests/cli_contract.rs` | every command's `--help` exits 0; required-arg violations exit non-zero |
| Read paths | `void-cli/tests/read_paths.rs` | seeds an on-disk `void.db` in a tempdir, runs `inbox`/`search`/`messages`/… asserting seeded content |
| Read-path JSON snapshots | `void-cli/tests/read_paths_snapshots.rs` | `insta` snapshots of `inbox` / `conversations` JSON envelopes (layout regressions) |
| First run | `void-cli/tests/first_run.rs` | empty store / missing config never panics; `doctor --non-interactive` exits cleanly |
| Sync engine | `void-core/src/sync/` | mock `Connector` drives orchestration, failure isolation, cancellation, `LOCK` release |
| Database | `void-core/src/db/` | FTS5 search (incl. proptest fuzzing), `bulk_archive_before`, schema snapshot + migration data-preservation, dedup, mute |
| Hooks | `void-core/src/hooks/` | trigger matching, cron scheduling, active windows, placeholders, and `execute_hook` against a stub agent binary |
| Remote store | `void-core/src/store/` | fake `ssh`/`scp` on `PATH` verify argv, staging order, error surfacing; cache TTL |
| Config | `void-core/src/config/` | legacy `[[accounts]]` migration, unknown-type errors |
| Connectors | each `void-*` crate | API-response parsing happy + error paths (401/429/5xx/malformed) over `wiremock`; message/media extraction |

## Conventions

- **Determinism**: no real network, no wall-clock (`Utc::now()`) in assertions — inject fixed `chrono` instants. No real user filesystem — use `tempfile::tempdir()`.
- **Mock `Connector`**: an in-crate test double implementing the async `Connector` trait, recording calls via `Arc<Mutex<…>>`/atomics with configurable behavior (succeed / fail / block-until-cancelled). See `void-core/src/sync/tests.rs`.
- **Stub agent** (hooks): a shell script written to a tempdir emitting canned Claude-style stream-json, gated `#[cfg(unix)]`.
- **Fake `ssh`/`scp`** (remote store): scripts on a prepended `PATH`, gated `#[cfg(unix)]`, serialized on a mutex since `PATH` is process-global.
- **HTTP connectors**: `wiremock::MockServer` via each client's `with_base_url(...)` test constructor (including Hacker News `HnClient::with_base_url`). For a 429 retry test, set `Retry-After: 0` so retries exhaust without sleeping.
- **No `#[ignore]`**: a test that can't run is removed or `#[cfg]`-gated, not left ignored.

## Known coverage gaps

Honest list of what is *not* covered and why — good first contributions:

- **Telegram/WhatsApp live sync** (`start_sync`, `authenticate`): still require a live MTProto/WhatsApp session; `health_check` and pure extract/send helpers are now unit-tested, but full orchestration is not driven end-to-end without a real client.

When you close one of these, update this section.
