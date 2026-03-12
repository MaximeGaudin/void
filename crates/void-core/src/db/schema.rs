//! Database schema and migrations.

use rusqlite::{Connection, OptionalExtension};
use tracing::debug;

pub const SCHEMA_VERSION: i32 = 7;

/// Run all pending migrations on the database connection.
pub fn run_migrations(conn: &Connection) -> anyhow::Result<()> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS schema_version (version INTEGER NOT NULL);")?;

    let current: Option<i32> = conn
        .query_row(
            "SELECT version FROM schema_version ORDER BY version DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()?;

    let version = current.unwrap_or(0);
    tracing::info!(
        current_version = version,
        target_version = SCHEMA_VERSION,
        "running migrations"
    );
    if version < 1 {
        migrate_v1(conn)?;
    }
    if version < 2 {
        migrate_v2(conn)?;
    }
    if version < 3 {
        migrate_v3(conn)?;
    }
    if version < 4 {
        migrate_v4(conn)?;
    }
    if version < 5 {
        migrate_v5(conn)?;
    }
    if version < 6 {
        migrate_v6(conn)?;
    }
    if version < 7 {
        migrate_v7(conn)?;
    }
    Ok(())
}

fn migrate_v1(conn: &Connection) -> anyhow::Result<()> {
    debug!("running migration v1");
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS conversations (
            id              TEXT PRIMARY KEY,
            account_id      TEXT NOT NULL,
            external_id     TEXT NOT NULL,
            name            TEXT,
            kind            TEXT NOT NULL,
            last_message_at INTEGER,
            unread_count    INTEGER NOT NULL DEFAULT 0,
            metadata        TEXT,
            UNIQUE(account_id, external_id)
        );

        CREATE TABLE IF NOT EXISTS messages (
            id              TEXT PRIMARY KEY,
            conversation_id TEXT NOT NULL REFERENCES conversations(id),
            account_id      TEXT NOT NULL,
            external_id     TEXT NOT NULL,
            sender          TEXT NOT NULL,
            sender_name     TEXT,
            body            TEXT,
            timestamp       INTEGER NOT NULL,
            is_from_me      INTEGER NOT NULL DEFAULT 0,
            reply_to_id     TEXT,
            media_type      TEXT,
            metadata        TEXT,
            UNIQUE(account_id, external_id)
        );

        CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts USING fts5(
            body,
            sender_name,
            content=messages,
            content_rowid=rowid
        );

        CREATE TRIGGER IF NOT EXISTS messages_ai AFTER INSERT ON messages BEGIN
            INSERT INTO messages_fts(rowid, body, sender_name)
            VALUES (new.rowid, new.body, new.sender_name);
        END;

        CREATE TRIGGER IF NOT EXISTS messages_ad AFTER DELETE ON messages BEGIN
            INSERT INTO messages_fts(messages_fts, rowid, body, sender_name)
            VALUES ('delete', old.rowid, old.body, old.sender_name);
        END;

        CREATE TRIGGER IF NOT EXISTS messages_au AFTER UPDATE ON messages BEGIN
            INSERT INTO messages_fts(messages_fts, rowid, body, sender_name)
            VALUES ('delete', old.rowid, old.body, old.sender_name);
            INSERT INTO messages_fts(rowid, body, sender_name)
            VALUES (new.rowid, new.body, new.sender_name);
        END;

        CREATE TABLE IF NOT EXISTS events (
            id              TEXT PRIMARY KEY,
            account_id      TEXT NOT NULL,
            external_id     TEXT NOT NULL,
            title           TEXT NOT NULL,
            description     TEXT,
            location        TEXT,
            start_at        INTEGER NOT NULL,
            end_at          INTEGER NOT NULL,
            all_day         INTEGER NOT NULL DEFAULT 0,
            attendees       TEXT,
            status          TEXT,
            calendar_name   TEXT,
            meet_link       TEXT,
            metadata        TEXT,
            UNIQUE(account_id, external_id)
        );

        CREATE TABLE IF NOT EXISTS sync_state (
            account_id      TEXT NOT NULL,
            key             TEXT NOT NULL,
            value           TEXT NOT NULL,
            PRIMARY KEY(account_id, key)
        );

        INSERT INTO schema_version (version) VALUES (1);
    ",
    )?;
    Ok(())
}

fn migrate_v2(conn: &Connection) -> anyhow::Result<()> {
    debug!("running migration v2");
    conn.execute_batch(
        "
        ALTER TABLE messages ADD COLUMN synced_at INTEGER;
        ALTER TABLE events ADD COLUMN synced_at INTEGER;

        INSERT OR REPLACE INTO schema_version (version) VALUES (2);
    ",
    )?;
    Ok(())
}

fn migrate_v3(conn: &Connection) -> anyhow::Result<()> {
    debug!("running migration v3");
    conn.execute_batch(
        "
        ALTER TABLE messages ADD COLUMN is_read INTEGER NOT NULL DEFAULT 0;
        ALTER TABLE messages ADD COLUMN is_archived INTEGER NOT NULL DEFAULT 0;

        INSERT OR REPLACE INTO schema_version (version) VALUES (3);
    ",
    )?;
    Ok(())
}

fn migrate_v4(conn: &Connection) -> anyhow::Result<()> {
    debug!("running migration v4");
    conn.execute_batch(
        "
        ALTER TABLE conversations ADD COLUMN connector TEXT NOT NULL DEFAULT '';
        ALTER TABLE messages ADD COLUMN connector TEXT NOT NULL DEFAULT '';
        ALTER TABLE events ADD COLUMN connector TEXT NOT NULL DEFAULT '';

        INSERT OR REPLACE INTO schema_version (version) VALUES (4);
    ",
    )?;
    Ok(())
}

fn migrate_v5(conn: &Connection) -> anyhow::Result<()> {
    debug!("running migration v5");
    conn.execute_batch(
        "
        ALTER TABLE conversations ADD COLUMN is_muted INTEGER NOT NULL DEFAULT 0;

        INSERT OR REPLACE INTO schema_version (version) VALUES (5);
    ",
    )?;
    Ok(())
}

fn migrate_v6(conn: &Connection) -> anyhow::Result<()> {
    debug!("running migration v6");
    conn.execute_batch(
        "
        ALTER TABLE messages ADD COLUMN context_id TEXT;
        CREATE INDEX IF NOT EXISTS idx_messages_context_id ON messages(context_id);

        INSERT OR REPLACE INTO schema_version (version) VALUES (6);
    ",
    )?;
    Ok(())
}

fn migrate_v7(conn: &Connection) -> anyhow::Result<()> {
    debug!("running migration v7: drop is_read and is_from_me");
    conn.execute_batch(
        "
        ALTER TABLE messages DROP COLUMN is_read;
        ALTER TABLE messages DROP COLUMN is_from_me;

        INSERT OR REPLACE INTO schema_version (version) VALUES (7);
    ",
    )?;
    Ok(())
}
