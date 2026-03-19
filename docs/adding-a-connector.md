# Adding a Connector

This guide walks through every file you need to touch when adding a new communication connector to Void. It follows the same pattern used by the WhatsApp, Slack, Gmail, and Calendar connectors.

Use this as a checklist. Each step lists the exact files and code patterns to extend.

Throughout this guide, replace the placeholders:

- `Acme` — PascalCase connector name (e.g. `Telegram`, `WhatsApp`, `Slack`)
- `acme` — lowercase connector name (e.g. `telegram`, `whatsapp`, `slack`)
- `AC` — two-letter badge (e.g. `TG`, `WA`, `SL`)
- `am` — short alias for `--connector` / `--via` (e.g. `tg`, `wa`, `sl`)

---

## Architecture at a Glance

```
Cargo.toml                          # workspace members + deps
crates/
  void-core/src/
    models.rs                       # ConnectorType enum
    config.rs                       # AccountType, AccountSettings, deserialization
    connector.rs                    # Connector trait (the interface you implement)
    db/mod.rs                       # Database layer (connectors use this, don't modify it)
  void-acme/                        # NEW — your connector crate
    Cargo.toml
    src/
      lib.rs
      error.rs
      connector/
        mod.rs                      # Struct + Connector trait impl
        sync.rs                     # Sync / backfill logic
        send.rs                     # Message construction + peer resolution
        media.rs                    # Upload / download
        extract.rs                  # Field extraction from platform messages
  void-cli/src/
    main.rs                         # Command enum
    output.rs                       # parse_connector_type, badge_from_connector
    commands/
      mod.rs                        # Module declarations
      connector_factory.rs          # Builds connectors from config
      setup.rs                      # Interactive setup wizard
      sync.rs                       # Sync command (session cleanup)
      reply.rs                      # Reply ID formatting
      acme.rs                       # NEW — connector-specific subcommands
  void-agent/src/
    prompt.rs                       # AI agent system prompt
    tools.rs                        # AI agent tool descriptions
README.md
```

---

## Step 1 — Register Core Types

### `crates/void-core/src/models.rs`

Add your connector to three places in `ConnectorType`:

**Enum variant:**

```rust
pub enum ConnectorType {
    WhatsApp,
    Slack,
    Gmail,
    Calendar,
    Acme,  // ← add
}
```

**`Display` impl:**

```rust
impl std::fmt::Display for ConnectorType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            // ...existing arms...
            Self::Acme => write!(f, "acme"),
        }
    }
}
```

**`badge()` method:**

```rust
impl ConnectorType {
    pub fn badge(&self) -> &'static str {
        match self {
            // ...existing arms...
            Self::Acme => "AC",
        }
    }
}
```

### `crates/void-core/src/config.rs`

**`AccountType` enum + `Display`:**

```rust
pub enum AccountType {
    // ...existing...
    Acme,
}

// In Display impl:
Self::Acme => write!(f, "acme"),
```

**`AccountSettings` enum — add a variant with your connector's config fields:**

```rust
pub enum AccountSettings {
    // ...existing...
    Acme {
        // Add whatever your connector needs, e.g.:
        // api_key: String,
        // api_secret: String,
    },
}
```

**`Deserialize` impl for `AccountConfig`** — find the match on `raw.account_type` and add:

```rust
AccountType::Acme => AccountSettings::Acme {
    // Map from RawAccountConfig fields
},
```

If your settings use fields not yet in `RawAccountConfig`, add them there too (with `#[serde(default)]`).

**`find_account_by_connector()`** — add to the match:

```rust
"acme" => AccountType::Acme,
```

**`default_config()` template** — add a commented-out example:

```toml
# [[accounts]]
# id = "acme"
# type = "acme"
# api_key = "..."
```

---

## Step 2 — Create the Connector Crate

### Directory structure

```
crates/void-acme/
├── Cargo.toml
└── src/
    ├── lib.rs
    ├── error.rs
    └── connector/
        ├── mod.rs
        ├── sync.rs
        ├── send.rs
        ├── media.rs
        └── extract.rs
```

### `Cargo.toml`

```toml
[package]
name = "void-acme"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true
description = "Acme adapter for Void CLI"

[dependencies]
void-core = { workspace = true }
tokio = { workspace = true }
serde_json = { workspace = true }
tracing = { workspace = true }
anyhow = { workspace = true }
thiserror = { workspace = true }
async-trait = { workspace = true }
tokio-util = { workspace = true }
# Add your platform SDK crate(s) here
```

### `src/lib.rs`

```rust
pub mod connector;
pub mod error;
```

### `src/error.rs`

Follow the same pattern as other connectors — a `thiserror` enum with common categories:

```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AcmeError {
    #[error("authentication error: {0}")]
    Auth(String),
    #[error("connection error: {0}")]
    Connection(String),
    #[error("media error: {0}")]
    Media(String),
    #[error("decode error: {0}")]
    Decode(String),
    #[error("{0}")]
    Other(String),
}
```

---

## Step 3 — Implement the Connector Trait

### `src/connector/mod.rs`

```rust
mod extract;
mod media;
mod send;
mod sync;

use std::sync::Arc;
use async_trait::async_trait;
use tokio_util::sync::CancellationToken;
use void_core::connector::Connector;
use void_core::db::Database;
use void_core::models::{ConnectorType, HealthStatus, MessageContent};

pub struct AcmeConnector {
    config_id: String,
    // Add fields for session path, API clients, credentials, etc.
}

impl AcmeConnector {
    pub fn new(account_id: &str, /* ...platform-specific params... */) -> Self {
        Self {
            config_id: account_id.to_string(),
            // ...
        }
    }
}

#[async_trait]
impl Connector for AcmeConnector {
    fn connector_type(&self) -> ConnectorType {
        ConnectorType::Acme
    }

    fn account_id(&self) -> &str {
        &self.config_id
    }

    async fn authenticate(&mut self) -> anyhow::Result<()> {
        // Interactive auth flow (OAuth, QR code, phone+code, tokens, etc.)
        todo!()
    }

    async fn start_sync(
        &self,
        db: Arc<Database>,
        cancel: CancellationToken,
    ) -> anyhow::Result<()> {
        // Long-running sync loop — see sync.rs
        todo!()
    }

    async fn health_check(&self) -> anyhow::Result<HealthStatus> {
        // Check session validity, connectivity, etc.
        todo!()
    }

    async fn send_message(
        &self,
        to: &str,
        content: MessageContent,
    ) -> anyhow::Result<String> {
        // Send a message, return the platform message ID
        todo!()
    }

    async fn reply(
        &self,
        message_id: &str,
        content: MessageContent,
        in_thread: bool,
    ) -> anyhow::Result<String> {
        // Reply to a message, return the platform message ID
        todo!()
    }

    // Optional overrides (have default implementations):
    // async fn mark_read(&self, external_id, conv_external_id) -> Result<()>
    // async fn archive(&self, external_id, conv_external_id) -> Result<()>
    // async fn forward(&self, external_id, conv_external_id, to, comment) -> Result<String>
}
```

### Trait methods reference

| Method | Required | What to return |
|--------|----------|----------------|
| `connector_type()` | Yes | `ConnectorType::Acme` |
| `account_id()` | Yes | `&self.config_id` |
| `authenticate()` | Yes | Run interactive auth, persist session |
| `start_sync(db, cancel)` | Yes | Backfill history then stream live updates until `cancel` fires |
| `health_check()` | Yes | Return `HealthStatus { ok: true/false, ... }` |
| `send_message(to, content)` | Yes | Send message, return platform message ID as String |
| `reply(message_id, content, in_thread)` | Yes | Reply to message, return platform message ID |
| `mark_read(external_id, conv_external_id)` | No | Default is no-op `Ok(())` |
| `archive(external_id, conv_external_id)` | No | Default is no-op `Ok(())` |
| `forward(external_id, conv_external_id, to, comment)` | No | Default returns "not supported" error |

---

## Step 4 — Implement Sync

### Sync contract

Every connector **must** satisfy three requirements:

| Requirement | Description |
|-------------|-------------|
| **Resumable backfill** | Initial backfill runs once. On subsequent restarts, the connector must skip or shorten the backfill using a persisted cursor (sync token, history ID, or last message timestamp). It must **never** re-fetch the full history window on every restart. |
| **Incremental sync** | After backfill, the connector must ingest new messages — either via a **real-time stream** (WebSocket, long-poll, server push) or **periodic polling**. The sync loop must run until the `CancellationToken` fires. |
| **Log on ingest** | Every newly ingested message must produce an `eprintln!` line in the format `[connector:account_id] timestamp (new|sent) context — sender: preview`. This is the only way to confirm sync is working when the daemon runs in the background. |

### `start_sync` structure

```rust
async fn start_sync(&self, db: Arc<Database>, cancel: CancellationToken) -> anyhow::Result<()> {
    // Phase 1: backfill (skip if sync cursor exists)
    let has_cursor = db.get_sync_state(&self.account_id, "sync_cursor")?.is_some();
    if !has_cursor {
        self.initial_backfill(&db).await?;
    }

    // Phase 2: incremental sync (real-time or polling)
    let mut interval = tokio::time::interval(Duration::from_secs(30));
    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            _ = interval.tick() => {
                self.incremental_sync(&db).await?;
            }
        }
    }
    Ok(())
}
```

For real-time connectors (WebSocket, Telegram update stream, etc.), replace the polling interval with the event stream:

```rust
loop {
    tokio::select! {
        _ = cancel.cancelled() => {
            // Persist update state before exiting
            break;
        }
        event = stream.next() => {
            handle_event(&db, event).await?;
            // Persist sync state after each event (or batch)
        }
    }
}
```

### Persisting sync state

Use `db.get_sync_state` / `db.set_sync_state` to store cursors so the connector can resume:

| Connector | Cursor key | Value | What it enables |
|-----------|-----------|-------|-----------------|
| Gmail | `history_id` | Gmail history ID | `history.list` API picks up where it left off |
| Slack | `backfill_done` | `"1"` | Skips initial backfill; catch-up uses `latest_message_timestamp` |
| Calendar | `sync_token:{cal_id}` | Google sync token | `events.list` returns only changes since token |
| Telegram | (session file) | grammers update state | `stream_updates(catch_up: true)` replays missed updates |

Your connector should follow the same pattern — persist whatever cursor the platform provides after each sync cycle.

### `src/connector/sync.rs`

Key patterns:

```rust
use std::sync::Arc;
use void_core::db::Database;
use void_core::models::{Conversation, ConversationKind, Message};

pub(super) fn handle_message(
    db: &Arc<Database>,
    account_id: &str,
    platform_chat_id: &str,
    platform_msg_id: &str,
    /* platform-specific message type */
) -> anyhow::Result<()> {
    let conv_id = format!("{account_id}-{platform_chat_id}");
    let conv_external_id = format!("acme_{account_id}_{platform_chat_id}");
    let msg_external_id = format!("acme_{account_id}_{platform_msg_id}");

    if db.message_exists(account_id, &msg_external_id)? {
        return Ok(());
    }

    let conv = Conversation {
        id: conv_id.clone(),
        account_id: account_id.to_string(),
        connector: "acme".to_string(),
        external_id: conv_external_id.clone(),
        name: None, // set from platform data
        kind: ConversationKind::Dm,
        last_message_at: None,
        unread_count: 0,
        is_muted: false,
        metadata: None,
    };
    db.upsert_conversation(&conv)?;

    let msg = Message {
        id: format!("{account_id}-{platform_msg_id}"),
        conversation_id: conv_id,
        account_id: account_id.to_string(),
        connector: "acme".to_string(),
        external_id: msg_external_id,
        sender: "sender_id".to_string(),
        sender_name: None, // set from platform data
        body: None,        // set from platform data
        timestamp: 0,      // set from platform data
        synced_at: None,
        is_archived: false,
        reply_to_id: None,
        media_type: None,
        metadata: None,
        context_id: Some(conv_external_id.clone()),
        context: None,
    };
    db.upsert_message(&msg)?;

    // Log the new message (required by sync contract)
    eprintln!("[acme:{account_id}] 2026-01-01 12:00:00 (new) Chat — Sender: preview text");

    Ok(())
}
```

### Conventions for IDs

Every model has two ID fields — `id` (the primary key) and `external_id` (unique with `account_id`). Both **must** be set to non-empty, deterministic values. Setting `id` to `String::new()` will cause `UNIQUE constraint failed: messages.id` on the second insert.

| Field | Conversations | Messages |
|-------|--------------|----------|
| `id` (PK) | `{account_id}-{platform_chat_id}` | `{account_id}-{platform_message_id}` |
| `external_id` | `acme_{account_id}_{platform_chat_id}` | `acme_{account_id}_{platform_message_id}` |
| `conversation_id` | — | Set to the conversation's `id` value |
| `connector` | `"acme"` | `"acme"` |

The `connector` field must match `ConnectorType::Display` (lowercase).

### Database methods your sync will use

| Method | Purpose |
|--------|---------|
| `db.upsert_conversation(&conv)` | Create or update a conversation |
| `db.upsert_message(&msg)` | Create or update a message (triggers hooks for new messages) |
| `db.message_exists(account_id, external_id)` | Dedup check |
| `db.latest_message_timestamp(account_id, connector)` | Incremental sync cursor |
| `db.get_sync_state(account_id, key)` | Read sync cursor/state |
| `db.set_sync_state(account_id, key, value)` | Write sync cursor/state |

**CancellationToken:** Your sync loop must respect the cancellation token. Use `tokio::select!` to race your update stream against `cancel.cancelled()`.

---

## Step 5 — Implement Send & Reply

### `src/connector/send.rs`

```rust
use void_core::models::MessageContent;

/// Build a platform-specific outgoing message from MessageContent.
pub(crate) fn build_message(content: &MessageContent) -> anyhow::Result</* PlatformMessage */> {
    match content {
        MessageContent::Text(text) => {
            // Build text message
            todo!()
        }
        MessageContent::File { path, caption, mime_type } => {
            // Build file/media message
            todo!()
        }
    }
}

/// Resolve a user-provided recipient string to a platform peer.
/// Input may be: phone number, username, email, numeric ID, etc.
pub(crate) fn resolve_peer(/* client, */ input: &str) -> anyhow::Result</* PlatformPeer */> {
    todo!()
}

/// Parse a reply ID string back into (conversation_id, message_id).
/// Format: "{conv_external_id}:{msg_external_id}"
pub(crate) fn parse_reply_id(message_id: &str) -> anyhow::Result<(String, String)> {
    let (conv, msg) = message_id
        .split_once(':')
        .ok_or_else(|| anyhow::anyhow!("invalid reply ID format: {message_id}"))?;
    Ok((conv.to_string(), msg.to_string()))
}
```

---

## Step 6 — Implement Media

### `src/connector/media.rs`

Handle file uploads for sending and downloads for the `void acme download` command:

```rust
/// Upload a file and build a platform media message.
pub(crate) async fn upload_and_build_media_message(
    /* client, */
    path: &std::path::Path,
    caption: Option<&str>,
    mime_type: Option<&str>,
) -> anyhow::Result</* PlatformMessage */> {
    todo!()
}
```

If your connector supports media download, expose a public method on the connector struct:

```rust
impl AcmeConnector {
    pub async fn download_media(
        &self,
        /* platform-specific params from message metadata */
    ) -> Result<Vec<u8>, AcmeError> {
        todo!()
    }
}
```

---

## Step 7 — Implement Extract

### `src/connector/extract.rs`

Extract normalized fields from platform-specific message types:

```rust
/// Extract the text body from a platform message.
pub(crate) fn extract_text(msg: &/* PlatformMessage */) -> Option<String> {
    todo!()
}

/// Determine the media type string (e.g. "image", "video", "document", "audio").
pub(crate) fn extract_media_type(msg: &/* PlatformMessage */) -> Option<String> {
    todo!()
}

/// Extract metadata needed for later media download (stored as JSON in message.metadata).
pub(crate) fn extract_media_metadata(msg: &/* PlatformMessage */) -> Option<serde_json::Value> {
    todo!()
}
```

---

## Step 8 — Wire into the CLI

This is the most file-touching step. Every file below needs a small addition.

### 8.1 `crates/void-cli/Cargo.toml`

Add the dependency:

```toml
[dependencies]
# ...existing...
void-acme = { workspace = true }
```

### 8.2 `crates/void-cli/src/commands/mod.rs`

Add the module:

```rust
pub mod acme;
```

### 8.3 `crates/void-cli/src/main.rs`

Add to the `Command` enum:

```rust
/// Acme-specific operations (media download, etc.)
Acme(commands::acme::AcmeArgs),
```

Add the dispatch arm in `main()`:

```rust
Some(Command::Acme(args)) => commands::acme::run(args).await,
```

### 8.4 `crates/void-cli/src/commands/acme.rs` (new file)

Create the connector-specific subcommand (mirror `whatsapp.rs`):

```rust
use clap::{Args, Subcommand};

#[derive(Debug, Args)]
pub struct AcmeArgs {
    #[command(subcommand)]
    pub command: AcmeCommand,
}

#[derive(Debug, Subcommand)]
pub enum AcmeCommand {
    /// Download media from a message
    Download(DownloadArgs),
}

#[derive(Debug, Args)]
pub struct DownloadArgs {
    /// Message ID (void internal ID or external ID)
    pub message_id: String,
    /// Output file path
    #[arg(long)]
    pub out: String,
    /// Account to use
    #[arg(long)]
    pub account: Option<String>,
}

pub async fn run(args: &AcmeArgs, _json: bool) -> anyhow::Result<()> {
    match &args.command {
        AcmeCommand::Download(a) => run_download(a).await,
    }
}

async fn run_download(args: &DownloadArgs) -> anyhow::Result<()> {
    // 1. Load config and DB
    // 2. Fetch message by ID
    // 3. Validate connector == "acme"
    // 4. Extract media metadata from message.metadata
    // 5. Build connector, call download_media()
    // 6. Write bytes to args.out
    todo!()
}
```

### 8.5 `crates/void-cli/src/commands/connector_factory.rs`

Add the build arm:

```rust
(AccountType::Acme, AccountSettings::Acme { /* destructure fields */ }) => {
    let session_path = store_path.join(format!("acme-{}.session", account.id));
    Ok(Arc::new(void_acme::connector::AcmeConnector::new(
        &account.id,
        // ...pass config fields...
    )))
}
```

### 8.6 `crates/void-cli/src/commands/setup.rs`

**Four functions** need changes:

**`add_connector_account()`** — add to the select menu and match:

```rust
let choice = select(
    "Which connector type?",
    &[
        "Gmail",
        "Slack",
        "WhatsApp",
        "Acme",           // ← add
        "Google Calendar",
        "Google Drive",
    ],
);
// ...
// Add a match arm for your index → setup_acme(cfg, store_path, true).await?
```

**`run_full_wizard()`** — add the setup call in sequence:

```rust
separator();
setup_acme(cfg, store_path, false).await?;
```

**`show_configuration()`** — add display for your settings:

```rust
config::AccountSettings::Acme { /* fields */ } => {
    // eprintln!("    api_key: {}", config::redact_token(api_key));
}
```

**`rename_account()`** — if your connector has a session file, add rename logic:

```rust
if account_type.to_string() == "acme" {
    let old_session = store_path.join(format!("acme-{old_name}.session"));
    let new_session = store_path.join(format!("acme-{new_name}.session"));
    if old_session.exists() {
        std::fs::rename(&old_session, &new_session)?;
    }
}
```

**New `setup_acme()` function** — follow the pattern from `setup_whatsapp()` or `setup_slack()`:

```rust
async fn setup_acme(
    cfg: &mut VoidConfig,
    store_path: &Path,
    add_only: bool,
) -> anyhow::Result<()> {
    eprintln!("🔌  ACME");
    eprintln!();
    eprintln!("Connects your Acme account.");

    // 1. Handle existing accounts (pick_connector_action)
    // 2. Prompt for credentials / config
    // 3. Build AccountConfig with AccountSettings::Acme { ... }
    // 4. Optionally authenticate now
    // 5. Push to cfg.accounts

    todo!()
}
```

### 8.7 `crates/void-cli/src/output.rs`

**`parse_connector_type()`** — add the alias:

```rust
"acme" | "am" => Some(ConnectorType::Acme),
```

**`badge_from_connector()`** — add the badge:

```rust
"acme" => "[AC]".into(),
```

### 8.8 `crates/void-cli/src/commands/reply.rs`

**`build_reply_id()`** — add the format:

```rust
ConnectorType::Acme => format!("{conv_external_id}:{msg_external_id}"),
```

### 8.9 `crates/void-cli/src/commands/sync.rs`

**`--clear_connector` handling** — if your connector has session files, add cleanup:

```rust
if ct == "acme" {
    for account in &cfg.accounts {
        if account.account_type.to_string() == "acme" {
            let session = store_path.join(format!("acme-{}.session", account.id));
            if session.exists() {
                std::fs::remove_file(&session)?;
                eprintln!(
                    "Removed Acme session: {} (will require re-authentication)",
                    session.display()
                );
            }
        }
    }
}
```

---

## Step 9 — Update the AI Agent

### `crates/void-agent/src/prompt.rs`

Update `DEFAULT_SYSTEM_PROMPT`:

- Add your connector to every mention of "Gmail, Slack, WhatsApp, and Google Calendar"
- Add an inbox processing step: `void inbox --connector acme`
- Add to the "NEVER send ... without confirmation" rule if applicable

### `crates/void-agent/src/tools.rs`

Update the `VoidCommandTool` definition string:

- Description: add your connector name
- `inbox` command help: add `|acme` to `--connector`
- `send` command help: add `|acme` to `--via` (if sending is supported)

---

## Step 10 — Workspace Configuration

### `Cargo.toml` (workspace root)

Add to `members`:

```toml
[workspace]
members = [
    # ...existing...
    "crates/void-acme",
]
```

Add to `[workspace.dependencies]`:

```toml
void-acme = { path = "crates/void-acme" }
```

---

## Step 11 — README

Update `README.md`:

- Project tagline / description — add your connector name
- Connectors list / features table
- Setup instructions for your connector
- Usage examples: `--via acme`, `--connector acme`
- Connector-specific commands: `void acme download`

---

## Step 12 — Verify

Run the full build pipeline:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --release
```

Common issues:
- Missing match arms — the compiler will catch exhaustive-match errors for `ConnectorType`, `AccountType`, and `AccountSettings`
- Forgotten `Display`/`badge` arm — causes a runtime panic or incomplete output
- `find_account_by_connector` not updated — connector won't be found when replying or archiving

---

## Read-Only Connectors

Not all connectors support bidirectional communication. For read-only data sources (e.g. Hacker News, RSS feeds), the following simplifications apply:

- **No `send.rs` / `media.rs` / `extract.rs`** — these files can be omitted entirely.
- **`send_message()` and `reply()`** — return `anyhow::bail!("... is a read-only connector")`.
- **`authenticate()`** — no-op `Ok(())` if the API is public.
- **No session files** — skip session cleanup in `sync.rs` and rename logic in `setup.rs`.
- **No connector-specific CLI subcommand** — no `void acme download` etc. The connector only surfaces items in the inbox.
- **Polling sync only** — use `tokio::time::interval` with a configurable interval in `SyncConfig`.

The `void-hackernews` crate is a reference implementation for this pattern.

---

## Checklist

Use this as a final review checklist:

- [ ] `void-core/src/models.rs` — `ConnectorType` enum + `Display` + `badge()`
- [ ] `void-core/src/config.rs` — `AccountType` + `Display` + `AccountSettings` + `Deserialize` + `find_account_by_connector` + `default_config`
- [ ] `void-acme/Cargo.toml` — crate created
- [ ] `void-acme/src/lib.rs` — module declarations
- [ ] `void-acme/src/error.rs` — error enum
- [ ] `void-acme/src/connector/mod.rs` — struct + `Connector` impl
- [ ] `void-acme/src/connector/sync.rs` — backfill + live updates
- [ ] `void-acme/src/connector/send.rs` — message building + peer resolution
- [ ] `void-acme/src/connector/media.rs` — upload + download
- [ ] `void-acme/src/connector/extract.rs` — field extraction
- [ ] `void-cli/Cargo.toml` — dependency added
- [ ] `void-cli/src/commands/mod.rs` — `pub mod acme`
- [ ] `void-cli/src/main.rs` — `Command::Acme` + dispatch
- [ ] `void-cli/src/commands/acme.rs` — subcommand file
- [ ] `void-cli/src/commands/connector_factory.rs` — build arm
- [ ] `void-cli/src/commands/setup.rs` — `setup_acme()` + menu + wizard + show_config + rename
- [ ] `void-cli/src/output.rs` — `parse_connector_type` + `badge_from_connector`
- [ ] `void-cli/src/commands/reply.rs` — `build_reply_id`
- [ ] `void-cli/src/commands/sync.rs` — session cleanup in `--clear_connector`
- [ ] `void-agent/src/prompt.rs` — system prompt updated
- [ ] `void-agent/src/tools.rs` — tool descriptions updated
- [ ] `Cargo.toml` (root) — workspace members + deps
- [ ] `README.md` — updated
- [ ] `cargo fmt && cargo clippy && cargo test && cargo build --release` — all pass
