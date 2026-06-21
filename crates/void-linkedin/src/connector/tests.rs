use void_core::connector::Connector;
use void_core::models::ConnectorType;

use super::LinkedInConnector;
use crate::CONNECTOR_ID;

#[test]
fn linkedin_connector_new_sets_ids() {
    let c = LinkedInConnector::new("linkedin", "key", "api.example.com:443", "acc-1", 1800, 15);
    assert_eq!(c.connection_id(), "linkedin");
    assert_eq!(c.connector_type(), ConnectorType::from_static(CONNECTOR_ID));
}
