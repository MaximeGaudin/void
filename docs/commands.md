# Command reference

Complete reference for every `void` command. For per-service credential setup, see [Connector setup](connectors.md).

- [Global flags](#global-flags)
- [Common read flags](#common-read-flags)
- [Reading](#reading)
- [Acting](#acting)
- [Calendar](#calendar)
- [Gmail](#gmail)
- [Slack](#slack)
- [WhatsApp](#whatsapp)
- [Telegram](#telegram)
- [LinkedIn](#linkedin)
- [Hacker News](#hacker-news)
- [Hooks](#hooks)
- [System](#system)
- [Remote store](#remote-store)

## Global flags

Available on every command:

| Flag | Description |
|------|-------------|
| `--store <path>` | Override store directory |
| `--config <path>` | Override config file path |
| `-v`, `--verbose` | Enable debug logging |
| `--no-context` | Disable context enrichment (related messages) |

## Common read flags

Most read commands accept:

| Flag | Description |
|------|-------------|
| `--connector <type>` | Filter by connector: `slack`, `gmail`, `whatsapp`, `telegram`, `calendar`, `linkedin` (alias: `li`), `hackernews` (alias: `hn`), `googlenews` (alias: `gn`), `reddit` (alias: `rd`) |
| `--connection <id>` | Filter by connection ID (when you have several accounts of one type) |
| `-n`, `--size <N>` | Limit number of results (default: 50) |
| `--page <N>` | Page through results |
| `--include-muted` | Include muted conversations |

## Reading

| Command | Description |
|---------|-------------|
| `void inbox` | Unarchived messages across all connectors. `--all` includes archived items |
| `void search <query>` | Full-text search (SQLite FTS5) across all messages |
| `void conversations` | List conversations |
| `void messages <id>` | Messages in a conversation. `--since <date>`, `--until <date>` |
| `void contacts [search]` | List or search contacts |
| `void channels [search]` | List channels and groups (excluding DMs) |

## Acting

| Command | Description |
|---------|-------------|
| `void send --via <connector> --to <recipient> --message <text>` | Send a new message. Use `--conversation <id>` instead of `--to` to target an existing void conversation (e.g. WhatsApp notes-to-self / "Message yourself"). `--connection <id>` to pick an account, `--subject` (email), `--file <path>` to attach, `--at <time>` to schedule delivery (Slack only) |
| `void reply <id> --message <text>` | Reply to a message. `--in-thread` for threaded replies, `--file` to attach, `--at` to schedule (Slack only) |
| `void forward <id> --to <recipient>` | Forward a message. `--comment <text>` to add a note |
| `void archive <ids...>` | Archive one or more messages (mark as processed). `--before <date>` and `--connector <type>` for bulk archiving |
| `void mute <targets...>` | Mute conversations/channels (hidden from inbox). `--unmute` to reverse, `--list` to show muted, `--connection`/`--connector` to scope |

## Calendar

`void calendar` with no subcommand lists today's events. Top-level flags: `--day <date>` (`YYYY-MM-DD`, `today`, `tomorrow`, `yesterday`), `--from <date>`, `--to <date>`, `--connection`, `--connector`.

Times are ISO 8601: `2026-03-31T17:00:00`, `2026-03-31 17:00`, or `2026-03-31` (midnight local). Timezone offsets are accepted; without one, local time is assumed.

| Command | Description |
|---------|-------------|
| `void calendar` | Today's events |
| `void calendar week` | This week's events |
| `void calendar create --title <t> --start <time>` | Create an event. `--end` (default: start + 30 min), `--description`, `--attendees`, `--meet` (attach a Google Meet link) |
| `void calendar search <query>` | Search events. `--from`, `--to` |
| `void calendar respond <id> --status <accepted\|declined\|tentative>` | Respond to an invite. `--comment`, `--email` |
| `void calendar update <id>` | Update an event: `--title`, `--description`, `--start`, `--end` |
| `void calendar delete <id>` | Delete an event |
| `void calendar availability --attendees <emails> --from <t> --to <t>` | Check attendee availability (FreeBusy) |
| `void calendar calendars` | List available calendars |

## Gmail

All Gmail subcommands accept `--connection <id>` to target a specific account.

| Command | Description |
|---------|-------------|
| `void gmail search <query>` | Search with Gmail query syntax (`from:`, `newer_than:7d`, …). `--max <N>` |
| `void gmail thread <id>` | View a full email thread |
| `void gmail url <id>` | Generate the Gmail web URL for a thread |
| `void gmail labels` | List labels |
| `void gmail label <id> --add <labels> --remove <labels>` | Modify labels on a thread |
| `void gmail batch-modify <ids...> --add <labels> --remove <labels>` | Batch-modify labels on multiple messages |
| `void gmail drafts` | List drafts. `--max <N>` |
| `void gmail draft create --subject <s> --body <b>` | Create a draft (never sends directly). `--to`, `--file` to attach, `--reply-to <id>` to draft a reply |
| `void gmail draft update <id> --to <t> --subject <s> --body <b>` | Update a draft |
| `void gmail draft delete <id>` | Delete a draft |
| `void gmail attachment <id> <attachment-id> --out <path>` | Download an attachment |
| `void gmail forward <id> --to <recipient>` | Forward a message. `--comment` |

## Slack

All Slack subcommands accept `--connection <id>`.

| Command | Description |
|---------|-------------|
| `void slack react <id> --emoji <name>` | Add an emoji reaction |
| `void slack edit <id> --message <text>` | Edit a message you sent |
| `void slack schedule --channel <c> --message <m> --at <time>` | Schedule a message. `--at` accepts `HH:MM` (today), `YYYY-MM-DD HH:MM`, or a Unix timestamp. `--thread <ts>` for threaded |
| `void slack open --users <u1,u2,...>` | Open a DM or group DM with one or more users |
| `void slack forward <id> --to <channel-or-user>` | Forward a message. `--comment` |
| `void slack saved` | Messages saved for later (Later view). Synced during background sync; requires `search:read` user scope. Items outside the sync window are fetched on demand. `--connection`, `-n`, `--page` |

## WhatsApp

| Command | Description |
|---------|-------------|
| `void whatsapp download <id> --out <path>` | Download media from a message |

## Telegram

| Command | Description |
|---------|-------------|
| `void telegram download <id> --out <path>` | Download media from a message |
| `void telegram forward <id> --to <chat>` | Forward a message. `--comment` |

## LinkedIn

| Command | Description |
|---------|-------------|
| `void linkedin download <id> --out <path>` | Download message media (via Unipile) |

## Hacker News

Tune the watched-keywords feed without editing `config.toml`. See [Connector setup](connectors.md#hacker-news).

| Command | Description |
|---------|-------------|
| `void hn config` | Show current keywords and minimum score |
| `void hn keywords list` | List watched keywords |
| `void hn keywords add <csv>` | Add one or more keywords (comma-separated) |
| `void hn keywords remove <csv>` | Remove one or more keywords (comma-separated) |
| `void hn keywords set <csv>` | Replace all keywords (empty to clear) |
| `void hn min-score <N>` | Set the minimum score threshold for stories |

## Reddit

Tune watched subreddits, keywords, and score threshold without editing `config.toml`. See [Connector setup](connectors.md#reddit).

| Command | Description |
|---------|-------------|
| `void reddit config` | Show current subreddits, keywords, and minimum score (credentials redacted) |
| `void reddit subreddits list` | List watched subreddits |
| `void reddit subreddits add <csv>` | Add one or more subreddits (comma-separated) |
| `void reddit subreddits remove <csv>` | Remove one or more subreddits (comma-separated) |
| `void reddit subreddits set <csv>` | Replace all subreddits |
| `void reddit keywords list` | List watched keywords |
| `void reddit keywords add <csv>` | Add one or more keywords (comma-separated) |
| `void reddit keywords remove <csv>` | Remove one or more keywords (comma-separated) |
| `void reddit keywords set <csv>` | Replace all keywords (empty to clear) |
| `void reddit min-score <N>` | Set the minimum score threshold for posts |

Reply to synced Reddit comments (requires OAuth commenting enabled during setup):

| Command | Description |
|---------|-------------|
| `void reply <message-id> --message "..."` | Reply to a post or comment in a Reddit thread |
| `void send --via reddit --to <post-id> --message "..."` | Post a top-level comment on a Reddit post |

Alias: `void rd …`

## Hooks

LLM automations triggered by new messages or cron schedules. See the full [Hooks guide](hooks.md).

| Command | Description |
|---------|-------------|
| `void hook list` | List all hooks |
| `void hook create --name <n> --trigger <new_message\|schedule> --prompt <text>` | Create a hook. `--prompt-file <path>` instead of inline text, `--connector` to filter trigger, `--cron <expr>` for schedules, `--agent <cli>` (default: `claude`), `--max-turns <N>`, `--active-days`/`--active-start`/`--active-end`/`--active-utc-offset` to restrict when it fires |
| `void hook show <name>` | Show a hook's full configuration |
| `void hook enable <name>` / `void hook disable <name>` | Toggle a hook |
| `void hook delete <name>` | Delete a hook |
| `void hook test <name>` | Dry-run a hook. `--message-id <id>` to test `new_message` hooks against a real message |
| `void hook log` | Execution logs. `-n/--limit`, `--hook <name>`, `--id <log-id>` |

## System

| Command | Description |
|---------|-------------|
| `void setup` | Interactive wizard — add, configure, and rename connections |
| `void sync` | Run sync in the foreground (Ctrl+C to stop) |
| `void sync --daemon` | Start the background sync daemon |
| `void sync --status` | Show daemon status |
| `void sync --restart` | Restart the daemon |
| `void sync --stop` | Stop the daemon |
| `void sync --connectors <list>` | Sync only the listed connectors |
| `void sync --clear` | Clear the database and start fresh |
| `void sync --clear-connector <type>` | Clear one connector's data |
| `void sync --allow-broken` | Keep syncing even if a connector fails |
| `void doctor` | Check configuration and connectivity. `--non-interactive` for scripts/CI |

## Remote store

Requires `store.mode = "remote"` — see the [Remote store guide](remote-store.md).

| Command | Description |
|---------|-------------|
| `void remote status` | SSH connection, cache age, and remote daemon state |
| `void remote refresh` | Force-refresh cached remote config and database snapshot |
