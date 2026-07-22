# MCP server

Void exposes a [Model Context Protocol](https://modelcontextprotocol.io/) (MCP) server so AI agents can drive the same unified inbox as the CLI — reads, writes, connector-specific commands, hooks, and more — over stdio.

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

## Prompts

The server exposes one MCP prompt (`prompts/list` + `prompts/get`) — a reusable workflow an agent or user can invoke directly.

| Prompt | Arguments | Description |
|--------|-----------|-------------|
| `triage_inbox` | `focus?` (optional connector or topic to prioritize) | Returns the Inbox Zero operating guide: triage → understand → act → archive, with guidance on which tool to use. Use it to onboard an agent onto Void without external prompting |

In Cursor or Claude Desktop, `triage_inbox` appears as a slash-command-style prompt. Calling it with `{ "focus": "slack" }` biases the guide toward Slack.

## Full CLI parity: the `run` tool

The **`run`** tool executes any void CLI subcommand by re-invoking the same binary with your `--store` / `--config` globals. This is the escape hatch for **everything** the CLI supports: `void slack saved`, `void gmail search`, `void hook list`, `void hn keywords list`, media downloads, calendar API calls, and any future command.

Example tool call:

```json
{
  "args": ["slack", "saved", "-n", "20"],
  "no_context": false
}
```

Another:

```json
{
  "args": ["gmail", "search", "from:alice newer_than:7d", "--max", "10"]
}
```

Successful commands return the same JSON envelope printed by the CLI (`{ "data", "error" }` or paginated). Non-JSON stdout is wrapped as `{ "stdout": "...", "stderr": "..." }`.

**Blocked via `run`:** `void mcp` (recursion), `void setup` (interactive wizard), `void sync --daemon` (background daemon — start from a terminal).

## Named tools (convenience)

These call the shared service layer in-process (faster, typed schemas). Prefer **`run`** when no named tool exists.

### Read tools

| Tool | CLI equivalent | Description |
|------|----------------|-------------|
| `inbox` | `void inbox` | Recent unarchived messages |
| `conversations` | `void conversations` | List conversations |
| `messages` | `void messages` | Messages in a conversation or connector |
| `search` | `void search` | FTS5 search across messages |
| `contacts` | `void contacts` | List contacts |
| `channels` | `void channels` | List channels/groups |
| `slack_saved` | `void slack saved` | Slack Later / saved-for-later messages |
| `calendar` | `void calendar` | Events from local sync cache |
| `health` | `void doctor` (connectivity subset) | Per-connection health checks |

### Write tools

| Tool | CLI equivalent | Description |
|------|----------------|-------------|
| `send` | `void send` | Send a message |
| `reply` | `void reply` | Reply to a message |
| `forward` | `void forward` | Forward a message |
| `archive` | `void archive` | Archive by IDs or bulk `--before` |
| `mute` | `void mute` | Mute/unmute conversations |

In-process write tools (`send`, `reply`, `forward`, `archive`) require **local store mode**. Use **`run`** instead when on a remote client — it follows the same SSH proxy path as the CLI.

## Architecture

```
Agent ──stdio JSON-RPC──► void mcp ──┬── prompts ──► triage_inbox guide
                                     ├── named tools ──► service/ ──► void.db
                                     └── run tool ──► void subprocess (full CLI)
```

Named tools and the CLI share `void-cli/src/service/` so behavior stays in lockstep.
