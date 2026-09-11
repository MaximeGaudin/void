use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;

use void_core::config::{
    redact_token, settings_str, settings_string, settings_u32, ConnectionConfig, SyncConfig,
};
use void_core::connector::Connector;

use super::{ConnectorPlugin, ReplyIdStyle, SetupCtx};

const DEFAULT_POLL_INTERVAL_SECS: u64 = 900;
pub(crate) const DEFAULT_BACKFILL_DAYS: u32 = 365;

inventory::submit! {
    ConnectorPlugin {
        id: void_circleback::CONNECTOR_ID,
        aliases: &["circleback", "cb"],
        menu_label: "Circleback",
        badge: "CB",
        default_poll_interval_secs: Some(DEFAULT_POLL_INTERVAL_SECS),
        reply_id_style: ReplyIdStyle::MsgOnly,
        supports_scheduling: false,
        uses_daemon_rpc: false,
        prompt_token_reauth: false,
        session_files,
        build,
        setup,
        parse_settings,
        show_config,
    }
}

fn session_files(_store: &Path, _connection_id: &str) -> Vec<PathBuf> {
    vec![]
}

pub(crate) fn include_transcript(table: &toml::Table) -> bool {
    table
        .get("include_transcript")
        .and_then(|v| v.as_bool())
        .unwrap_or(true)
}

fn build(
    connection: &ConnectionConfig,
    _store_path: &Path,
    sync: &SyncConfig,
) -> anyhow::Result<Arc<dyn Connector>> {
    let api_key = settings_string(&connection.settings, "api_key").ok_or_else(|| {
        anyhow::anyhow!(
            "missing api_key for Circleback connection '{}'",
            connection.id
        )
    })?;
    let backfill_days =
        settings_u32(&connection.settings, "backfill_days").unwrap_or(DEFAULT_BACKFILL_DAYS);
    let poll_secs =
        sync.poll_interval_secs(void_circleback::CONNECTOR_ID, DEFAULT_POLL_INTERVAL_SECS);
    Ok(Arc::new(
        void_circleback::connector::CirclebackConnector::new(
            &connection.id,
            api_key,
            backfill_days,
            include_transcript(&connection.settings),
            poll_secs,
        ),
    ))
}

fn setup(ctx: SetupCtx<'_>) -> Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + '_>> {
    Box::pin(crate::commands::setup::circleback::setup_circleback(
        ctx.cfg,
        ctx.add_only,
    ))
}

fn parse_settings(table: &toml::Table) -> anyhow::Result<()> {
    match settings_str(table, "api_key") {
        None => anyhow::bail!("missing api_key"),
        Some(k) if k.trim().is_empty() => anyhow::bail!("api_key is empty"),
        Some(_) => {}
    }
    if let Some(v) = table.get("include_transcript") {
        if !v.is_bool() {
            anyhow::bail!("include_transcript must be true or false");
        }
    }
    if let Some(v) = table.get("backfill_days") {
        if !v.is_integer() || v.as_integer().is_some_and(|n| n < 0) {
            anyhow::bail!("backfill_days must be a non-negative integer");
        }
    }
    Ok(())
}

fn show_config(table: &toml::Table, out: &mut dyn std::fmt::Write) -> std::fmt::Result {
    if let Some(key) = settings_str(table, "api_key") {
        writeln!(out, "    api_key:            {}", redact_token(key))?;
    }
    writeln!(
        out,
        "    backfill_days:      {}",
        settings_u32(table, "backfill_days").unwrap_or(DEFAULT_BACKFILL_DAYS)
    )?;
    writeln!(out, "    include_transcript: {}", include_transcript(table))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(toml_src: &str) -> toml::Table {
        toml::from_str(toml_src).unwrap()
    }

    #[test]
    fn parse_settings_requires_api_key() {
        assert!(parse_settings(&table("")).is_err());
        assert!(parse_settings(&table("api_key = \"\"")).is_err());
        assert!(parse_settings(&table("api_key = \"cb_x\"")).is_ok());
    }

    #[test]
    fn parse_settings_validates_optional_fields() {
        assert!(
            parse_settings(&table("api_key = \"cb_x\"\ninclude_transcript = \"yes\"")).is_err()
        );
        assert!(parse_settings(&table("api_key = \"cb_x\"\nbackfill_days = -1")).is_err());
        assert!(parse_settings(&table(
            "api_key = \"cb_x\"\nbackfill_days = 30\ninclude_transcript = false"
        ))
        .is_ok());
    }

    #[test]
    fn include_transcript_defaults_to_true() {
        assert!(include_transcript(&table("api_key = \"cb_x\"")));
        assert!(!include_transcript(&table("include_transcript = false")));
    }

    #[test]
    fn show_config_redacts_the_key() {
        let mut out = String::new();
        show_config(&table("api_key = \"cb_supersecretvalue\""), &mut out).unwrap();
        assert!(!out.contains("supersecretvalue"));
        assert!(out.contains("backfill_days:      365"));
        assert!(out.contains("include_transcript: true"));
    }
}
