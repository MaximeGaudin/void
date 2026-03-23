/// Parameters for creating a new calendar event.
#[derive(Debug, Clone)]
pub struct CreateEventParams<'a> {
    pub title: &'a str,
    pub description: Option<&'a str>,
    pub start: &'a str,
    pub end: &'a str,
    pub meet: bool,
    pub attendees: Option<&'a str>,
}

/// Parameters for updating an existing calendar event.
#[derive(Debug, Clone)]
pub struct UpdateEventParams<'a> {
    pub event_id: &'a str,
    pub title: Option<&'a str>,
    pub description: Option<&'a str>,
    pub start: Option<&'a str>,
    pub end: Option<&'a str>,
    pub send_updates: Option<&'a str>,
}

/// Syncs Google Calendar for a single configured connection (OAuth via Gmail auth stack).
pub struct CalendarConnector {
    pub(crate) connection_id: String,
    pub(crate) credentials_file: Option<String>,
    pub(crate) calendar_ids: Vec<String>,
    pub(crate) store_path: std::path::PathBuf,
}

impl CalendarConnector {
    pub fn new(
        connection_id: &str,
        credentials_file: Option<&str>,
        calendar_ids: Vec<String>,
        store_path: &std::path::Path,
    ) -> Self {
        Self {
            connection_id: connection_id.to_string(),
            credentials_file: credentials_file.map(|s| s.to_string()),
            calendar_ids: if calendar_ids.is_empty() {
                vec!["primary".to_string()]
            } else {
                calendar_ids
            },
            store_path: store_path.to_path_buf(),
        }
    }
}
