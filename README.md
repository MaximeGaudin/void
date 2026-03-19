# Void CLI

A unified command-line interface for interacting with WhatsApp, Telegram, Slack, Gmail, Google Calendar, and Google Drive from a single tool — plus an AI agent and LLM-powered hooks.

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
│                          ├── void slack react/edit  │
│  AI & Automation         ├── void slack schedule     │
│  ├── void agent          ├── void drive download    │
│  └── void hook           ├── void whatsapp download │
│                          └── void telegram download │
│                                                     │
│  Sync daemon                                        │
│  ├── WhatsApp (wa-rs WebSocket)                     │
│  ├── Telegram (grammers MTProto)                    │
│  ├── Slack (Socket Mode WebSocket)                  │
│  ├── Gmail (history.list polling)                   │
│  └── Calendar (syncToken polling)                   │
└────────────────────────────────────────────────────┘
```

## Quick Start

```bash
# Build and install to ~/bin
./scripts/build-install.sh

# Or specify a custom directory
./scripts/build-install.sh /usr/local/bin
```

```bash
# Interactive setup — configure connectors, authenticate accounts
void setup

# Start background sync
void sync

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
| `void gmail draft create` | Create an email draft (never sends directly) |
| `void gmail labels` | List Gmail labels |
| `void gmail attachment` | Download an attachment |
| `void slack react <id>` | Add an emoji reaction |
| `void slack edit <id>` | Edit a Slack message |
| `void slack schedule` | Schedule a message for later |
| `void slack open` | Open a group DM with multiple users |
| `void whatsapp download <id>` | Download WhatsApp media |
| `void telegram download <id>` | Download Telegram media |
| `void calendar create` | Create a calendar event |
| `void calendar search` | Search calendar events |
| `void calendar respond <id>` | Accept/decline/tentative an invite |
| `void calendar update <id>` | Update an event |
| `void calendar delete <id>` | Delete an event |
| `void calendar availability` | Check attendee availability (FreeBusy) |
| `void calendar calendars` | List available calendars |
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

### System

| Command | Description |
|---------|-------------|
| `void setup` | Interactive setup wizard — add, configure, rename accounts |
| `void sync` | Start background sync daemon |
| `void sync --restart` | Restart the sync daemon |
| `void sync --stop` | Stop the sync daemon |
| `void sync --clear` | Clear database and start fresh |
| `void doctor` | Check configuration and connectivity |

### Global Flags

| Flag | Description |
|------|-------------|
| `--connector <type>` | Filter by connector: `slack`, `gmail`, `whatsapp`, `telegram`, `calendar` |
| `--account <id>` | Filter by account ID |
| `-n` / `--size <N>` | Limit number of results (default: 50) |
| `--all` | Include archived items |
| `--include-muted` | Include muted conversations |
| `--store <path>` | Override store directory |
| `--no-context` | Disable context enrichment (related messages) |
| `-v` / `--verbose` | Enable debug logging |

## Configuration

Configuration lives at `~/.config/void/config.toml`. Use `void setup` to create and manage it interactively.

```toml
[store]
path = "~/.local/share/void"

[sync]
gmail_poll_interval_secs = 30
calendar_poll_interval_secs = 60

[[accounts]]
id = "whatsapp"
type = "whatsapp"

[[accounts]]
id = "work-slack"
type = "slack"
app_token = "xapp-1-..."
user_token = "xoxp-..."

[[accounts]]
id = "telegram"
type = "telegram"

[[accounts]]
id = "mgaudin@gladia.io"
type = "gmail"
credentials_file = "~/.config/void/google-credentials.json"

[[accounts]]
id = "mgaudin@gladia.io-calendar"
type = "calendar"
credentials_file = "~/.config/void/google-credentials.json"
calendar_ids = ["primary"]
```

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

## Data Storage

All data is stored locally:

- **Database**: `~/.local/share/void/void.db` (SQLite with WAL mode)
- **WhatsApp sessions**: `~/.local/share/void/whatsapp-*.db`
- **Telegram sessions**: `~/.local/share/void/telegram-*.json`
- **OAuth tokens**: `~/.local/share/void/*-token.json`
- **Config**: `~/.config/void/config.toml`

No external database or Docker required.

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
  void-agent/      # AI agent: LLM-powered interactive assistant with tool access
```

## License

Copyright (C) 2026 Maxime Gaudin

This program is free software: you can redistribute it and/or modify it under the terms of the [GNU General Public License v3.0](https://www.gnu.org/licenses/gpl-3.0.html) as published by the Free Software Foundation.

See [LICENSE](LICENSE) for the full text.
