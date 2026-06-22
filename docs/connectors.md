# Connector setup

Every connector is added through the same flow: run `void setup`, pick the service, follow the prompts. This page covers what each service needs.

| Connector | Credentials needed | Sync mechanism |
|-----------|--------------------|----------------|
| [WhatsApp](#whatsapp) | None — QR code | wa-rs WebSocket (push) |
| [Telegram](#telegram) | None — QR code | grammers MTProto (push) |
| [Slack](#slack) | Slack app tokens | Socket Mode WebSocket (push) |
| [Gmail](#gmail--google-calendar) | Built-in OAuth (or your own) | `history.list` polling |
| [Google Calendar](#gmail--google-calendar) | Built-in OAuth (or your own) | `syncToken` polling |
| [LinkedIn](#linkedin-unipile) | Unipile API key | Unipile API polling |
| [Hacker News](#hacker-news) | None — public API | HN API polling |
| [Google News](#google-news) | None — public RSS | Google News RSS polling |
| [Reddit](#reddit) | Reddit app OAuth | Reddit API polling |
| [GitHub](#github) | Personal Access Token | GitHub REST API polling |

## WhatsApp

No external credentials needed.

1. Run `void setup` and select WhatsApp
2. Scan the QR code with your phone: **WhatsApp → Linked Devices → Link a Device**

## Telegram

No external credentials needed.

1. Run `void setup` and select Telegram
2. Scan the QR code with your phone: **Telegram → Settings → Devices → Link Desktop Device**

Optionally, set your own `api_id` / `api_hash` in the connection config — see [Configuration](configuration.md#connections).

## Slack

Create a Slack app with a **user token** (`xoxp-…`) and an **app-level token** (`xapp-…`) with Socket Mode enabled. Add both tokens through `void setup`.

```toml
[[connections]]
id = "work-slack"
type = "slack"
app_token = "xapp-1-..."
user_token = "xoxp-..."
```

## Gmail & Google Calendar

Built-in OAuth2 credentials are included — **no Google Cloud setup required**:

1. Run `void setup` and select Gmail or Calendar
2. Accept the default built-in credentials (or provide your own Google Cloud credentials file via `credentials_file`)
3. Complete the OAuth flow in your browser

Gmail and Calendar share the same OAuth credentials, so adding the second one after the first is instant. By default Calendar syncs your primary calendar; list more with `calendar_ids`.

## LinkedIn (Unipile)

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

Send messages with `void send --via linkedin --to <chat-id-or-linkedin-member-id> --message "..."`. For new conversations with a connection, use the recipient's LinkedIn provider ID (often starts with `ACo`). For existing chats, use the Unipile chat ID or the void conversation external ID.

In addition to DMs, sync pulls **comments on your own posts** (Unipile Posts & Comments API). Each post appears as a thread conversation (`kind: thread`); comments are messages with `metadata.source = linkedin_post_comment`. Reply to a comment with `void reply`, same as DMs.

## Hacker News

No credentials needed — the HN API is public. Run `void setup`, select Hacker News, enter keywords to watch and a minimum score threshold. Stories matching your keywords above the score threshold land in your inbox on each sync cycle.

```toml
[[connections]]
id = "hackernews"
type = "hackernews"
keywords = ["rust", "ai", "startup"]
min_score = 100
```

Tune it later without editing the config:

```bash
void hn keywords add "sqlite,local-first"
void hn min-score 150
void hn config
```

## Google News

No credentials needed — Google News exposes a public RSS search feed. Run `void setup`, select Google News, enter keywords to watch, a recency window, and the edition (language + country). Each keyword triggers its own Google News search; matching articles land in your inbox on each sync cycle.

```toml
[[connections]]
id = "googlenews"
type = "googlenews"
keywords = ["intelligence artificielle", "startup"]
when = "7d"          # recency window (e.g. 24h, 7d) — empty for no limit
language = "fr"      # hl parameter
country = "FR"       # gl parameter
```

Tune it later without editing the config:

```bash
void gn keywords add "open source,rust"
void gn when 24h
void gn language en
void gn country US
void gn config
```

To follow several editions (e.g. French and US news), add one connection per edition — each is targetable with `--connection <id>`.

## Reddit

Reddit requires a **web** app registered at [reddit.com/prefs/apps](https://www.reddit.com/prefs/apps) with redirect URI `http://localhost:8765`.

**Read-only mode** uses application-only OAuth (`client_credentials`) with just `client_id` and `client_secret`. Posts from watched subreddits appear in your inbox (one channel conversation per subreddit).

**Commenting mode** (optional during `void setup`) runs a browser OAuth flow and stores a `refresh_token` in config. When enabled, matching posts also sync as thread conversations with comments, and you can reply from the CLI.

OAuth setup tries a local callback on `localhost:8765` first. If the port is busy, the browser cannot open, or you are on a remote machine, setup falls back to printing the authorize URL and asking you to paste the returned code.

Run `void setup`, select Reddit, and enter your client ID, client secret, subreddits, keywords, and minimum score. Optionally enable commenting for the OAuth flow.

```toml
[[connections]]
id = "reddit"
type = "reddit"
client_id = "your-reddit-app-client-id"
client_secret = "your-reddit-app-client-secret"
refresh_token = "stored-by-setup-when-commenting-enabled"  # optional
subreddits = ["rust", "programming", "startups"]
keywords = ["ai", "llm"]
min_score = 50
```

Tune filters later without editing the config:

```bash
void reddit subreddits add "rust,local-first"
void reddit subreddits remove "startups"
void reddit keywords add "ai,llm"
void reddit min-score 100
void reddit config
```

Reply to a synced comment or post (requires `refresh_token`):

```bash
void reply <message-id> --message "Thanks for sharing!"
void send --via reddit --to reddit_reddit_post_abc123 --message "Great post!"
```

`void reply` targets a post-body or comment message inside a synced thread (the conversations created when commenting is enabled). To comment on a post that only appears in the subreddit feed, use `void send --via reddit --to <post-id>` instead.

## GitHub

GitHub syncs actionable activity into your inbox (read-only):

- Open pull requests requesting your review
- Comments on pull requests you authored
- @mentions of your handle

1. Create a [GitHub Personal Access Token](https://github.com/settings/tokens) with at least the `notifications` scope
2. For private repositories, also grant `repo` (classic PAT) or Pull requests read access (fine-grained PAT)
3. Run `void setup`, select GitHub, and paste the token

```toml
[[connections]]
id = "github"
type = "github"
token = "ghp_..."
username = "your-github-handle"
```

Each repository appears as its own conversation. Mute noisy repos with `void mute owner/repo` or add them to `ignore_conversations`:

```toml
ignore_conversations = ["facebook/react", "kubernetes"]
```

## Multiple accounts

Add as many connections as you want, including several of the same type. Target a specific one anywhere with `--connection <id>`:

```bash
void inbox --connection work-slack
void gmail search "newer_than:7d" --connection you@gmail.com
```

## Adding a new connector

Want to wire in a new service? See [Adding a connector](adding-a-connector.md).
