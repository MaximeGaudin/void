use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;

use void_core::config::{settings_string, settings_string_list, ConnectionConfig, SyncConfig};
use void_core::connector::Connector;

use super::{ConnectorPlugin, ReplyIdStyle, SetupCtx};

fn default_gn_language() -> String {
    "fr".to_string()
}

fn default_gn_country() -> String {
    "FR".to_string()
}

const DEFAULT_POLL_INTERVAL_SECS: u64 = 3600;

inventory::submit! {
    ConnectorPlugin {
        id: void_googlenews::CONNECTOR_ID,
        aliases: &["googlenews", "gn"],
        menu_label: "Google News",
        badge: "GN",
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

fn build(
    connection: &ConnectionConfig,
    _store_path: &Path,
    sync: &SyncConfig,
) -> anyhow::Result<Arc<dyn Connector>> {
    let keywords = settings_string_list(&connection.settings, "keywords");
    let when = settings_string(&connection.settings, "when").unwrap_or_default();
    let language =
        settings_string(&connection.settings, "language").unwrap_or_else(default_gn_language);
    let country =
        settings_string(&connection.settings, "country").unwrap_or_else(default_gn_country);
    let poll_secs =
        sync.poll_interval_secs(void_googlenews::CONNECTOR_ID, DEFAULT_POLL_INTERVAL_SECS);
    Ok(Arc::new(
        void_googlenews::connector::GoogleNewsConnector::new(
            &connection.id,
            keywords,
            &when,
            &language,
            &country,
            poll_secs,
        ),
    ))
}

fn setup(ctx: SetupCtx<'_>) -> Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + '_>> {
    Box::pin(async move {
        crate::commands::setup::googlenews::setup_googlenews(ctx.cfg, ctx.add_only)?;
        Ok(())
    })
}

fn parse_settings(_table: &toml::Table) -> anyhow::Result<()> {
    Ok(())
}

fn show_config(table: &toml::Table, out: &mut dyn std::fmt::Write) -> std::fmt::Result {
    let keywords = settings_string_list(table, "keywords");
    if keywords.is_empty() {
        writeln!(out, "    keywords:  (none)")?;
    } else {
        writeln!(out, "    keywords:  {}", keywords.join(", "))?;
    }
    let when = settings_string(table, "when").unwrap_or_default();
    if when.is_empty() {
        writeln!(out, "    when:      (no limit)")?;
    } else {
        writeln!(out, "    when:      {when}")?;
    }
    let language = settings_string(table, "language").unwrap_or_else(default_gn_language);
    let country = settings_string(table, "country").unwrap_or_else(default_gn_country);
    writeln!(out, "    language:  {language}")?;
    writeln!(out, "    country:   {country}")?;
    Ok(())
}
