use void_core::connector::Connector;
use void_core::models::ConnectorType;

use super::TelegramConnector;

#[test]
fn telegram_connector_new_sets_ids() {
    let session_path = std::env::temp_dir().join("tg.json");
    let c = TelegramConnector::new("conn-a", &session_path.to_string_lossy(), None, None);
    assert_eq!(c.connection_id(), "conn-a");
    assert_eq!(c.connector_type(), ConnectorType::Telegram);
}
