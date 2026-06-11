# Void CLI

A unified command-line interface for interacting with WhatsApp, Telegram, Slack, Gmail, Google Calendar, Google Drive, LinkedIn, and Hacker News from a single tool — plus an AI agent and LLM-powered hooks.

## Quick Start

Install the latest release from [GitHub Releases](https://github.com/MaximeGaudin/void/releases/latest). Commands install to directories that are already on `PATH` by default on each platform.

| Platform | Install command |
|----------|-----------------|
| macOS (Apple Silicon) | `curl -fsSL https://github.com/MaximeGaudin/void/releases/latest/download/void-darwin-arm64.tar.gz \| sudo tar xz -C /usr/local/bin` |
| macOS (Intel) | `curl -fsSL https://github.com/MaximeGaudin/void/releases/latest/download/void-darwin-amd64.tar.gz \| sudo tar xz -C /usr/local/bin` |
| Linux (x86_64) | `curl -fsSL https://github.com/MaximeGaudin/void/releases/latest/download/void-linux-amd64.tar.gz \| sudo tar xz -C /usr/local/bin` |
| Linux (ARM64) | `curl -fsSL https://github.com/MaximeGaudin/void/releases/latest/download/void-linux-arm64.tar.gz \| sudo tar xz -C /usr/local/bin` |
| Windows (PowerShell) | `$dir="$env:LOCALAPPDATA\Programs\void"; New-Item -ItemType Directory -Force -Path $dir \| Out-Null; curl.exe -fsSL -o "$env:TEMP\void.zip" https://github.com/MaximeGaudin/void/releases/latest/download/void-windows-amd64.zip; Expand-Archive -Path "$env:TEMP\void.zip" -DestinationPath $dir -Force; $userPath=[Environment]::GetEnvironmentVariable('Path','User'); if ($userPath -notlike "*$dir*") { [Environment]::SetEnvironmentVariable('Path', "$userPath;$dir", 'User') }; $env:Path += ";$dir"` |

Example (macOS Apple Silicon):

```bash
# Install latest release
curl -fsSL https://github.com/MaximeGaudin/void/releases/latest/download/void-darwin-arm64.tar.gz | sudo tar xz -C /usr/local/bin
# macOS (Intel):       curl -fsSL https://github.com/MaximeGaudin/void/releases/latest/download/void-darwin-amd64.tar.gz | sudo tar xz -C /usr/local/bin
# Linux (x86_64):      curl -fsSL https://github.com/MaximeGaudin/void/releases/latest/download/void-linux-amd64.tar.gz | sudo tar xz -C /usr/local/bin
# Linux (ARM64):       curl -fsSL https://github.com/MaximeGaudin/void/releases/latest/download/void-linux-arm64.tar.gz | sudo tar xz -C /usr/local/bin
# Windows (PowerShell): $dir="$env:LOCALAPPDATA\Programs\void"; New-Item -ItemType Directory -Force -Path $dir | Out-Null; curl.exe -fsSL -o "$env:TEMP\void.zip" https://github.com/MaximeGaudin/void/releases/latest/download/void-windows-amd64.zip; Expand-Archive -Path "$env:TEMP\void.zip" -DestinationPath $dir -Force; $userPath=[Environment]::GetEnvironmentVariable('Path','User'); if ($userPath -notlike "*$dir*") { [Environment]::SetEnvironmentVariable('Path', "$userPath;$dir", 'User') }; $env:Path += ";$dir"

# Interactive setup — configure connectors, authenticate connections
void setup

# Start background sync daemon
void sync --daemon

# Read your unified inbox
void inbox

# Search across all connectors
void search "quarterly report"

# Send a message
void send --via slack --to "#general" --message "Hello team"

# Archive a processed message
void archive <message-id>

# View today's calendar
void calendar
```

## Inbox Zero

Void follows an **Inbox Zero** model. All unprocessed messages land in a single inbox. The goal is to reach Inbox Zero — an empty inbox — by processing every item:

1. **Triage**: `void inbox` shows all unarchived messages across every connector
2. **Act**: Reply, react, draft, delegate, or simply read
3. **Archive**: `void archive <id>` marks the item as processed
4. **Done**: When `void inbox` returns nothing, you've reached Inbox Zero

Items are archived because they've been handled — either an action was taken (reply, draft, reaction) or they were informational and acknowledged. Use `void inbox --all` to review archived items.

## Architecture

Void runs a background sync daemon that continuously pulls messages and events from all configured connectors into a local SQLite database. CLI read commands query this local database for instant results. Write operations (send, reply, create event) make direct API calls.

```
┌────────────────────────────────────────────────────┐
│                    void CLI                         │
│                                                     │
│  Read (local DB)         Write (direct API)         │
│  ├── void inbox          ├── void send              │
│  ├── void search         ├── void reply             │
│  ├── void calendar       ├── void forward           │
│  ├── void contacts       ├── void archive           │
│  ├── void channels       ├── void mute              │
│  ├── void messages       ├── void calendar create   │
│  ├── void conversations  ├── void gmail draft ...   │
│                          ├── void gmail forward     │
│  AI & Automation         ├── void slack react/edit  │
│  ├── void agent          ├── void slack schedule     │
│  └── void hook           ├── void slack forward     │
│                          ├── void telegram forward  │
│                          ├── void drive download    │
│                          ├── void whatsapp download │
│                          ├── void telegram download │
│                          └── void linkedin download │
│                                                     │
│  Sync daemon                                        │
│  ├── WhatsApp (wa-rs WebSocket)                     │
│  ├── Telegram (grammers MTProto)                    │
│  ├── Slack (Socket Mode WebSocket)                  │
│  ├── Gmail (history.list polling)                   │
│  ├── Calendar (syncToken polling)                   │
│  ├── LinkedIn (Unipile API polling)                 │
│  └── Hacker News (HN API polling)                   │
└────────────────────────────────────────────────────┘
```

## Building from Source

```bash
# Build and install to ~/bin
./scripts/build-install.sh

# Or specify a custom directory
./scripts/build-install.sh /usr/local/bin
```

```powershell
# Windows (PowerShell)
.\scripts\build-install.ps1

# Or specify a custom directory
.\scripts\build-install.ps1 -InstallDir "$HOME\\bin"
```

## Commands

### Core

| Command | Description |
|---------|-------------|
| `void inbox` | Unarchived messages across all connectors |
| `void search <query>` | Full-text search (FTS5) |
| `void messages <id>` | Messages in a conversation |
| `void conversations` | List conversations |
| `void contacts` | List contacts |
| `void channels` | List channels and groups (excluding DMs) |
| `void calendar` | Today's events |
| `void calendar week` | This week's events |

### Actions

| Command | Description |
|---------|-------------|
| `void send` | Send a new message |
| `void reply <id>` | Reply to a message (`--in-thread` for threaded replies) |
| `void forward <id>` | Forward a message to another recipient |
| `void archive <id>` | Archive a message (mark as processed) |
| `void mute <target>` | Mute conversations/channels (hides from inbox) |
| `void mute --unmute` | Unmute previously muted conversations |
| `void mute --list` | List all muted conversations |

### Connector-Specific

| Command | Description |
|---------|-------------|
| `void gmail search` | Search Gmail (Gmail query syntax) |
| `void gmail thread <id>` | View a full email thread |
| `void gmail url <id>` | Generate Gmail web URL for a thread |
| `void gmail labels` | List Gmail labels |
| `void gmail label <id>` | Modify labels on a thread or message |
| `void gmail batch-modify` | Batch modify labels on multiple messages |
| `void gmail drafts` | List drafts |
| `void gmail draft create` | Create an email draft (never sends directly) |
| `void gmail draft update <id>` | Update an existing draft |
| `void gmail draft delete <id>` | Delete a draft |
| `void gmail attachment` | Download an attachment |
| `void gmail forward <id>` | Forward a Gmail message to another recipient |
| `void slack react <id>` | Add an emoji reaction |
| `void slack edit <id>` | Edit a Slack message |
| `void slack schedule` | Schedule a message for later |
| `void slack open` | Open a group DM with multiple users |
| `void slack forward <id>` | Forward a Slack message to another channel/user |
| `void whatsapp download <id>` | Download WhatsApp media |
| `void telegram download <id>` | Download Telegram media |
| `void telegram forward <id>` | Forward a Telegram message to another chat |
| `void linkedin download <id>` | Download LinkedIn message media (via Unipile) |
| `void calendar create` | Create a calendar event |
| `void calendar search` | Search calendar events |
| `void calendar respond <id>` | Accept/decline/tentative an invite |
| `void calendar update <id>` | Update an event |
| `void calendar delete <id>` | Delete an event |
| `void calendar availability` | Check attendee availability (FreeBusy) |
| `void calendar calendars` | List available calendars |
| `void hn config` | Show current Hacker News configuration |
| `void hn keywords list\|add\|remove\|set` | Manage watched keywords |
| `void hn min-score <N>` | Set minimum score threshold |
| `void drive download <url>` | Download a file from Google Drive/Docs/Sheets/Slides |
| `void drive info <url>` | Show metadata for a Google Drive file |
| `void drive auth` | Authenticate with Google Drive |

### AI & Automation

| Command | Description |
|---------|-------------|
| `void agent` | Start an interactive AI agent with access to all connectors |
| `void hook list` | List all hooks |
| `void hook create` | Create a hook (LLM prompt triggered by events or schedules) |
| `void hook show <name>` | Show a hook's full configuration |
| `void hook delete <name>` | Delete a hook |
| `void hook enable <name>` | Enable a hook |
| `void hook disable <name>` | Disable a hook |
| `void hook test <name>` | Test a hook (dry-run) |
| `void hook log` | View hook execution logs |

### System

| Command | Description |
|---------|-------------|
| `void setup` | Interactive setup wizard — add, configure, rename connections |
| `void sync` | Run sync in the foreground (Ctrl+C to stop) |
| `void sync --daemon` | Start the background sync daemon |
| `void sync --restart` | Restart the sync daemon |
| `void sync --stop` | Stop the sync daemon |
| `void sync --clear` | Clear database and start fresh |
| `void doctor` | Check configuration and connectivity |
| `void remote status` | Show remote store connection and cache status (requires `store.mode = "remote"`) |
| `void remote refresh` | Force-refresh cached remote config and database snapshot |

### Global Flags

| Flag | Description |
|------|-------------|
| `--connector <type>` | Filter by connector: `slack`, `gmail`, `whatsapp`, `telegram`, `calendar`, `linkedin` (alias: `li`), `hackernews` (alias: `hn`) |
| `--connection <id>` | Filter by connection ID |
| `-n` / `--size <N>` | Limit number of results (default: 50) |
| `--all` | Include archived items |
| `--include-muted` | Include muted conversations |
| `--store <path>` | Override store directory |
| `--no-context` | Disable context enrichment (related messages) |
| `-v` / `--verbose` | Enable debug logging |

## Configuration

Configuration is stored at a platform-specific default path and created by `void setup`:

- Linux: `~/.config/void/config.toml`
- macOS: prefers existing `~/.config/void/config.toml`, otherwise uses `~/Library/Application Support/void/config.toml`
- Windows: `%APPDATA%\\void\\config.toml`

Void keeps backward compatibility with existing Unix-style paths when those directories already exist.

```toml
[store]
path = "~/.local/share/void"

[sync]
gmail_poll_interval_secs = 30
calendar_poll_interval_secs = 60
hackernews_poll_interval_secs = 3600
linkedin_poll_interval_secs = 1800
linkedin_backfill_days = 15

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
id = "mgaudin@gladia.io"
type = "gmail"
credentials_file = "~/.config/void/google-credentials.json"

[[connections]]
id = "mgaudin@gladia.io-calendar"
type = "calendar"
credentials_file = "~/.config/void/google-credentials.json"
calendar_ids = ["primary"]

[[connections]]
id = "hackernews"
type = "hackernews"
keywords = ["rust", "ai", "startup"]
min_score = 100

[[connections]]
id = "linkedin"
type = "linkedin"
api_key = "your-unipile-api-key"
dsn = "https://api1.unipile.com:13111"
account_id = "your-unipile-account-id"
```

### `ignore_conversations`

Any connection can include an `ignore_conversations` list. Matching conversations are auto-muted on every sync start (case-insensitive substring match on name or external ID). This is useful for permanently silencing noisy groups or channels:

```toml
[[connections]]
id = "whatsapp"
type = "whatsapp"
ignore_conversations = ["noisy-group@g.us", "spam", "social"]
```

You can also mute/unmute conversations interactively with `void mute` (see [Write Commands](#write-commands)).

## Connector Setup

### WhatsApp

No external credentials needed. Run `void setup`, select WhatsApp, and scan the QR code with your phone (WhatsApp > Linked Devices > Link a Device).

### Telegram

No external credentials needed. Run `void setup`, select Telegram, and scan the QR code with your phone (Telegram > Settings > Devices > Link Desktop Device).

### Slack

Create a Slack app with a **user token** (`xoxp-`) and an **app-level token** (`xapp-`). Add both tokens through `void setup`.

### Gmail & Google Calendar

Built-in OAuth2 credentials are included — no Google Cloud setup required:

1. Run `void setup` and select Gmail or Calendar
2. Accept the default built-in credentials (or provide your own Google Cloud credentials file)
3. Complete the OAuth flow in your browser

Gmail and Calendar share the same OAuth credentials.

### LinkedIn (Unipile)

LinkedIn messages are synced through the [Unipile](https://www.unipile.com/) API. You need a Unipile account with a connected LinkedIn profile.

1. Sign up at [dashboard.unipile.com](https://dashboard.unipile.com)
2. Connect your LinkedIn account in the Unipile dashboard
3. Copy your **API key**, **DSN** (API base URL), and **account ID**
4. Run `void setup`, select LinkedIn, and paste the credentials

```toml
[[connections]]
id = "linkedin"
type = "linkedin"
api_key = "your-unipile-api-key"
dsn = "https://api1.unipile.com:13111"
account_id = "your-unipile-account-id"
```

Send messages with `void send --via linkedin --to <chat-id-or-linkedin-member-id> --message "..."`. For new conversations with a connection, use the recipient's LinkedIn provider ID (often starts with `ACo`). For existing chats, use the Unipile chat ID or void conversation external ID.

In addition to DMs, sync pulls **comments on your own posts** (Unipile Posts & Comments API). Each post appears as a thread conversation (`kind: thread`); comments are messages with `metadata.source = linkedin_post_comment`. Reply to a comment with `void reply` (same as DMs).

### Hacker News

No credentials needed — the HN API is public. Run `void setup`, select Hacker News, enter keywords to watch and a minimum score threshold. Stories matching your keywords and exceeding the minimum score will appear in your inbox during each sync cycle.

```toml
[[connections]]
id = "hackernews"
type = "hackernews"
keywords = ["rust", "ai", "startup"]
min_score = 100
```

## Data Storage

All data is stored locally in the configured `store.path` directory:

- **Database**: `<store.path>/void.db` (SQLite with WAL mode)
- **WhatsApp sessions**: `<store.path>/whatsapp-*.db`
- **Telegram sessions**: `<store.path>/telegram-*.json`
- **OAuth tokens**: `<store.path>/*-token.json`
- **Config**: platform default path (see Configuration section)

Default `store.path` values:
- Linux/macOS: `~/.local/share/void` (legacy-compatible default on Unix)
- Windows: `%APPDATA%\\void` unless a legacy Unix-style path already exists

No external database or Docker required.

### Remote store (SSH)

Run sync on a home server but use `void` locally against the same data. The local machine keeps a thin client profile; the authoritative `config.toml`, credentials, and database live on the server.

```toml
# Local ~/.config/void/config.toml
[store]
mode = "remote"

[store.remote]
host = "homeserver"
user = "mgaudin"
remote_config_path = "~/.config/void/config.toml"

[store.remote.cache]
path = "~/.cache/void/remote/homeserver"
config_ttl_secs = 300
database_ttl_secs = 30
```

On the server, configure and run sync as usual (`void setup`, `void sync --daemon`). Locally:

- **Read commands** (`inbox`, `search`, `messages`, …) use a cached DB snapshot pulled over SSH
- **Write commands** (`send`, `reply`, `archive`, …) are proxied to the server via SSH
- **File attachments** — `--file` on proxied sends/replies/drafts is staged to the remote store over SCP before the command runs; download commands (`gmail attachment`, `whatsapp download`, `telegram download`, `linkedin download`, `drive download --output`) write to a remote temp path and the result is pulled back to your local `--out` / `--output` path
- **`void remote status`** — SSH, cache age, remote daemon state
- **`void remote refresh`** — force-refresh config + DB snapshot

Global flags:
- `--config <path>` — local client profile (default: platform config path)
- `--store <path>` — override remote store path (local mode: override `store.path`)

## Development

```bash
cargo fmt           # Format
cargo clippy        # Lint
cargo test          # Test
cargo build --release  # Build release
```

### Workspace Structure

```
crates/
  void-core/       # Shared: config, DB, models, hooks, Connector trait, SyncEngine
  void-cli/        # Binary: clap commands, output formatting
  void-slack/      # Slack connector: Web API client
  void-gmail/      # Gmail connector: OAuth2, API client
  void-calendar/   # Calendar connector: shared OAuth, API client
  void-whatsapp/   # WhatsApp connector: wa-rs integration
  void-telegram/   # Telegram connector: grammers MTProto integration
  void-gdrive/     # Google Drive connector: download, export, metadata
  void-hackernews/ # Hacker News connector: keyword-filtered story monitoring
  void-linkedin/  # LinkedIn connector: Unipile REST API integration
  void-agent/      # AI agent: LLM-powered interactive assistant with tool access
```

## License

Copyright (C) 2026 Maxime Gaudin

This program is free software: you can redistribute it and/or modify it under the terms of the [GNU General Public License v3.0](https://www.gnu.org/licenses/gpl-3.0.html) as published by the Free Software Foundation.

See [LICENSE](LICENSE) for the full text.
