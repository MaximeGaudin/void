# Connector setup

Every connector is added through the same flow: run `void setup`, pick the service, follow the prompts. This page covers what each service needs.

Most connectors need nothing but that wizard. Slack, Reddit, GitHub, Circleback and Withings first need credentials registered with the service itself — each section below walks through that registration, field by field, before the wizard.

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
| [Circleback](#circleback) | Circleback API key | Circleback API polling |
| [Withings](#withings) | Withings app OAuth | Withings API polling |

## WhatsApp

No external credentials needed.

1. Run `void setup` and select WhatsApp
2. Scan the QR code with your phone: **WhatsApp → Linked Devices → Link a Device**

The sync daemon keeps the linked session in `unavailable` presence so contacts do not see you as permanently online (and phone push notifications keep working). Sending a message may briefly flip presence; void re-asserts unavailable shortly after.

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

Gmail API calls from the CLI and the sync daemon share a per-account token bucket in the local store (~90 requests / minute) so they do not stampede after a quota error.

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

## Circleback

[Circleback](https://circleback.ai) records and summarizes meetings. The connector pulls them
read-only: every meeting becomes a conversation holding its notes, its action items and, when
enabled, the full transcript — so past meetings are searchable next to your messages.

1. Open Circleback → Settings → API and create an API key
2. Run `void setup`, select Circleback, and paste the key

```toml
[[connections]]
id = "circleback"
type = "circleback"
api_key = "cb_..."
backfill_days = 365
include_transcript = true
```

| Setting | Default | Meaning |
|---------|---------|---------|
| `api_key` | — | required; the key from Circleback → Settings → API |
| `backfill_days` | 365 | how far back the first sync reaches |
| `include_transcript` | `true` | import each speaker turn as a message; set to `false` to keep only notes and action items |

Each meeting yields one conversation named after the meeting, with:

- a **notes** message: title, duration, attendees, meeting URL, then Circleback's summary
- an **action items** message: a checklist with the assignee of each item
- one message per **transcript** turn, attributed to the speaker, ordered by timestamp

Meetings still being processed by Circleback are skipped and picked up on a later poll. A meeting
already imported is re-imported only when Circleback changes it, and its transcript is fetched once.
The connector is read-only: `void send` and `void reply` refuse a Circleback conversation.

Turning `include_transcript` on after the first import does not backfill transcripts for meetings
already stored: a meeting is only re-read when Circleback changes it. To fetch them, clear the
connector's state first with `void sync --clear-connector circleback`, then sync again.

```bash
void inbox --connector circleback
void search "pricing" --connector circleback
```

## Withings

[Withings](https://www.withings.com) scales, blood-pressure monitors, watches and sleep mats feed
the Health Mate API. The connector pulls them read-only: each data stream becomes a conversation,
and each measurement group, day, night, workout session, ECG recording or device becomes a message.

Withings has no shared credentials to lend, so this takes one detour through their developer
dashboard. Budget five minutes; the walkthrough below leaves nothing to guess.

### 1. Register a Withings application

Sign in at <https://developer.withings.com/dashboard> and create an application. The form asks for
more than you need — only three fields matter:

| Field | What to put | Why |
|-------|-------------|-----|
| Target environment | **Development** | Production requires a public HTTPS callback; void runs on your machine |
| Application name / description | Anything, e.g. `Void` | Shown only to you, on the consent screen |
| Registered URLs | **`http://localhost:8765/callback`** | Must match byte for byte — Withings compares the string exactly, a trailing slash or a different port fails the exchange |

Withings will warn you that *"HTTP URLs, localhost and ports other than 80 or 443 are not supported
for applications running in production […] your application will be restricted to 10 users"*.
That is expected and harmless here: the application is yours, and it has exactly one user — you.

When asked for scopes, tick all three:

```
user.info      user.metrics      user.activity
```

Leaving one out does not fail the setup — it silently empties a stream. `user.metrics` carries
weight and blood pressure, `user.activity` carries steps, sleep, workouts and ECG.

Once the application is created, the dashboard shows a **Client ID** and a **Client Secret**. Keep
that tab open; the next step asks for both.

### 2. Run the wizard

```bash
void setup
```

Pick **Add a connection** → **Withings**, then answer four prompts:

1. **Withings client ID** — paste from the dashboard
2. **Withings client secret** — paste from the dashboard
3. **Streams** — press Enter for all six, or name a subset (`measures, sleep`)
4. **Backfill days** and **Account name** — press Enter for `365` and `withings`

Your browser then opens on the Withings consent screen. Approve access, and the tab confirms the
authorization. Withings gives only about 30 seconds to redeem an authorization code, so the wizard
exchanges it the instant the redirect lands — do not leave the consent screen waiting.

If the browser does not open, the wizard prints the URL; paste it manually. If the port is taken,
free it (`lsof -nP -iTCP:8765 -sTCP:LISTEN`) and re-run the wizard.

### 3. Check it took

```bash
void setup     # → Show configuration: credentials redacted, streams listed
void doctor    # → Withings credentials valid
void sync      # first sync backfills a year
void inbox --connector withings
```

The resulting config:

```toml
[[connections]]
id = "withings"
type = "withings"
client_id = "..."
client_secret = "..."
streams = ["measures", "activity", "sleep", "workouts", "heart", "devices"]
backfill_days = 365
```

| Setting | Default | Meaning |
|---------|---------|---------|
| `client_id` | — | required; from the Withings developer dashboard |
| `client_secret` | — | required; from the same place |
| `streams` | all six | any of `measures`, `activity`, `sleep`, `workouts`, `heart`, `devices` — omit the key for all of them |
| `backfill_days` | 365 | how far back the first sync reaches |

### What lands in the inbox

Each stream is one conversation:

- **Body measurements** — one message per measurement group: weight, fat ratio, muscle and bone
  mass, hydration, blood pressure, heart rate, SpO2, pulse wave velocity, vascular age
- **Daily activity** — one message per day: steps, distance, calories, active time, average heart rate
- **Sleep** — one message per night: time asleep, sleep score, deep and REM time, time awake,
  average heart rate
- **Workouts** — one message per session: sport, duration, moving time, distance, calories,
  average and max heart rate
- **Heart & ECG** — one message per recording: the AFib verdict for an ECG, heart rate, and blood
  pressure when the cuff reported it
- **Devices** — one message per linked device, rewritten on each poll: model, kind, battery level
  (`high`/`medium`/`low`, Withings gives no percentage) and last session

Every raw field is kept in the message metadata, so `void messages --json` gives you the numbers
without re-parsing the rendered line.

Measurements a scale could not attribute to a user (someone else stepped on it) are skipped. The
day and the night just recorded keep changing after they are first imported, so the connector
re-reads the last 7 days of activity, sleep, workouts and heart data on every poll and updates
those rows in place; body measurements use Withings' own `lastupdate` cursor and are only re-read
when Withings changes them.

The connector is read-only: `void send` and `void reply` refuse a Withings conversation.

```bash
void inbox --connector withings
void messages withings-sleep
void search "weight" --connector withings
```

### Tokens, and why one application per program

Withings rotates the refresh token on every refresh and kills the previous one. Tokens therefore
live in `<store>/<connection-id>-withings-token.json` (owner-only), never in `config.toml`. void
re-reads that file before every call and serializes refreshes on a lock file next to it (`flock` on
Unix, an exclusive open on Windows), so the sync daemon and a CLI command running side by side
cannot invalidate each other.

A *separate* program holding the same grant — another machine, a hosted MCP server, a sync script —
has no way to see that rotation, and the first refresh kills it. Give each program its own Withings
application; they are free and take a minute to register.

If a grant is lost anyway, re-authorize with `void setup` → **Re-authenticate a connection** →
your Withings connection. That reruns the browser flow and rewrites the token file.

### When something is off

| Symptom | Cause | Fix |
|---------|-------|-----|
| `no Withings token at …` | the OAuth flow never completed | `void setup` → Re-authenticate a connection |
| `the Withings refresh token was rejected` | another program refreshed the same grant, or access was revoked in the Health Mate app | re-authorize, then give that program its own application |
| Consent screen returns an error page | the registered URL does not match `http://localhost:8765/callback` exactly | fix it in the dashboard, re-run the wizard |
| Sleep, workouts or activity stay empty | `user.activity` was not granted | add the scope in the dashboard, then re-authenticate |
| `could not listen on 127.0.0.1:8765` | another process holds the port | `lsof -nP -iTCP:8765 -sTCP:LISTEN`, stop it, re-run |
| `Withings is rate limiting this app` (status 601) | too many calls | the connector retries on its own; raise `withings_poll_interval_secs` if it persists |

## Multiple accounts

Add as many connections as you want, including several of the same type. Target a specific one anywhere with `--connection <id>`:

```bash
void inbox --connection work-slack
void gmail search "newer_than:7d" --connection you@gmail.com
```

## Adding a new connector

Want to wire in a new service? See [Adding a connector](adding-a-connector.md).
