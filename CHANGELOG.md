# Changelog

All notable changes to Void CLI are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
