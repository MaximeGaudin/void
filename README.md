# Void CLI

![void banner](docs/assets/readme-banner.svg)

[![CI](https://github.com/MaximeGaudin/void/actions/workflows/ci.yml/badge.svg)](https://github.com/MaximeGaudin/void/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/MaximeGaudin/void)](https://github.com/MaximeGaudin/void/releases/latest)
[![License: AGPL-3.0](https://img.shields.io/badge/license-AGPL--3.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.89%2B-orange.svg)](Cargo.toml)

**One inbox for everything.** `void` unifies WhatsApp, Telegram, Slack, Gmail, Google Calendar, Google Drive, LinkedIn, and Hacker News into a single local-first command-line tool — one inbox, one search index, one set of commands.

It is built for terminals, shell scripts, and AI agents:

- **One inbox** — `void inbox` shows every unprocessed message across all your accounts
- **Local-first** — a background daemon syncs everything into SQLite; reads are instant and work offline
- **Full-text search** — FTS5 across every message on every service
- **Inbox Zero** — triage, act, archive; muted noise stays out of sight
- **Agent hooks** — run Claude Code (or any agent CLI) on new messages or cron schedules
- **Remote mode** — sync on a home server, drive it from any laptop over plain SSH

**Start here:** [Install](docs/install.md) · [Commands](docs/commands.md) · [Connector setup](docs/connectors.md) · [Configuration](docs/configuration.md) · [Hooks](docs/hooks.md) · [Remote store](docs/remote-store.md)

## Install

```bash
# macOS (Homebrew) — recommended
brew install MaximeGaudin/tap/void

# macOS (Apple Silicon, direct)
curl -fsSL https://github.com/MaximeGaudin/void/releases/latest/download/void-darwin-arm64.tar.gz | sudo tar xz -C /usr/local/bin

# Linux (x86_64)
curl -fsSL https://github.com/MaximeGaudin/void/releases/latest/download/void-linux-amd64.tar.gz | sudo tar xz -C /usr/local/bin
```

macOS Intel, Linux ARM64, Windows, and build-from-source: see [Install](docs/install.md).

## Quick start

```bash
void setup            # interactive wizard — connect WhatsApp, Slack, Gmail, ...
void sync --daemon    # start the background sync daemon

void inbox                                            # everything unprocessed, all services
void search "quarterly report"                        # full-text search across all of it
void reply <id> --message "On it — sending today."    # reply from where you are
void archive <id>                                     # done; on to the next one
```

WhatsApp and Telegram connect by scanning a QR code. Gmail and Calendar ship with built-in OAuth credentials — no Google Cloud project required. Per-service details: [Connector setup](docs/connectors.md).

## The Inbox Zero loop

Void follows an **Inbox Zero** model: every unprocessed message from every service lands in a single inbox, and the goal is to empty it.

1. **Triage** — `void inbox` shows all unarchived messages across every connector
2. **Act** — reply, react, forward, draft, or just read
3. **Archive** — `void archive <id>` marks the item as processed
4. **Done** — when `void inbox` returns nothing, you're at Inbox Zero

Noise never reaches the inbox in the first place: `void mute` (or `ignore_conversations` in the config) silences groups and channels permanently. `void inbox --all` reviews what's been archived.

## Everyday examples

### Messaging

```bash
# Send anywhere through one interface
void send --via slack --to "#general" --message "Hello team"
void send --via whatsapp --to "Alice" --message "Running 5 min late"
void send --via telegram --to "Notes" --file ./screenshot.png

# Threads, reactions, edits, scheduled messages
void reply <id> --message "Agreed" --in-thread
void slack react <id> --emoji rocket
void slack schedule --channel "#standup" --message "OOO today" --at "2026-06-12 09:00"

# Bulk-archive an old backlog
void archive --before 2026-05-01 --connector slack
```

### Gmail

Docs: [commands](docs/commands.md#gmail) · [setup](docs/connectors.md#gmail--google-calendar)

```bash
void gmail search "from:boss newer_than:7d"
void gmail thread <id>

# Drafts only — void never sends email directly
void gmail draft create --reply-to <id> --subject "Re: Q3" --body "LGTM, approved."

# Archive in Gmail by removing the INBOX label, in bulk
void gmail batch-modify <id1> <id2> --remove INBOX
```

### Calendar

Docs: [commands](docs/commands.md#calendar)

```bash
void calendar                       # today
void calendar week                  # this week
void calendar --day tomorrow
void calendar create --title "1:1 Alice" --start "2026-06-16T14:00" --meet   # ends +30min by default
void calendar availability --attendees alice@x.com,bob@x.com --from 2026-06-15T09:00 --to 2026-06-15T18:00
void calendar respond <id> --status accept
```

### Google Drive

Docs: [commands](docs/commands.md#google-drive)

```bash
void drive download "https://docs.google.com/document/d/..." -o spec.md
void drive download <sheet-url> --stdout | head    # pipe exported content
```

### Hacker News

Keyword-watched stories land in your inbox like any other message:

```bash
void hn keywords add "rust,local-first"
void hn min-score 100
```

### Automation with hooks

Hooks run an AI agent on new messages or cron schedules — and since the agent can call `void` itself, it can triage, draft, and notify on your behalf. Docs: [Hooks](docs/hooks.md)

```bash
# Get pinged on Telegram when an important email lands
void hook create --name email-triage --trigger new_message --connector gmail \
  --prompt 'New email: {message}. If important and actionable, send me a one-line summary on Telegram (chat "Notes") via void send. Otherwise do nothing.'

# Weekday-morning digest
void hook create --name digest --trigger schedule --cron "0 8 * * mon-fri" \
  --prompt-file ~/.config/void/prompts/digest.md --max-turns 10
```

## How it works

A background daemon keeps a local SQLite database in sync with every connected service. Read commands hit the local database — instant, offline-capable. Write commands call the service APIs directly.

```
            reads (instant, offline)          writes (direct API)
  void ◄──────────► SQLite (FTS5) ◄────── sync daemon ──────► services
                                              │
        WhatsApp │ Telegram │ Slack ──── push (WebSocket / MTProto)
        Gmail │ Calendar │ LinkedIn │ HN ──── polling
```

| Crate | Role |
|-------|------|
| `void-core` | Config, database, models, hooks, `Connector` trait, sync engine |
| `void-cli` | The `void` binary: clap commands, output formatting |
| `void-slack`, `void-gmail`, `void-calendar`, `void-whatsapp`, `void-telegram`, `void-gdrive`, `void-hackernews`, `void-linkedin` | One crate per connector |

All data stays on your machine in `~/.local/share/void` — no external database, no Docker, no cloud. Layout details: [Configuration](docs/configuration.md#data-storage-layout).

### Two ways to run it

| | **Local mode** (default) | **Remote mode** |
|---|---|---|
| Where sync runs | your machine | an always-on server |
| Setup | nothing beyond `void setup` | one server + a 4-line client config |
| When your laptop is off | sync and hooks pause | sync and hooks keep running 24/7 |
| Multiple machines | one database per machine | any laptop, over plain SSH |

**Local mode** is the zero-infrastructure default: daemon, database, and credentials all live on your machine. The only catch is that sync follows your laptop — closed lid means a stale inbox and silent hooks until it wakes.

**Remote mode** moves the daemon (and your credentials) to a home server or VPS. Your laptop keeps a cached snapshot for instant, offline-capable reads, and proxies writes to the server over plain SSH — no extra service, no open ports beyond SSH.

Details, trade-offs, and migration: [Deployment modes](docs/deployment.md) · config reference: [Remote store](docs/remote-store.md)

## Documentation

- [Install](docs/install.md) — all platforms, build from source
- [Command reference](docs/commands.md) — every command and flag
- [Connector setup](docs/connectors.md) — credentials and onboarding per service
- [Configuration](docs/configuration.md) — full `config.toml` schema, data layout
- [Hooks](docs/hooks.md) — LLM automation: triggers, placeholders, agent contract
- [Deployment modes](docs/deployment.md) — local vs remote sync: trade-offs, architecture, migration
- [Remote store](docs/remote-store.md) — server-side sync over SSH
- [Adding a connector](docs/adding-a-connector.md) — wire in a new service
- [Testing](docs/testing.md) — suite layout, conventions, coverage gaps

## Development

```bash
./scripts/check.sh       # fmt + clippy + tests, same as CI
cargo build --release
```

Contributions welcome — read [CONTRIBUTING.md](CONTRIBUTING.md), and [Adding a connector](docs/adding-a-connector.md) is the best place to start. Security reports: [SECURITY.md](SECURITY.md). Release notes: [CHANGELOG.md](CHANGELOG.md).

## License

Copyright (C) 2026 Maxime Gaudin

Free software under the [GNU Affero General Public License v3.0](https://www.gnu.org/licenses/agpl-3.0.html). See [LICENSE](LICENSE) for the full text.
