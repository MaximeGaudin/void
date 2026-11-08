use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;

use void_core::config::{
    redact_token, settings_str, settings_string, settings_string_list, settings_u32,
    ConnectionConfig, SyncConfig,
};
use void_core::connector::Connector;

use void_withings::{parse_streams, DEFAULT_BACKFILL_DAYS};

use super::{ConnectorPlugin, ReplyIdStyle, SetupCtx};

const DEFAULT_POLL_INTERVAL_SECS: u64 = 3600;

inventory::submit! {
    ConnectorPlugin {
        id: void_withings::CONNECTOR_ID,
        aliases: &["withings", "wi"],
        menu_label: "Withings",
        badge: "WI",
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

fn session_files(store: &Path, connection_id: &str) -> Vec<PathBuf> {
    vec![void_withings::auth::token_cache_path(store, connection_id)]
}

fn build(
    connection: &ConnectionConfig,
    store_path: &Path,
    sync: &SyncConfig,
) -> anyhow::Result<Arc<dyn Connector>> {
    let client_id = settings_string(&connection.settings, "client_id").ok_or_else(|| {
        anyhow::anyhow!(
            "missing client_id for Withings connection '{}'",
            connection.id
        )
    })?;
    let client_secret =
        settings_string(&connection.settings, "client_secret").ok_or_else(|| {
            anyhow::anyhow!(
                "missing client_secret for Withings connection '{}'",
                connection.id
            )
        })?;
    let streams = parse_streams(&settings_string_list(&connection.settings, "streams"))?;
    let backfill_days =
        settings_u32(&connection.settings, "backfill_days").unwrap_or(DEFAULT_BACKFILL_DAYS);
    let poll_secs =
        sync.poll_interval_secs(void_withings::CONNECTOR_ID, DEFAULT_POLL_INTERVAL_SECS);

    Ok(Arc::new(void_withings::connector::WithingsConnector::new(
        &connection.id,
        client_id,
        client_secret,
        store_path,
        streams,
        backfill_days,
        poll_secs,
    )))
}

fn setup(ctx: SetupCtx<'_>) -> Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + '_>> {
    Box::pin(crate::commands::setup::withings::setup_withings(
        ctx.cfg,
        ctx.store_path,
        ctx.add_only,
    ))
}

fn parse_settings(table: &toml::Table) -> anyhow::Result<()> {
    match settings_str(table, "client_id") {
        None => anyhow::bail!("missing client_id"),
        Some(v) if v.trim().is_empty() => anyhow::bail!("client_id is empty"),
        Some(_) => {}
    }
    match settings_str(table, "client_secret") {
        None => anyhow::bail!("missing client_secret"),
        Some(v) if v.trim().is_empty() => anyhow::bail!("client_secret is empty"),
        Some(_) => {}
    }
    parse_streams(&settings_string_list(table, "streams"))?;
    if let Some(v) = table.get("backfill_days") {
        if !v.is_integer() || v.as_integer().is_some_and(|n| n < 0) {
            anyhow::bail!("backfill_days must be a non-negative integer");
        }
    }
    Ok(())
}

fn show_config(table: &toml::Table, out: &mut dyn std::fmt::Write) -> std::fmt::Result {
    if let Some(client_id) = settings_str(table, "client_id") {
        writeln!(out, "    client_id:     {}", redact_token(client_id))?;
    }
    if let Some(client_secret) = settings_str(table, "client_secret") {
        writeln!(out, "    client_secret: {}", redact_token(client_secret))?;
    }
    let streams = parse_streams(&settings_string_list(table, "streams")).unwrap_or_default();
    writeln!(
        out,
        "    streams:       {}",
        streams
            .iter()
            .map(|s| s.id())
            .collect::<Vec<_>>()
            .join(", ")
    )?;
    writeln!(
        out,
        "    backfill_days: {}",
        settings_u32(table, "backfill_days").unwrap_or(DEFAULT_BACKFILL_DAYS)
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(toml_src: &str) -> toml::Table {
        toml::from_str(toml_src).unwrap()
    }

    const CREDS: &str = "client_id = \"cid\"\nclient_secret = \"sec\"\n";

    #[test]
    fn parse_settings_requires_both_credentials() {
        assert!(parse_settings(&table("")).is_err());
        assert!(parse_settings(&table("client_id = \"cid\"")).is_err());
        assert!(parse_settings(&table("client_id = \"\"\nclient_secret = \"s\"")).is_err());
        assert!(parse_settings(&table(CREDS)).is_ok());
    }

    #[test]
    fn parse_settings_rejects_an_unknown_stream() {
        let err = parse_settings(&table(&format!("{CREDS}streams = [\"vo2\"]"))).unwrap_err();
        assert!(err.to_string().contains("vo2"));
        assert!(parse_settings(&table(&format!("{CREDS}streams = [\"sleep\"]"))).is_ok());
    }

    #[test]
    fn parse_settings_rejects_a_negative_backfill() {
        assert!(parse_settings(&table(&format!("{CREDS}backfill_days = -1"))).is_err());
        assert!(parse_settings(&table(&format!("{CREDS}backfill_days = 30"))).is_ok());
    }

    #[test]
    fn show_config_redacts_credentials_and_defaults_to_every_stream() {
        let mut out = String::new();
        show_config(
            &table("client_id = \"cid-supersecret\"\nclient_secret = \"sec-supersecret\""),
            &mut out,
        )
        .unwrap();
        assert!(!out.contains("supersecret"));
        assert!(out.contains("streams:       measures, activity, sleep"));
        assert!(out.contains("backfill_days: 365"));
    }

    #[test]
    fn session_files_point_at_the_token_cache() {
        let files = session_files(Path::new("/store"), "health");
        assert_eq!(files.len(), 1);
        assert!(files[0].ends_with("health-withings-token.json"));
    }
}
