use void_core::connector::Connector;
use void_core::models::ConnectorType;

use super::TelegramConnector;
use crate::CONNECTOR_ID;

#[test]
fn telegram_connector_new_sets_ids() {
    let session_path = std::env::temp_dir().join("tg.json");
    let c = TelegramConnector::new("conn-a", &session_path.to_string_lossy(), None, None);
    assert_eq!(c.connection_id(), "conn-a");
    assert_eq!(c.connector_type(), ConnectorType::from_static(CONNECTOR_ID));
}

#[tokio::test]
async fn health_check_missing_session_file_reports_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("nonexistent.session");
    let connector = TelegramConnector::new("test-conn", &missing.to_string_lossy(), None, None);

    let status = connector.health_check().await.unwrap();

    assert!(!status.ok);
    assert!(status.message.contains("Session file not found"));
    assert_eq!(status.connection_id, "test-conn");
    assert_eq!(
        status.connector_type,
        ConnectorType::from_static(CONNECTOR_ID)
    );
}
