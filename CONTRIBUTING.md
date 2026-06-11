# Contributing to Void

Thanks for your interest in contributing! Void is a unified messaging CLI written in Rust, and contributions of all kinds are welcome: bug reports, docs, new connectors, and features.

## Getting started

```bash
git clone https://github.com/MaximeGaudin/void.git
cd void
cargo build
```

The Rust toolchain is pinned by `rust-toolchain.toml` — `rustup` picks it up automatically. Minimum supported Rust version is declared in `Cargo.toml` (`rust-version`).

## Before you push

CI enforces formatting, clippy with `-D warnings`, and tests on Linux and Windows. Run the same checks locally:

```bash
./scripts/check.sh        # fmt + clippy + test, same as CI
```

or individually:

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test
```

## Project layout

```
crates/
  void-core/    # Config, database, models, hooks, Connector trait, sync engine
  void-cli/     # The `void` binary: clap commands, output formatting
  void-*/       # One crate per connector (slack, gmail, whatsapp, ...)
```

The best deep-dive is [docs/adding-a-connector.md](docs/adding-a-connector.md) — it walks through the `Connector` trait, the sync engine, and the config schema.

## Adding a connector

New connectors are the most impactful contribution. Read [Adding a connector](docs/adding-a-connector.md) first, then open an issue describing the service you want to wire in so we can discuss the approach (auth model, push vs. polling) before you invest time.

## Commit conventions

We use [Conventional Commits](https://www.conventionalcommits.org/):

```
feat(slack): add scheduled message support
fix(gmail): paginate list_history to avoid dropping events
docs: clarify remote store cache TTLs
```

Common types: `feat`, `fix`, `docs`, `refactor`, `chore`, `test`.

## Changelog

User-visible changes go in [CHANGELOG.md](CHANGELOG.md) under `[Unreleased]`, following the [Keep a Changelog](https://keepachangelog.com/) format (`Added` / `Changed` / `Fixed` / `Removed`). One line, bold component prefix:

```markdown
### Fixed
- **Gmail** — Preserve HTML when forwarding messages.
```

## Pull requests

1. Fork and create a feature branch from `main`
2. Keep PRs focused — one logical change per PR
3. Update the changelog and docs when behavior changes
4. Make sure `./scripts/check.sh` passes
5. Open the PR against `main`; CI must be green

## Reporting bugs

Use the [bug report template](https://github.com/MaximeGaudin/void/issues/new/choose). Include the output of `void --version` and `void doctor`, your platform, and `-v` logs when relevant. **Never paste tokens, OAuth credentials, or message contents** — redact anything personal.

## Security issues

Please do not open public issues for security vulnerabilities — see [SECURITY.md](SECURITY.md).

## License

By contributing, you agree that your contributions will be licensed under the [GPL-3.0](LICENSE).
