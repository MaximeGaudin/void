# Changelog

All notable changes to Void CLI are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [4.0.1] - 2026-03-24

### Added

- **Gmail** — Made `--to` optional on `void gmail draft create`; replies now default to reply-all recipients automatically.

### Changed

- **Gmail** — Removed `--thread-id` flag from draft create; thread is now auto-derived from `--reply-to` message.

### Fixed

- **Gmail** — Stripped internal ID prefix from `--reply-to` and `--thread-id` values so the Gmail API receives clean message IDs.
- **Build** — Gated `sysinfo::Signal` import to Unix targets to fix Windows compilation.

## [4.0.0] - 2026-03-23

### Added

- **Cross-platform runtime** — Added Windows support for the sync daemon lifecycle, including detached background execution and stop handling.
- **Windows install script** — Added `scripts/build-install.ps1` for native PowerShell install/update workflow.
- **CI/Release** — Added Windows targets to CI and release packaging, including `void.exe` zip artifacts.

### Changed

- **Sync daemon** — Replaced Unix-only double-fork daemonization with a cross-platform re-exec daemon model and internal daemon mode routing.
- **Process management** — Switched stale-lock and daemon process checks to `sysinfo` for cross-platform behavior.
- **Configuration paths** — Made default config/store path resolution platform-aware using `dirs`, while preserving legacy Unix path compatibility when existing paths are present.
- **Agent shell execution** — Added OS-specific shell invocation (`sh -c` on Unix, `cmd /C` on Windows) and home directory resolution via `dirs`.
- **Codebase structure** — Split large connector modules and reorganized connector crate internals for maintainability.
- **Documentation** — Updated README to reflect daemon behavior (`void sync` foreground vs `void sync --daemon`), Windows install steps, and platform-specific config/storage paths.

### Fixed

- **CLI setup editor fallback** — Uses `notepad` by default on Windows instead of Unix-only editor assumptions.
- **Tests** — Replaced hardcoded `/tmp` paths with `std::env::temp_dir()` in connector/auth tests and added targeted edge-case coverage for config redaction, lock-file parsing, and Gmail auth cache loading.

## [0.3.2] - 2026-03-23

### Added

- **Hacker News** — `void hn` subcommand to view and manage keywords and min-score from the CLI (`void hn config`, `void hn keywords`, `void hn min-score`)
- **Gmail** — `--file` attachment support on `void gmail draft create`, `void gmail draft update`, and `void reply`

### Changed

- **void-calendar** — Split monolithic connector into focused submodules (types, mapping, sync_ops, events)
- **void-cli** — Split setup wizard into per-connector modules for better maintainability
- **void-core** — Split database layer into focused submodules (schema, messages, conversations, contacts, search)
- Updated dependencies (env_logger, html-to-markdown-rs, html5ever, moka, ureq, and more)
- Expanded test coverage in void-core (models, links, config)

### Fixed

- **Gmail** — Inbox sync now mirrors the actual Gmail INBOX state; startup reconciliation ensures local `is_archived` flags match Gmail labels, and incremental sync handles `labelsAdded`/`labelsRemoved` events
- **Gmail** — Fixed `history_id` overwrite on daemon restart that caused missed incremental changes
- **Gmail** — Resolved clippy `collapsible_if` warning in incremental sync

## [0.3.1] - 2026-03-19

### Fixed

- Fixed code formatting across all crates to pass CI

## [0.3.0] - 2026-03-19

### Added

- **Telegram connector** — full MTProto-based connector using grammers with QR code login, real-time sync, send, reply, and deterministic message IDs
- **Hacker News connector** — monitor top stories with keyword/score filtering, backfill progress reporting, and real-time polling
- **Gmail** — Exposed file attachment IDs in search, thread, and sync results
- **Slack** — Resolve permalink URLs across all commands (`send`, `reply`, `search`, etc.)
- **Sync** — Improved real-time message logging with datetime, conversation name, and sender
- **Sync** — Full timestamps in daemon log lines
- **Docs** — Added connector runbook for contributors with ID conventions and concurrency patterns

### Changed

- Removed `--pretty` flag — output is always JSON
- Renamed "account" to "connection" across the entire codebase (CLI flags, config, database, commands)
- License changed from MIT to GPL-3.0-only
- Replaced unmaintained `daemonize` crate with manual double-fork daemon
- **Hacker News** — Only poll top stories from HN API, dropped new stories endpoint
- Updated and cleaned up dependencies; removed unused crates

### Fixed

- **Gmail** — Skipped sent-only messages in incremental sync
- **Gmail** — Handled padded base64 in attachment/body decoding
- **Gmail** — RFC 2047 encode non-ASCII email subject headers
- **Slack** — Sync missing messages with proper pagination, fixed filter logic, handled `file_share` subtype
- **Slack** — Resolve `#channel` names to IDs for file uploads
- **Telegram** — Replaced libsql-backed session with JSON file storage for portability
- **Telegram** — Seed session with production DC addresses and handle DC migration during QR login
- **Telegram** — Poll frequently during QR login to avoid token expiry
- **Telegram** — Fall through to `search_peer` when `resolve_username` fails
- **Telegram** — Use conversation PK (not `external_id`) for `message.conversation_id`
- **Hacker News** — Use conversation ID for message foreign key
- **Hacker News** — Removed meaningless context field from messages
- **WhatsApp** — Suppressed noisy `wa-rs` notification handler warning
- **Sync** — Let sync overwrite archive state instead of using sticky MAX
- **Sync** — Auto-detect and remove stale lock files
- **Sync** — Stop daemon before installing new binary to prevent zombie processes
- **Sync** — Start real-time listeners before backfill to avoid missing messages
- **Sync** — Enforce sync contract across all connectors
- Added HTTP timeouts to prevent CLI hangs
- Validated and normalized `--connector` flag across all commands
- Partial ID matching in `list_messages` and `get_message`

## [0.2.0] - 2026-03-16

### Added

- **Agent mode** — `void agent` command with LLM-powered agentic communication processing
  - Multi-provider support: Anthropic API, Claude Code CLI (Max/Pro), OpenRouter, OpenAI
  - Claude Code CLI as primary backend for Max/Pro subscription users
- **Hooks system** — LLM prompts triggered by events or cron schedules
  - `void hook create|list|show|delete|enable|disable|test|log` commands
  - Event-driven hooks fire on new messages per connector
  - Scheduled hooks with cron expressions
  - Full session logging with input prompt, raw agent output, and execution metadata
  - Sync log visibility with `[hook]` lines for real-time monitoring
- **Forward messages** — `void forward <MESSAGE_ID> --to <RECIPIENT>` for Gmail and Slack
- **Google Drive** — `void drive` command for downloading files from Drive/Docs/Sheets/Slides
- **File attachments** — send and reply with file attachments across Gmail, Slack, WhatsApp
- **Slack Socket Mode** — real-time event streaming replaces polling
- **Slack scheduled messages** — `void send --at` and `void reply --at` for deferred delivery
- **Slack file upload** — multi-step upload flow for `send_message` and `reply`
- **Slack incremental catch-up** — fetch missed messages on sync restart
- **Slack `open` command** — open group conversations with multiple users
- **Calendar management** — `update`, `delete`, `respond`, `search`, `availability` commands
- **Calendar notifications** — meeting reminders during sync
- **Gmail management** — threads, attachments, labels, drafts via `void gmail` subcommands
- **WhatsApp media download** — `void whatsapp download` for media files
- **Mute command** — `void mute` to silence noisy channels/conversations
- **Bulk archive/read** — accept multiple message IDs in a single call
- **Message context enrichment** — `context_id` grouping with deduplication
- **ISO 8601 dates** — all date fields serialized as ISO 8601 across all models
- **Embedded Google credentials** — no manual OAuth client setup required

### Changed

- Slack backfill and catch-up unified into shared `fetch_history`
- Skip inactive Slack conversations during catch-up for better performance
- `--limit` renamed to `--size` (`-n`) across all listing commands
- `--all` flag on inbox now includes muted conversations
- Connector trait renamed from `Channel` across the codebase

### Fixed

- Calendar auth runs interactive OAuth flow with correct credential wording
- Calendar config no longer deserialized as Gmail variant
- Calendar handles deleted events during incremental sync
- Calendar pagination and local timezone for date filtering
- Slack re-backfill skipped on restart; `connection_id` added to progress logs
- Connection rename now moves token files and session DBs
- WhatsApp health check uses session file instead of live connection
- `Ctrl+C` properly stops sync with force-quit and timeout
- UTF-8 multi-byte character panic in output truncation
- FTS5 search query escaping

## [0.1.0] - 2026-03-11

### Added

- **Core architecture** — Rust workspace with `void-core`, `void-cli`, and per-connector crates
- **Configuration** — TOML-based config with `void setup` interactive wizard
- **Database** — SQLite WAL with FTS5 full-text search
- **Sync engine** — concurrent connector sync with file locking and cancellation
- **Gmail connector** — OAuth2 auth, full/incremental sync, send, reply, archive, mark read
- **Slack connector** — token auth, conversation sync, send, reply, mark read
- **Google Calendar connector** — OAuth2 auth, event sync, event creation with `--meet`
- **WhatsApp connector** — QR code auth via wa-rs, real-time sync, send, reply
- **CLI commands** — `inbox`, `conversations`, `messages`, `search`, `contacts`, `channels`, `calendar`, `send`, `reply`, `archive`, `doctor`, `status`
- **Output formatting** — JSON mode and human-readable tables
- **Skills** — daily routine, calendar, Gmail, Slack, WhatsApp skill files
