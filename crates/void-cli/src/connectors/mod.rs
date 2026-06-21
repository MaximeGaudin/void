//! Compile-time connector plugin registry (`inventory`).

mod calendar;
mod github;
mod gmail;
mod googlenews;
mod hackernews;
mod linkedin;
mod slack;
mod telegram;
mod whatsapp;

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;

use void_core::config::{ConnectionConfig, SyncConfig, VoidConfig};
use void_core::connector::Connector;
use void_core::models::ConnectorType;

#[derive(Clone, Copy)]
pub enum ReplyIdStyle {
    ConvMsg,
    MsgOnly,
}

pub struct SetupCtx<'a> {
    pub cfg: &'a mut VoidConfig,
    pub store_path: &'a Path,
    pub add_only: bool,
}

pub type SetupFn = fn(SetupCtx<'_>) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + '_>>;

pub struct ConnectorPlugin {
    pub id: &'static str,
    pub aliases: &'static [&'static str],
    pub menu_label: &'static str,
    pub badge: &'static str,
    pub default_poll_interval_secs: Option<u64>,
    pub reply_id_style: ReplyIdStyle,
    pub supports_scheduling: bool,
    pub uses_daemon_rpc: bool,
    pub prompt_token_reauth: bool,
    pub session_files: fn(store: &Path, connection_id: &str) -> Vec<PathBuf>,
    pub build: fn(&ConnectionConfig, &Path, &SyncConfig) -> anyhow::Result<Arc<dyn Connector>>,
    pub setup: SetupFn,
    pub parse_settings: fn(&toml::Table) -> anyhow::Result<()>,
    pub show_config: fn(&toml::Table, &mut dyn std::fmt::Write) -> std::fmt::Result,
}

inventory::collect!(ConnectorPlugin);

pub fn all() -> Vec<&'static ConnectorPlugin> {
    inventory::iter::<ConnectorPlugin>().collect()
}

pub fn by_id(id: &str) -> Option<&'static ConnectorPlugin> {
    inventory::iter::<ConnectorPlugin>().find(|p| p.id == id)
}

pub fn by_alias_or_id(s: &str) -> Option<&'static ConnectorPlugin> {
    let lower = s.to_lowercase();
    inventory::iter::<ConnectorPlugin>()
        .find(|p| p.id == lower || p.aliases.iter().any(|a| *a == lower))
}

pub fn connector_type_from_alias(s: &str) -> Option<ConnectorType> {
    by_alias_or_id(s).map(|p| ConnectorType::from_static(p.id))
}

pub fn known_ids_csv() -> String {
    let mut ids: Vec<&str> = inventory::iter::<ConnectorPlugin>().map(|p| p.id).collect();
    ids.sort();
    ids.join(", ")
}

pub fn badge_for(connector_type: ConnectorType) -> &'static str {
    by_id(connector_type.as_str())
        .map(|p| p.badge)
        .unwrap_or("??")
}

pub fn build_reply_id(
    connector_type: ConnectorType,
    conv_external_id: &str,
    msg_external_id: &str,
) -> String {
    let plugin = by_id(connector_type.as_str());
    match plugin.map(|p| p.reply_id_style) {
        Some(ReplyIdStyle::ConvMsg) => format!("{conv_external_id}:{msg_external_id}"),
        Some(ReplyIdStyle::MsgOnly) | None => msg_external_id.to_string(),
    }
}

pub fn validate_connection_settings(conn: &ConnectionConfig) -> anyhow::Result<()> {
    let plugin = by_id(conn.connector_type.as_str())
        .ok_or_else(|| anyhow::anyhow!("unknown connector type: {}", conn.connector_type))?;
    (plugin.parse_settings)(&conn.settings)?;
    Ok(())
}

pub fn validate_all_connections(cfg: &VoidConfig) -> anyhow::Result<()> {
    for conn in &cfg.connections {
        validate_connection_settings(conn)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_plugin_has_unique_id() {
        let plugins = all();
        assert!(plugins.len() >= 9);
        let mut ids = std::collections::HashSet::new();
        for p in &plugins {
            assert!(ids.insert(p.id), "duplicate connector id: {}", p.id);
        }
    }

    #[test]
    fn every_plugin_has_unique_badge() {
        let plugins = all();
        let mut badges = std::collections::HashSet::new();
        for p in plugins {
            assert!(badges.insert(p.badge), "duplicate badge: {}", p.badge);
        }
    }

    #[test]
    fn by_alias_or_id_resolves_all_aliases() {
        for p in all() {
            assert_eq!(by_alias_or_id(p.id).map(|x| x.id), Some(p.id));
            for alias in p.aliases {
                assert_eq!(
                    by_alias_or_id(alias).map(|x| x.id),
                    Some(p.id),
                    "alias {alias} for {id}",
                    id = p.id
                );
            }
        }
    }

    #[test]
    fn badge_for_known_connectors() {
        assert_eq!(badge_for(ConnectorType::from_static("slack")), "SL");
        assert_eq!(badge_for(ConnectorType::from_static("github")), "GH");
    }

    #[test]
    fn slack_parse_settings_requires_tokens() {
        let plugin = by_id("slack").unwrap();
        assert!((plugin.parse_settings)(&toml::Table::new()).is_err());
        let mut table = toml::Table::new();
        table.insert("app_token".into(), toml::Value::String("xapp".into()));
        table.insert("user_token".into(), toml::Value::String("xoxp".into()));
        assert!((plugin.parse_settings)(&table).is_ok());
    }
}
