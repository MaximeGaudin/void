use schemars::JsonSchema;
use serde::Deserialize;

use rmcp::transport::stdio;
use rmcp::{
    handler::server::wrapper::Parameters,
    model::{CallToolResult, Content, ServerInfo},
    tool, tool_handler, tool_router, ErrorData as McpError, ServerHandler, ServiceExt,
};

use crate::service::health;
use crate::service::reads::{
    self, CalendarQuery, ChannelsQuery, ContactsQuery, InboxQuery, MessagesQuery, SearchQuery,
};

#[derive(Clone)]
pub struct VoidMcpServer {
    #[allow(dead_code)]
    tool_router: rmcp::handler::server::tool::ToolRouter<Self>,
}

impl VoidMcpServer {
    pub fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema, Default)]
struct HealthToolParams {}

#[derive(Debug, Deserialize, JsonSchema)]
struct InboxToolParams {
    /// Filter by connection (partial match on connection_id)
    connection: Option<String>,
    /// Filter by connector (slack, gmail, whatsapp, etc.)
    connector: Option<String>,
    #[serde(default = "default_size")]
    size: i64,
    #[serde(default = "default_page")]
    page: i64,
    #[serde(default)]
    all: bool,
    #[serde(default)]
    include_muted: bool,
    #[serde(default = "default_true")]
    enrich_context: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ConversationsToolParams {
    connection: Option<String>,
    connector: Option<String>,
    #[serde(default = "default_size")]
    size: i64,
    #[serde(default = "default_page")]
    page: i64,
    #[serde(default)]
    include_muted: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct MessagesToolParams {
    /// Conversation ID, connector name, or Slack message link
    target: String,
    since: Option<String>,
    until: Option<String>,
    #[serde(default = "default_messages_size")]
    size: i64,
    #[serde(default = "default_page")]
    page: i64,
    #[serde(default = "default_true")]
    enrich_context: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct SearchToolParams {
    query: String,
    connection: Option<String>,
    connector: Option<String>,
    #[serde(default = "default_size")]
    size: i64,
    #[serde(default = "default_page")]
    page: i64,
    #[serde(default)]
    include_muted: bool,
    #[serde(default = "default_true")]
    enrich_context: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ContactsToolParams {
    search: Option<String>,
    connection: Option<String>,
    connector: Option<String>,
    #[serde(default = "default_contacts_size")]
    size: i64,
    #[serde(default = "default_page")]
    page: i64,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ChannelsToolParams {
    search: Option<String>,
    connection: Option<String>,
    connector: Option<String>,
    #[serde(default = "default_contacts_size")]
    size: i64,
    #[serde(default = "default_page")]
    page: i64,
    #[serde(default)]
    include_muted: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct CalendarToolParams {
    /// Show this week's events instead of the default day/range filter
    #[serde(default)]
    week: bool,
    day: Option<String>,
    from: Option<String>,
    to: Option<String>,
    connection: Option<String>,
    connector: Option<String>,
}

fn default_size() -> i64 {
    50
}

fn default_messages_size() -> i64 {
    100
}

fn default_contacts_size() -> i64 {
    100
}

fn default_page() -> i64 {
    1
}

fn default_true() -> bool {
    true
}

fn json_result(value: serde_json::Value) -> CallToolResult {
    let text = serde_json::to_string_pretty(&value).unwrap_or_else(|e| {
        format!("{{\"data\": null, \"error\": \"failed to serialize response: {e}\"}}")
    });
    CallToolResult::success(vec![Content::text(text)])
}

fn tool_ok<T: serde::Serialize>(data: T) -> CallToolResult {
    json_result(serde_json::json!({ "data": data, "error": serde_json::Value::Null }))
}

fn tool_err(err: impl std::fmt::Display) -> CallToolResult {
    json_result(serde_json::json!({ "data": serde_json::Value::Null, "error": err.to_string() }))
}

fn service_result(result: anyhow::Result<serde_json::Value>) -> CallToolResult {
    match result {
        Ok(value) => json_result(value),
        Err(e) => tool_err(e),
    }
}

#[tool_router]
impl VoidMcpServer {
    #[tool(description = "Show recent messages across all connectors (void inbox)")]
    async fn inbox(&self, params: Parameters<InboxToolParams>) -> Result<CallToolResult, McpError> {
        let db = match crate::context::open_db() {
            Ok(db) => db,
            Err(e) => return Ok(tool_err(e)),
        };
        let p = params.0;
        Ok(service_result(reads::inbox(
            &db,
            &InboxQuery {
                connection: p.connection.as_deref(),
                connector: p.connector.as_deref(),
                size: p.size,
                page: p.page,
                all: p.all,
                include_muted: p.include_muted,
            },
            p.enrich_context,
        )))
    }

    #[tool(description = "List conversations across all connectors")]
    async fn conversations(
        &self,
        params: Parameters<ConversationsToolParams>,
    ) -> Result<CallToolResult, McpError> {
        let db = match crate::context::open_db() {
            Ok(db) => db,
            Err(e) => return Ok(tool_err(e)),
        };
        let p = params.0;
        Ok(service_result(reads::conversations(
            &db,
            &InboxQuery {
                connection: p.connection.as_deref(),
                connector: p.connector.as_deref(),
                size: p.size,
                page: p.page,
                all: false,
                include_muted: p.include_muted,
            },
        )))
    }

    #[tool(description = "Show messages in a conversation or for a connector")]
    async fn messages(
        &self,
        params: Parameters<MessagesToolParams>,
    ) -> Result<CallToolResult, McpError> {
        let db = match crate::context::open_db() {
            Ok(db) => db,
            Err(e) => return Ok(tool_err(e)),
        };
        let p = params.0;
        Ok(service_result(reads::messages(
            &db,
            &MessagesQuery {
                target: &p.target,
                since: p.since.as_deref(),
                until: p.until.as_deref(),
                size: p.size,
                page: p.page,
            },
            p.enrich_context,
        )))
    }

    #[tool(description = "Full-text search across synced messages")]
    async fn search(
        &self,
        params: Parameters<SearchToolParams>,
    ) -> Result<CallToolResult, McpError> {
        let db = match crate::context::open_db() {
            Ok(db) => db,
            Err(e) => return Ok(tool_err(e)),
        };
        let p = params.0;
        Ok(service_result(reads::search(
            &db,
            &SearchQuery {
                query: &p.query,
                connection: p.connection.as_deref(),
                connector: p.connector.as_deref(),
                size: p.size,
                page: p.page,
                include_muted: p.include_muted,
            },
            p.enrich_context,
        )))
    }

    #[tool(description = "List contacts across all connectors")]
    async fn contacts(
        &self,
        params: Parameters<ContactsToolParams>,
    ) -> Result<CallToolResult, McpError> {
        let db = match crate::context::open_db() {
            Ok(db) => db,
            Err(e) => return Ok(tool_err(e)),
        };
        let p = params.0;
        Ok(service_result(reads::contacts(
            &db,
            &ContactsQuery {
                search: p.search.as_deref(),
                connection: p.connection.as_deref(),
                connector: p.connector.as_deref(),
                size: p.size,
                page: p.page,
            },
        )))
    }

    #[tool(description = "List channels and groups (excluding DMs)")]
    async fn channels(
        &self,
        params: Parameters<ChannelsToolParams>,
    ) -> Result<CallToolResult, McpError> {
        let db = match crate::context::open_db() {
            Ok(db) => db,
            Err(e) => return Ok(tool_err(e)),
        };
        let p = params.0;
        Ok(service_result(reads::channels(
            &db,
            &ChannelsQuery {
                search: p.search.as_deref(),
                connection: p.connection.as_deref(),
                connector: p.connector.as_deref(),
                size: p.size,
                page: p.page,
                include_muted: p.include_muted,
            },
        )))
    }

    #[tool(description = "List calendar events from the local sync cache")]
    async fn calendar(
        &self,
        params: Parameters<CalendarToolParams>,
    ) -> Result<CallToolResult, McpError> {
        let db = match crate::context::open_db() {
            Ok(db) => db,
            Err(e) => return Ok(tool_err(e)),
        };
        let p = params.0;
        let result = if p.week {
            reads::calendar_week(&db)
        } else {
            reads::calendar_list(
                &db,
                &CalendarQuery {
                    day: p.day.as_deref(),
                    from: p.from.as_deref(),
                    to: p.to.as_deref(),
                    connection: p.connection.as_deref(),
                    connector: p.connector.as_deref(),
                },
            )
        };
        Ok(service_result(result))
    }

    #[tool(description = "Check connectivity health for all configured connections")]
    async fn health(
        &self,
        _params: Parameters<HealthToolParams>,
    ) -> Result<CallToolResult, McpError> {
        let cfg = crate::context::void_config();
        let store_path = crate::context::store_path();
        let statuses = health::check_connections(cfg, &store_path).await;
        Ok(tool_ok(statuses))
    }
}

#[tool_handler]
impl ServerHandler for VoidMcpServer {
    fn get_info(&self) -> ServerInfo {
        // Prefer the binary/product name over CARGO_CRATE_NAME ("void-cli").
        ServerInfo::default().with_server_info(rmcp::model::Implementation::new(
            "void",
            env!("CARGO_PKG_VERSION"),
        ))
    }
}

pub async fn run_server() -> anyhow::Result<()> {
    let server = VoidMcpServer::new();
    let service = server.serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn void_mcp_server_tool_router_has_read_tools() {
        let server = VoidMcpServer::new();
        let tools = server.tool_router.list_all();
        let names: Vec<_> = tools.iter().map(|t| t.name.as_ref()).collect();
        for expected in [
            "inbox",
            "conversations",
            "messages",
            "search",
            "contacts",
            "channels",
            "calendar",
            "health",
        ] {
            assert!(
                names.contains(&expected),
                "missing tool {expected}, got {names:?}"
            );
        }
    }

    #[test]
    fn server_info_uses_product_name_void() {
        let info = VoidMcpServer::new().get_info();
        assert_eq!(info.server_info.name, "void");
        assert_eq!(info.server_info.version, env!("CARGO_PKG_VERSION"));
    }
}
