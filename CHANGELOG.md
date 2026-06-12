# Changelog

All notable changes to Void CLI are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed

- **Remote mode** — Download commands (`gmail attachment`, `whatsapp download`, `telegram download`, `linkedin download`) failed on a fresh remote store because the remote staging directory was only created by `--file` uploads. The staging directory is now created before download-only proxied commands, and download handlers create the output file's parent directories (matching `drive download`).

## [0.9.3] - 2026-06-11

### Changed

- **Homebrew** — Fixed automatic tap formula updates on release (deploy-key authentication instead of a cross-repo PAT).

## [0.9.2] - 2026-06-11

### Changed

- **License** — Relicensed from GPL-3.0-only to **AGPL-3.0-only** to close the network-use (SaaS) loophole. As sole author, no contributor consent was required.
- **Dependencies** — Bumped rusqlite (0.39), tokio-tungstenite (0.29), croner (3.0), html-to-markdown-rs (3.0), and a group of minor/patch updates.
- **MSRV** — Raised the declared minimum supported Rust version to 1.89 (the real floor required by the dependency tree; the previously declared 1.75 did not build).
- **CI** — Added macOS to the test matrix, `--locked` builds, and new MSRV, `cargo deny`, and coverage jobs.
- **Tests** — Added ~100 tests (binary CLI integration, sync-engine orchestration, schema migrations, FTS5 fuzzing, hook execution, remote-store proxy, and per-connector API error paths). See [docs/testing.md](docs/testing.md).
- **Docs** — Restructured the README around a quick start and everyday examples, and split deep material into modular guides (`docs/`): command reference, configuration, connector setup, hooks, remote store, testing.
- **Community** — Added contribution guide, security policy (including embedded OAuth client documentation), code of conduct, issue/PR templates, and Dependabot configuration.

### Removed

- **Agent** — Removed the unused rig-core-based interactive agent mode (`void agent`) and its crate. Hooks (`void hook`) are unaffected.

## [0.9.1] - 2026-06-10

### Fixed

- **Calendar** — Auto-include the calendar account owner in the attendee list when creating events with `--attendees` (the organizer was omitted from Google Calendar API requests).
- **Remote** — Stage file attachments over SCP in proxied commands so send/reply with `--file` works in remote store mode.

### Changed

- **Docs** — Improved README quick start install instructions, including a curl one-liner.

## [0.9.0] - 2026-06-08

### Added

- **Remote store** — Full remote store mode with SSH proxy and local cache: run sync on a home server, use `void` locally against the same data (`store.mode = "remote"`, `void remote status`, `void remote refresh`).
- **Doctor** — Added `--non-interactive` flag for scriptable health checks.
- **LinkedIn** — Sync comments on your own posts via Unipile Posts & Comments API (one thread per post, nested comment replies, `void reply` on post comments).
- **LinkedIn** — Catch-up after sleep/idle (wall-clock idle detection, progress on resume, same as Slack/Gmail).
- **Hacker News** — Catch-up after sleep/idle with visible progress when resuming from hibernation.

### Fixed

- **Calendar** — Incremental sync now works: initial backfill no longer uses `orderBy` (which prevented Google from returning a sync token), and a bootstrap step acquires the token for ongoing updates.
- **Config** — Auto-create default config on first run instead of erroring.
- **Gmail** — Paginate `list_history` to avoid silently dropping events.
- **Gmail** — Preserve HTML when forwarding messages.
- **Inbox** — Thread dedup no longer hides threads with newer archived messages.
- **LinkedIn** — Run catch-up on daemon start when backfill is already complete (missed messages while void was stopped).
- **Slack** — Skip external/Google and thumbnail-only attachments when caching files; prefer `url_private_download` to avoid repeated 404/401 warnings.

## [0.8.0] - 2026-05-19

### Added

- **LinkedIn** — New connector via the Unipile API: sync direct messages, send, reply, and download attachments (`void linkedin download`).
- **LinkedIn** — Setup wizard and config fields (`api_key`, `dsn`, `account_id`); sync tuning via `linkedin_poll_interval_secs` and `linkedin_backfill_days` (default 15-day backfill).
- **LinkedIn** — Resolved sender display names, profile URLs, and avatars from Unipile user/attendee APIs.
- **CLI** — `void messages linkedin` / `void messages li` lists recent LinkedIn messages; contact names resolve to a conversation when unambiguous.
- **Agent** — Void Agent prompt and tools updated for LinkedIn inbox, send, reply, and media download.

### Fixed

- **Search** — Full-text search no longer deduplicates by conversation context, so older messages in a thread remain findable after a newer reply.
- **Search** — Conversation display names are included in search results alongside message body and sender name.

## [0.7.0] - 2026-05-18

### Added

- **Hooks** — Added `active_window` to restrict hook execution to specific days and hours.
- **Hooks** — Added per-hook `extra_args` (a generic argv passthrough) so hooks can forward any agent-specific CLI flags without `void` having to know their spelling. All agent-specific flags — model selection, tool permissions, etc. — are the hook author's responsibility. Example: `extra_args = ["--model", "sonnet", "--dangerously-skip-permissions"]`.

### Changed

- **Hooks** — Replaced dedicated `model`, `allowed_tools`, and `dangerously_skip_permissions` fields with the generic `extra_args` passthrough.
- **Sync** — Status logs now include timestamps; Slack is re-synced on any reconnect.

### Fixed

- **Hooks** — When an agent exits non-zero with an empty stderr (e.g. Claude rate-limit rejections), the error surfaced on the console and in logs was blank. The executor now parses the stream-json stdout and extracts the final `result` / `rate_limit_event` record, so failures show something like `claude exited with exit status: 1: [HTTP 429, rate_limit=five_hour] You've hit your limit …`.
- **Slack** — Thread replies are now fetched during backfill.
- **Slack** — Permalinks are resolved via the native `(channel, ts)` pair.
- **Slack** — Real-time messages ingested via Socket Mode now carry the same metadata as backfilled ones (`channel_id`, `channel_name`, `channel_kind`, optional `thread_ts`). Previously plain-text events ended up with `metadata: null`, which broke downstream consumers that relied on the channel name (e.g. notification hooks).
- **Sync** — Broken connectors now surface errors instead of being silently skipped.
- **Log** — Status macro output now uses full ISO-8601 timestamps.

### Removed

- **Knowledge Base** — Removed the `void kb` command and the entire Knowledge Base feature (daemon, indexing, and related config).

## [0.6.0] - 2026-04-23

### Added

- **Config** — Added `ignore_conversations` option to any connection. Matching conversations are auto-muted on every sync start (case-insensitive substring match on name or external ID).
- **Sync** — Added `--status` flag to show daemon and connector state, outputting as JSON.
- **Contacts** — Added profile picture (`avatar_url`) to contacts and backfilled URLs for existing messages.
- **Doctor** — Added connection health checks and offered re-authentication on failure.
- **CLI** — Added connector-specific forward subcommands for Gmail, Slack, and Telegram.

### Changed

- **Setup** — Slack re-authentication now keeps existing tokens on empty input and populates them as defaults.
- **Setup** — Slack re-authentication prints the refresh token save path to clarify it's not in config.toml.
- **Codebase** — Split large modules (hooks, config) and extracted duplicated forward helpers to a shared module.

### Fixed

- **CLI** — Fixed pagination metadata (`total_elements`, `total_pages`) being inflated when context dedup removed messages from the result set. Count and data queries now apply identical filtering at the SQL level.
- **Gmail** — Included email subject in message metadata JSON.
- **Gmail** — Enforced base64 and HTML formatting for email bodies.
- **Calendar** — Validated and normalized datetime inputs to RFC 3339.
- **Slack** — Added idle watchdog to detect stale WebSockets after hibernation.
- **Slack / Gmail** — Caught up on missed messages after hibernation.
- **Doctor** — Included event subscription config token validity in the health check.
- **Doctor** — Correctly handled interactive re-authentication for Slack.

## [0.5.0] - 2026-03-29

### Added

- **CLI** — Added `--page` parameter and pagination metadata to all list commands.
- **Slack** — Downloaded files locally for auth-free access instead of requiring Slack tokens to view attachments.
- **Slack** — Retroactively downloaded files for previously synced messages on startup.
- **Slack** — Automatically deleted cached files when the parent message is archived.
- **Archive** — Added `--before` flag for bulk date-based archiving.

### Changed

- **Slack** — Removed unused `exclude_channels` config field.

## [0.4.2] - 2026-03-24

### Added

- **Slack** — Auto-repair event subscriptions at sync startup. Slack silently disables event subscriptions when the CLI is not running for a while; void now detects this and restores them automatically via the App Manifest API.
- **Slack setup** — Added optional Step 6 to collect App ID and Config Refresh Token for the auto-repair feature.

### Fixed

- **Slack** — Always force-update the manifest on sync startup. Slack keeps events listed in the exported manifest even when the "Enable Events" toggle is OFF, so checking the manifest alone was unreliable.
- **Sync** — Prevented premature force-exit and improved shutdown signal logging.

## [0.4.1] - 2026-03-24

### Added

- **Gmail** — Made `--to` optional on `void gmail draft create`; replies now default to reply-all recipients automatically.

### Changed

- **Gmail** — Removed `--thread-id` flag from draft create; thread is now auto-derived from `--reply-to` message.

### Fixed

- **Gmail** — Stripped internal ID prefix from `--reply-to` and `--thread-id` values so the Gmail API receives clean message IDs.
- **Build** — Gated `sysinfo::Signal` import to Unix targets to fix Windows compilation.

## [0.4.0] - 2026-03-23

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
