# Configuration

`void setup` creates and maintains the configuration for you — you rarely need to edit it by hand. This page documents the full schema for when you do.

- [File location](#file-location)
- [Full example](#full-example)
- [`[store]`](#store)
- [`[sync]`](#sync)
- [`[[connections]]`](#connections)
- [`ignore_conversations`](#ignore_conversations)
- [Data storage layout](#data-storage-layout)

## File location

| Platform | Path |
|----------|------|
| Linux | `~/.config/void/config.toml` |
| macOS | `~/.config/void/config.toml` if it exists, otherwise `~/Library/Application Support/void/config.toml` |
| Windows | `%APPDATA%\void\config.toml` |

Override with the global `--config <path>` flag. Void keeps backward compatibility with existing Unix-style paths when those directories already exist, and automatically migrates legacy `[[accounts]]` sections to `[[connections]]` on load.

## Full example

```toml
[store]
path = "~/.local/share/void"

[sync]
gmail_poll_interval_secs = 30
calendar_poll_interval_secs = 60
hackernews_poll_interval_secs = 3600
reddit_poll_interval_secs = 3600
linkedin_poll_interval_secs = 1800
linkedin_backfill_days = 15
github_poll_interval_secs = 120

[[connections]]
id = "whatsapp"
type = "whatsapp"
ignore_conversations = ["noisy-group@g.us", "spam"]

[[connections]]
id = "work-slack"
type = "slack"
app_token = "xapp-1-..."
user_token = "xoxp-..."
ignore_conversations = ["random", "social"]

[[connections]]
id = "telegram"
type = "telegram"

[[connections]]
id = "you@gmail.com"
type = "gmail"
credentials_file = "~/.config/void/google-credentials.json"

[[connections]]
id = "you@gmail.com-calendar"
type = "calendar"
credentials_file = "~/.config/void/google-credentials.json"
calendar_ids = ["primary"]

[[connections]]
id = "hackernews"
type = "hackernews"
keywords = ["rust", "ai", "startup"]
min_score = 100

[[connections]]
id = "reddit"
type = "reddit"
client_id = "your-reddit-app-client-id"
client_secret = "your-reddit-app-client-secret"
refresh_token = "stored-by-setup-when-commenting-enabled"
subreddits = ["rust", "programming"]
keywords = ["ai", "startup"]
min_score = 50

[[connections]]
id = "linkedin"
type = "linkedin"
api_key = "your-unipile-api-key"
dsn = "https://api1.unipile.com:13111"
account_id = "your-unipile-account-id"

[[connections]]
id = "github"
type = "github"
token = "ghp_..."
username = "your-github-handle"
ignore_conversations = ["facebook/react"]
```

## `[store]`

| Field | Default | Description |
|-------|---------|-------------|
| `path` | `~/.local/share/void` (Unix), `%APPDATA%\void` (Windows) | Where the database, sessions, and tokens live |
| `mode` | `"local"` | `"local"` or `"remote"` — see the [Remote store guide](remote-store.md) for `[store.remote]`, `[store.remote.ssh]`, and `[store.remote.cache]` |

## `[sync]`

Polling intervals for connectors that poll (push-based connectors — WhatsApp, Telegram, Slack — receive events over persistent connections and don't poll).

| Field | Default |
|-------|---------|
| `gmail_poll_interval_secs` | 30 |
| `calendar_poll_interval_secs` | 60 |
| `hackernews_poll_interval_secs` | 3600 |
| `reddit_poll_interval_secs` | 3600 |
| `linkedin_poll_interval_secs` | 1800 |
| `linkedin_backfill_days` | 15 |
| `github_poll_interval_secs` | 120 |

## `[[connections]]`

Each connection is one account on one service. Every connection has:

| Field | Required | Description |
|-------|----------|-------------|
| `id` | yes | Unique name you choose — used by `--connection <id>` |
| `type` | yes | One of `whatsapp`, `telegram`, `slack`, `gmail`, `calendar`, `hackernews`, `googlenews`, `linkedin`, `reddit`, `github` |
| `ignore_conversations` | no | List of conversations to auto-mute (see below) |

Per-type fields:

| Type | Required fields | Optional fields |
|------|-----------------|-----------------|
| `whatsapp` | — | — |
| `telegram` | — | `api_id`, `api_hash` (custom Telegram API credentials) |
| `slack` | `app_token` (`xapp-…`), `user_token` (`xoxp-…`) | `app_id`, `config_refresh_token` |
| `gmail` | — | `credentials_file` (custom Google OAuth client) |
| `calendar` | — | `credentials_file`, `calendar_ids` (default: primary) |
| `hackernews` | — | `keywords` (default: `[]`), `min_score` (default: 0) |
| `googlenews` | — | `keywords`, `when`, `language`, `country` |
| `reddit` | `client_id`, `client_secret` | `refresh_token` (optional, enables commenting), `subreddits` (default: `[]`), `keywords` (default: `[]`), `min_score` (default: 0) |
| `linkedin` | `api_key`, `dsn`, `account_id` (Unipile) | — |
| `github` | `token`, `username` | — |

You can declare multiple connections of the same type (two Slack workspaces, several Gmail accounts, …) — give each a distinct `id`.

## `ignore_conversations`

Any connection can include an `ignore_conversations` list. Matching conversations are auto-muted on every sync start (case-insensitive substring match on name or external ID). Useful for permanently silencing noisy groups or channels:

```toml
[[connections]]
id = "whatsapp"
type = "whatsapp"
ignore_conversations = ["noisy-group@g.us", "spam", "social"]
```

You can also mute/unmute interactively with `void mute` — see the [command reference](commands.md#acting).

## Data storage layout

Everything lives locally under `store.path` — no external database, no Docker:

| File | Content |
|------|---------|
| `void.db` | Main SQLite database (WAL mode), plus `void.db-shm` / `void.db-wal` |
| `whatsapp-<connection-id>.db` | WhatsApp session |
| `telegram-<connection-id>.json` | Telegram session |
| `<connection-id>-token.json` | OAuth2 token cache (Gmail, Calendar) |
| `LOCK` | PID file while the sync daemon is running |

The config file lives at the platform config path (above), separate from the store.
