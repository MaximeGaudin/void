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

pub(crate) use calendar::build_calendar;
pub(crate) use gmail::build_gmail;
pub(crate) use slack::build_slack;
pub(crate) use telegram::build_telegram;
pub(crate) use whatsapp::{build_whatsapp, build_whatsapp_owned};

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
    fn every_plugin_has_unique_aliases() {
        let mut seen = std::collections::HashSet::new();
        for p in all() {
            for alias in p.aliases {
                assert!(
                    seen.insert(*alias),
                    "duplicate connector alias: {alias} (plugin {id})",
                    id = p.id
                );
            }
        }
    }

    #[test]
    fn known_ids_csv_includes_all_plugins() {
        let csv = known_ids_csv();
        for p in all() {
            assert!(
                csv.contains(p.id),
                "known_ids_csv missing plugin id: {}",
                p.id
            );
        }
    }

    #[test]
    fn validate_all_connections_unknown_type_fails() {
        let conn = ConnectionConfig {
            id: "bad".into(),
            connector_type: ConnectorType::from_static("twitter"),
            ignore_conversations: vec![],
            settings: toml::Table::new(),
        };
        let mut cfg = VoidConfig::default();
        cfg.connections.push(conn);
        let err = validate_all_connections(&cfg).unwrap_err();
        assert!(err.to_string().contains("unknown connector type"));
    }

    #[test]
    fn validate_all_connections_slack_missing_app_token_fails() {
        let mut settings = toml::Table::new();
        settings.insert("user_token".into(), toml::Value::String("xoxp".into()));
        let conn = ConnectionConfig {
            id: "work".into(),
            connector_type: ConnectorType::from_static("slack"),
            ignore_conversations: vec![],
            settings,
        };
        let mut cfg = VoidConfig::default();
        cfg.connections.push(conn);
        let err = validate_all_connections(&cfg).unwrap_err();
        assert!(err.to_string().contains("missing app_token"));
    }

    #[test]
    fn validate_all_connections_hackernews_empty_settings_ok() {
        let conn = ConnectionConfig {
            id: "hn".into(),
            connector_type: ConnectorType::from_static("hackernews"),
            ignore_conversations: vec![],
            settings: toml::Table::new(),
        };
        let mut cfg = VoidConfig::default();
        cfg.connections.push(conn);
        validate_all_connections(&cfg).unwrap();
    }

    #[test]
    fn build_hackernews_connector_via_registry() {
        let mut settings = toml::Table::new();
        settings.insert(
            "keywords".into(),
            toml::Value::Array(vec![toml::Value::String("rust".into())]),
        );
        let conn = ConnectionConfig {
            id: "test-hn".into(),
            connector_type: ConnectorType::from_static("hackernews"),
            ignore_conversations: vec![],
            settings,
        };
        let sync = SyncConfig::default();
        let plugin = by_id("hackernews").unwrap();
        let store = tempfile::tempdir().unwrap();
        let connector = (plugin.build)(&conn, store.path(), &sync).unwrap();
        assert_eq!(connector.connector_type().as_str(), "hackernews");
        assert_eq!(connector.connection_id(), "test-hn");
    }

    #[test]
    fn connection_config_debug_redacts_slack_tokens() {
        let mut settings = toml::Table::new();
        settings.insert(
            "app_token".into(),
            toml::Value::String("xapp-1-super-secret-token".into()),
        );
        settings.insert(
            "user_token".into(),
            toml::Value::String("xoxp-super-secret-user-token".into()),
        );
        let config = ConnectionConfig {
            id: "work".into(),
            connector_type: ConnectorType::from_static("slack"),
            ignore_conversations: vec![],
            settings,
        };
        let debug = format!("{config:?}");
        assert!(!debug.contains("xapp-1-super-secret-token"));
        assert!(!debug.contains("xoxp-super-secret-user-token"));
    }

    #[test]
    fn sync_default_poll_intervals_match_plugin_defaults() {
        let sync = SyncConfig::default();
        assert_eq!(
            sync.poll_interval_secs("gmail", 30),
            by_id("gmail").unwrap().default_poll_interval_secs.unwrap()
        );
        assert_eq!(sync.poll_interval_secs("github", 120), 120);
        assert_eq!(sync.poll_interval_secs("googlenews", 3600), 3600);
    }

    #[test]
    fn connector_type_slack_toml_round_trip() {
        let toml_str = r#"
id = "work"
type = "slack"
app_token = "xapp"
user_token = "xoxp"
"#;
        let conn: ConnectionConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(conn.connector_type.as_str(), "slack");
        assert_eq!(conn.id, "work");
        let serialized = toml::to_string(&conn).unwrap();
        let reparsed: ConnectionConfig = toml::from_str(&serialized).unwrap();
        assert_eq!(reparsed.connector_type.as_str(), "slack");
    }

    #[test]
    fn badge_for_all_connectors() {
        let expected = [
            ("gmail", "GM"),
            ("slack", "SL"),
            ("whatsapp", "WA"),
            ("telegram", "TG"),
            ("calendar", "CA"),
            ("hackernews", "HN"),
            ("googlenews", "GN"),
            ("linkedin", "LI"),
            ("github", "GH"),
        ];
        for (id, badge) in expected {
            assert_eq!(
                badge_for(ConnectorType::from_static(id)),
                badge,
                "badge for {id}"
            );
        }
        assert_eq!(badge_for(ConnectorType::from_static("unknown")), "??");
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
