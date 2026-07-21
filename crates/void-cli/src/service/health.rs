use std::path::Path;

use void_core::config::VoidConfig;

use serde::Serialize;

use crate::commands::connector_factory;

#[derive(Debug, Serialize)]
pub struct ConnectionHealth {
    pub connection_id: String,
    pub connector_type: String,
    pub ok: bool,
    pub message: String,
}

pub async fn check_connections(cfg: &VoidConfig, store_path: &Path) -> Vec<ConnectionHealth> {
    let mut results = Vec::new();

    for conn_config in &cfg.connections {
        let entry = match connector_factory::build_connector(conn_config, store_path) {
            Ok(connector) => match connector.health_check().await {
                Ok(status) => ConnectionHealth {
                    connection_id: conn_config.id.clone(),
                    connector_type: conn_config.connector_type.to_string(),
                    ok: status.ok,
                    message: status.message,
                },
                Err(e) => ConnectionHealth {
                    connection_id: conn_config.id.clone(),
                    connector_type: conn_config.connector_type.to_string(),
                    ok: false,
                    message: e.to_string(),
                },
            },
            Err(e) => ConnectionHealth {
                connection_id: conn_config.id.clone(),
                connector_type: conn_config.connector_type.to_string(),
                ok: false,
                message: format!("ERROR BUILDING: {e}"),
            },
        };

        results.push(entry);
    }

    results
}
