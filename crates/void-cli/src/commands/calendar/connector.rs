use void_core::config::{self, expand_tilde, VoidConfig};
use void_core::models::ConnectorType;

pub(super) fn build_calendar_connector(
    connection_filter: Option<&str>,
) -> anyhow::Result<(void_calendar::connector::CalendarConnector, VoidConfig)> {
    let config_path = config::default_config_path();
    let cfg = VoidConfig::load(&config_path)
        .map_err(|e| anyhow::anyhow!("Cannot load config: {e}\nRun `void setup` first."))?;

    let connection = cfg
        .connections
        .iter()
        .find(|a| {
            let is_calendar = a.connector_type == ConnectorType::Calendar;
            let name_matches = connection_filter.map_or(true, |n| a.id == n);
            is_calendar && name_matches
        })
        .ok_or_else(|| {
            anyhow::anyhow!("No calendar connection found in config. Run `void setup` to add one.")
        })?;

    let (credentials_file, calendar_ids) = match &connection.settings {
        void_core::config::ConnectionSettings::Calendar {
            credentials_file,
            calendar_ids,
        } => (credentials_file.clone(), calendar_ids.clone()),
        _ => anyhow::bail!(
            "Mismatched connection settings for calendar connection '{}'",
            connection.id
        ),
    };

    let cred_path = credentials_file.as_ref().map(|f| expand_tilde(f));
    let store_path = cfg.store_path();
    let connector = void_calendar::connector::CalendarConnector::new(
        &connection.id,
        cred_path.as_deref().and_then(|p| p.to_str()),
        calendar_ids,
        &store_path,
    );

    Ok((connector, cfg))
}
