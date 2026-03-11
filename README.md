# Void CLI

A unified command-line interface for interacting with WhatsApp, Slack, Gmail, and Google Calendar from a single tool.

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
│  ├── void calendar       ├── void archive           │
│  ├── void contacts       ├── void read              │
│  ├── void channels       ├── void calendar create   │
│  └── void messages       ├── void gmail draft ...   │
│                          ├── void slack react ...    │
│  Sync daemon             └── void whatsapp download │
│  ├── WhatsApp (wa-rs WebSocket)                     │
│  ├── Slack (Web API polling)                        │
│  ├── Gmail (history.list polling)                   │
│  └── Calendar (syncToken polling)                   │
└────────────────────────────────────────────────────┘
```

## Quick Start

```bash
# Build
cargo build --release

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
| `void read <id>` | Mark a message as read |
| `void archive <id>` | Archive a message (mark as processed) |

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
| `void whatsapp download <id>` | Download WhatsApp media |
| `void calendar create` | Create a calendar event |
| `void calendar search` | Search calendar events |
| `void calendar respond <id>` | Accept/decline/tentative an invite |
| `void calendar update <id>` | Update an event |
| `void calendar delete <id>` | Delete an event |

### System

| Command | Description |
|---------|-------------|
| `void setup` | Interactive setup wizard — add, configure, rename accounts |
| `void sync` | Start background sync daemon |
| `void sync --restart` | Restart the sync daemon |
| `void sync --stop` | Stop the sync daemon |
| `void sync --clear` | Clear database and start fresh |
| `void doctor` | Check configuration and connectivity |
| `void install` | Install the void binary into your PATH |

### Global Flags

| Flag | Description |
|------|-------------|
| `--pretty` | Human-readable table output (default is JSON) |
| `--connector <type>` | Filter by connector: `slack`, `gmail`, `whatsapp`, `calendar` |
| `--account <id>` | Filter by account ID |
| `-n` / `--size <N>` | Limit number of results (default: 50) |
| `--all` | Include archived items |
| `--include-muted` | Include muted conversations |
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

### Slack

Create a Slack app with a **user token** (`xoxp-`) and an **app-level token** (`xapp-`). Add both tokens through `void setup`.

### Gmail & Google Calendar

1. Create OAuth2 credentials in [Google Cloud Console](https://console.cloud.google.com/) (Desktop application type)
2. Download the credentials JSON file
3. Run `void setup` and provide the credentials file path
4. Complete the OAuth flow in your browser

Gmail and Calendar can share the same Google Cloud OAuth credentials file.

## Data Storage

All data is stored locally:

- **Database**: `~/.local/share/void/void.db` (SQLite with WAL mode)
- **WhatsApp sessions**: `~/.local/share/void/whatsapp-*.db`
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
  void-core/       # Shared: config, DB, models, Connector trait, SyncEngine
  void-cli/        # Binary: clap commands, output formatting
  void-slack/      # Slack connector: Web API client
  void-gmail/      # Gmail connector: OAuth2, API client
  void-calendar/   # Calendar connector: shared OAuth, API client
  void-whatsapp/   # WhatsApp connector: wa-rs integration
```

## License

MIT
