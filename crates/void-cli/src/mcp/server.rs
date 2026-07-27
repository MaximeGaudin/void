use schemars::JsonSchema;
use serde::Deserialize;

use rmcp::transport::stdio;
use rmcp::{
    handler::server::wrapper::Parameters,
    model::{CallToolResult, Content, ServerInfo},
    tool, tool_handler, tool_router, ErrorData as McpError, ServerHandler, ServiceExt,
};

use crate::service::exec::{self, ExecParams};
use crate::service::health;
use crate::service::reads::{
    self, CalendarQuery, ChannelsQuery, ContactsQuery, ConversationsQuery, InboxQuery,
    MessagesQuery, SearchQuery, SlackSavedQuery,
};
use crate::service::writes::{
    self, ArchiveParams, ForwardParams, MuteParams, ReplyParams, SendParams,
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
struct SendToolParams {
    via: String,
    connection: Option<String>,
    to: Option<String>,
    conversation: Option<String>,
    message: String,
    subject: Option<String>,
    #[serde(default)]
    signature: bool,
    signature_from: Option<String>,
    cc: Option<String>,
    bcc: Option<String>,
    file: Option<String>,
    at: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ReplyToolParams {
    message_id: String,
    message: String,
    file: Option<String>,
    #[serde(default)]
    in_thread: bool,
    #[serde(default)]
    signature: bool,
    signature_from: Option<String>,
    cc: Option<String>,
    bcc: Option<String>,
    at: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ForwardToolParams {
    message_id: String,
    to: String,
    comment: Option<String>,
    #[serde(default)]
    signature: bool,
    signature_from: Option<String>,
    cc: Option<String>,
    bcc: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ArchiveToolParams {
    #[serde(default)]
    message_ids: Vec<String>,
    before: Option<String>,
    connector: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct MuteToolParams {
    targets: Vec<String>,
    #[serde(default)]
    unmute: bool,
    connection: Option<String>,
    connector: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct SlackSavedToolParams {
    connection: Option<String>,
    #[serde(default = "default_size")]
    size: i64,
    #[serde(default = "default_page")]
    page: i64,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct RunToolParams {
    /// void CLI arguments after the binary name (e.g. ["slack", "saved", "-n", "10"])
    args: Vec<String>,
    #[serde(default)]
    no_context: bool,
}

fn require_local_store_for_writes() -> Option<CallToolResult> {
    if crate::context::is_remote() {
        Some(tool_err(
            "MCP write tools require local store mode; run `void mcp` on the machine that hosts the sync daemon",
        ))
    } else {
        None
    }
}

fn exec_to_tool_result(result: exec::ExecResult) -> CallToolResult {
    if result.exit_code != 0 {
        let mut msg = result.stdout.trim().to_string();
        if !result.stderr.trim().is_empty() {
            if !msg.is_empty() {
                msg.push('\n');
            }
            msg.push_str(result.stderr.trim());
        }
        if msg.is_empty() {
            msg = format!("void exited with code {}", result.exit_code);
        }
        return tool_err(msg);
    }

    let stdout = result.stdout.trim();
    if stdout.is_empty() {
        // Success with no stdout — omit informational stderr so agents don't
        // treat status lines as failures.
        return tool_ok(serde_json::json!({ "stdout": "" }));
    }

    if let Ok(val) = serde_json::from_str::<serde_json::Value>(stdout) {
        // CLI JSON envelopes already carry errors in-band; drop stderr noise.
        return json_result(val);
    }

    let mut wrap = serde_json::json!({ "stdout": stdout });
    let stderr = result.stderr.trim();
    if !stderr.is_empty() {
        wrap["stderr"] = serde_json::Value::String(stderr.to_string());
    }
    tool_ok(wrap)
}

fn open_db() -> Result<void_core::db::Database, CallToolResult> {
    crate::context::open_db().map_err(tool_err)
}

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
        let db = match open_db() {
            Ok(db) => db,
            Err(err) => return Ok(err),
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
        let db = match open_db() {
            Ok(db) => db,
            Err(err) => return Ok(err),
        };
        let p = params.0;
        Ok(service_result(reads::conversations(
            &db,
            &ConversationsQuery {
                connection: p.connection.as_deref(),
                connector: p.connector.as_deref(),
                size: p.size,
                page: p.page,
                include_muted: p.include_muted,
            },
        )))
    }

    #[tool(description = "Show messages in a conversation or for a connector")]
    async fn messages(
        &self,
        params: Parameters<MessagesToolParams>,
    ) -> Result<CallToolResult, McpError> {
        let db = match open_db() {
            Ok(db) => db,
            Err(err) => return Ok(err),
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
        let db = match open_db() {
            Ok(db) => db,
            Err(err) => return Ok(err),
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
        let db = match open_db() {
            Ok(db) => db,
            Err(err) => return Ok(err),
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
        let db = match open_db() {
            Ok(db) => db,
            Err(err) => return Ok(err),
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
        let db = match open_db() {
            Ok(db) => db,
            Err(err) => return Ok(err),
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

    #[tool(description = "List Slack messages saved for later (void slack saved)")]
    async fn slack_saved(
        &self,
        params: Parameters<SlackSavedToolParams>,
    ) -> Result<CallToolResult, McpError> {
        let db = match open_db() {
            Ok(db) => db,
            Err(err) => return Ok(err),
        };
        let p = params.0;
        Ok(service_result(reads::slack_saved(
            &db,
            &SlackSavedQuery {
                connection: p.connection.as_deref(),
                size: p.size,
                page: p.page,
            },
        )))
    }

    #[tool(
        description = "Run any void CLI subcommand with full parity (e.g. args=[\"slack\",\"saved\"], [\"gmail\",\"search\",\"from:alice\"], [\"hook\",\"list\"]). Blocks interactive setup and sync --daemon."
    )]
    async fn run(&self, params: Parameters<RunToolParams>) -> Result<CallToolResult, McpError> {
        let p = params.0;
        if p.args.is_empty() {
            return Ok(tool_err("args must include a void subcommand"));
        }
        let store = crate::context::store_path();
        let config = crate::context::client_config_path();
        match exec::run_subcommand(&ExecParams {
            args: &p.args,
            store: Some(store.as_path()),
            config: Some(config.as_path()),
            no_context: p.no_context,
        }) {
            Ok(result) => Ok(exec_to_tool_result(result)),
            Err(e) => Ok(tool_err(e)),
        }
    }

    #[tool(description = "Send a new message via a connector")]
    async fn send(&self, params: Parameters<SendToolParams>) -> Result<CallToolResult, McpError> {
        if let Some(err) = require_local_store_for_writes() {
            return Ok(err);
        }
        let cfg = crate::context::void_config();
        let db = match open_db() {
            Ok(db) => db,
            Err(err) => return Ok(err),
        };
        let store_path = crate::context::store_path();
        let p = params.0;
        match writes::send(
            &db,
            cfg,
            &store_path,
            SendParams {
                via: &p.via,
                connection: p.connection.as_deref(),
                to: p.to.as_deref(),
                conversation: p.conversation.as_deref(),
                message: &p.message,
                subject: p.subject.as_deref(),
                signature: p.signature,
                signature_from: p.signature_from.as_deref(),
                cc: p.cc.as_deref(),
                bcc: p.bcc.as_deref(),
                file: p.file.as_deref(),
                at: p.at.as_deref(),
            },
        )
        .await
        {
            Ok(result) => {
                let mut payload = serde_json::json!({ "message_id": result.id });
                if let Some(at) = result.scheduled_at {
                    payload["scheduled_at"] = serde_json::json!(at);
                }
                Ok(tool_ok(payload))
            }
            Err(e) => Ok(tool_err(e)),
        }
    }

    #[tool(description = "Reply to a message")]
    async fn reply(&self, params: Parameters<ReplyToolParams>) -> Result<CallToolResult, McpError> {
        if let Some(err) = require_local_store_for_writes() {
            return Ok(err);
        }
        let cfg = crate::context::void_config();
        let db = match open_db() {
            Ok(db) => db,
            Err(err) => return Ok(err),
        };
        let store_path = crate::context::store_path();
        let p = params.0;
        match writes::reply(
            &db,
            cfg,
            &store_path,
            ReplyParams {
                message_id: &p.message_id,
                message: &p.message,
                file: p.file.as_deref(),
                in_thread: p.in_thread,
                signature: p.signature,
                signature_from: p.signature_from.as_deref(),
                cc: p.cc.as_deref(),
                bcc: p.bcc.as_deref(),
                at: p.at.as_deref(),
            },
        )
        .await
        {
            Ok(result) => {
                let mut payload = serde_json::json!({ "message_id": result.id });
                if let Some(at) = result.scheduled_at {
                    payload["scheduled_at"] = serde_json::json!(at);
                }
                Ok(tool_ok(payload))
            }
            Err(e) => Ok(tool_err(e)),
        }
    }

    #[tool(description = "Forward a message to another recipient")]
    async fn forward(
        &self,
        params: Parameters<ForwardToolParams>,
    ) -> Result<CallToolResult, McpError> {
        if let Some(err) = require_local_store_for_writes() {
            return Ok(err);
        }
        let cfg = crate::context::void_config();
        let db = match open_db() {
            Ok(db) => db,
            Err(err) => return Ok(err),
        };
        let store_path = crate::context::store_path();
        let p = params.0;
        match writes::forward(
            &db,
            cfg,
            &store_path,
            ForwardParams {
                message_id: &p.message_id,
                to: &p.to,
                comment: p.comment.as_deref(),
                signature: p.signature,
                signature_from: p.signature_from.as_deref(),
                cc: p.cc.as_deref(),
                bcc: p.bcc.as_deref(),
            },
        )
        .await
        {
            Ok(message_id) => Ok(tool_ok(serde_json::json!({ "message_id": message_id }))),
            Err(e) => Ok(tool_err(e)),
        }
    }

    #[tool(description = "Archive one or more messages, or bulk-archive before a date")]
    async fn archive(
        &self,
        params: Parameters<ArchiveToolParams>,
    ) -> Result<CallToolResult, McpError> {
        if let Some(err) = require_local_store_for_writes() {
            return Ok(err);
        }
        let cfg = crate::context::void_config();
        let db = match open_db() {
            Ok(db) => db,
            Err(err) => return Ok(err),
        };
        let store_path = crate::context::store_path();
        let p = params.0;
        Ok(service_result(
            writes::archive(
                &db,
                cfg,
                &store_path,
                ArchiveParams {
                    message_ids: &p.message_ids,
                    before: p.before.as_deref(),
                    connector: p.connector.as_deref(),
                },
            )
            .await,
        ))
    }

    #[tool(description = "Mute or unmute conversations/channels")]
    async fn mute(&self, params: Parameters<MuteToolParams>) -> Result<CallToolResult, McpError> {
        let p = params.0;
        if p.targets.is_empty() {
            return Ok(tool_err("at least one target is required"));
        }
        let config_path = crate::context::client_config_path();
        let mut cfg = match void_core::config::VoidConfig::load(&config_path) {
            Ok(c) => c,
            Err(e) => return Ok(tool_err(format!("Cannot load config: {e}"))),
        };
        let db = match open_db() {
            Ok(db) => db,
            Err(err) => return Ok(err),
        };
        Ok(service_result(writes::mute(
            &db,
            &mut cfg,
            &config_path,
            MuteParams {
                targets: &p.targets,
                unmute: p.unmute,
                connection: p.connection.as_deref(),
                connector: p.connector.as_deref(),
            },
        )))
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
    fn void_mcp_server_tool_router_has_catalog() {
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
            "slack_saved",
            "run",
            "send",
            "reply",
            "forward",
            "archive",
            "mute",
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
