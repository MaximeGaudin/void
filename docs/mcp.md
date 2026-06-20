# MCP server

Void exposes a [Model Context Protocol](https://modelcontextprotocol.io/) (MCP) server so AI agents can read your unified inbox and check connector health over stdio — the same data the CLI uses, via the shared `void-cli/src/service` layer.

## Running

The MCP server uses **stdio** transport. Your agent (Cursor, Claude Desktop, a custom harness) spawns:

```bash
void mcp
```

Optional globals work as for any command:

```bash
void --store /path/to/store --config /path/to/config.toml mcp
```

**Important:** stdout is reserved for the JSON-RPC stream. Do not wrap `void mcp` in scripts that print to stdout.

Ensure `void sync --daemon` is running so the local SQLite cache stays current.

## Client configuration

### Cursor (`~/.cursor/mcp.json`)

```json
{
  "mcpServers": {
    "void": {
      "command": "void",
      "args": ["mcp"]
    }
  }
}
```

Use the full path to the `void` binary if it is not on the agent's `PATH`.

## Read tools (v1)

All tools return the same JSON envelope as the CLI: `{ "data": …, "error": null }` or paginated `{ "data", "pagination", "error" }`.

| Tool | CLI equivalent | Description |
|------|----------------|-------------|
| `inbox` | `void inbox` | Recent unarchived messages |
| `conversations` | `void conversations` | List conversations |
| `messages` | `void messages` | Messages in a conversation or connector |
| `search` | `void search` | FTS5 search across messages |
| `contacts` | `void contacts` | List contacts |
| `channels` | `void channels` | List channels/groups |
| `calendar` | `void calendar` | Events from local sync cache |
| `health` | `void doctor` (non-interactive subset) | Per-connection health checks |

Tool parameters mirror CLI flags (e.g. `connection`, `connector`, `size`, `page`). See each tool's JSON Schema via `list_tools`.

## Limitations

- **Local store only** in v1 — remote SSH proxy mode is not supported for MCP tool calls yet; run `void mcp` on the machine that holds the store.
- **Write tools** (`send`, `reply`, `forward`, `archive`, `mute`) are added in a follow-up release.
- Connector-specific operations (Gmail drafts, Slack react, media download) remain CLI-only for now.

## Architecture

```
Agent  ──stdio JSON-RPC──►  void mcp  ──►  service/reads  ──►  void.db (SQLite + FTS5)
```

The service layer is shared with the CLI so MCP and terminal commands stay in lockstep.
