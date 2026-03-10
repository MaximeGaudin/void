# Void CLI

A unified command-line interface for interacting with WhatsApp, Slack, Gmail, and Google Calendar from a single tool.

## Architecture

Void runs a background sync daemon that continuously pulls messages and events from all configured channels into a local SQLite database. CLI read commands query this local database for instant results. Write operations (send, reply, create event) make direct API calls.

```
┌─────────────────────────────────────────────────┐
│                  void CLI                        │
│                                                  │
│  void inbox ──┐                                  │
│  void search ─┤── reads from ── SQLite DB        │
│  void calendar┘                                  │
│                                                  │
│  void send ───┐                                  │
│  void reply ──┤── direct API calls               │
│  void cal create┘                                │
│                                                  │
│  void sync ──── background daemon writes ── DB   │
│      ├── WhatsApp (wa-rs WebSocket)              │
│      ├── Slack (Web API polling)                 │
│      ├── Gmail (history.list polling)            │
│      └── Calendar (syncToken polling)            │
└─────────────────────────────────────────────────┘
```

## Quick Start

```bash
# Build
cargo build --release

# Initialize configuration
void config init

# Edit config to add your accounts
void config edit

# Authenticate (e.g., WhatsApp QR scan)
void auth whatsapp

# Start syncing in the background
void sync &

# Read your unified inbox
void inbox

# Search across all channels
void search "quarterly report"

# Send a message
void send --via slack --to general --message "Hello team"

# View calendar
void calendar
void calendar week
```

## Configuration

Configuration lives at `~/.config/void/config.toml`:

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
id = "personal-gmail"
type = "gmail"
credentials_file = "~/.config/void/gmail-credentials.json"

[[accounts]]
id = "my-calendar"
type = "calendar"
credentials_file = "~/.config/void/calendar-credentials.json"
calendar_ids = ["primary"]
```

## Commands

| Command | Description |
|---------|-------------|
| `void` | Status dashboard |
| `void inbox` | Recent messages across all channels |
| `void conversations` | List all conversations |
| `void messages <id>` | Messages in a conversation |
| `void search <query>` | Full-text search (FTS5) |
| `void send` | Send a new message |
| `void reply` | Reply to a message |
| `void calendar` | Today's events |
| `void calendar week` | This week's events |
| `void calendar create` | Create a calendar event |
| `void sync` | Start background sync daemon |
| `void auth <type>` | Authenticate a channel |
| `void doctor` | Check system health |
| `void config init` | Create default config |
| `void config show` | Show current config |
| `void config edit` | Open config in editor |

### Global Flags

- `--json` — Output as JSON instead of tables
- `--verbose` / `-v` — Enable debug logging
- `--store <path>` — Override store directory

## Channel Setup

### WhatsApp

No external credentials needed. Run `void auth whatsapp` and scan the QR code with your phone (WhatsApp > Linked Devices > Link a Device). Uses [wa-rs](https://crates.io/crates/wa-rs) for the WhatsApp Web protocol.

### Slack

Create a Slack app with a **user token** (`xoxp-`) and an **app-level token** (`xapp-`). The user token ensures you see exactly what you see in Slack. Add both tokens to your config.

### Gmail

1. Create OAuth2 credentials in Google Cloud Console (Desktop application type)
2. Download the credentials JSON file
3. Set `credentials_file` in config to point to it
4. Run `void auth gmail` to complete the OAuth flow

### Google Calendar

Same as Gmail — uses shared Google OAuth2 credentials. Set `calendar_ids` to specify which calendars to sync (use `"primary"` for your main calendar).

## Data Storage

All data is stored locally:

- **Database**: `~/.local/share/void/void.db` (SQLite with WAL mode)
- **WhatsApp sessions**: `~/.local/share/void/whatsapp-*.db`
- **OAuth tokens**: `~/.local/share/void/*-token.json`
- **Config**: `~/.config/void/config.toml`

No external database or Docker required.

## Development

```bash
# Format
cargo fmt

# Lint
cargo clippy -- -D warnings

# Test
cargo test

# Build release
cargo build --release
```

### Workspace Structure

```
crates/
  void-core/       # Shared: config, DB, models, Channel trait, SyncEngine
  void-cli/        # Binary: clap commands, output formatting
  void-slack/      # Slack adapter: Web API client, Channel impl
  void-gmail/      # Gmail adapter: OAuth2, API client, Channel impl
  void-calendar/   # Calendar adapter: shared OAuth, API client, Channel impl
  void-whatsapp/   # WhatsApp adapter: wa-rs integration, Channel impl
```

## License

MIT
