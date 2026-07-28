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

## Example agent system prompt

Void does not ship opinionated MCP prompts — workflows belong in **your** agent config so you can tweak them without a release. One common pattern is interactive Inbox Zero triage; copy and adapt the sample below into:

- a Cursor rule (e.g. `.cursor/rules/void-triage.mdc`)
- Claude Desktop / other client system prompt
- a hook `--prompt-file` (e.g. `~/.config/void/prompts/triage.md`) for scheduled or event-driven runs

This is one workflow among many (batch-by-connector, search-first, archive-first, headless auto-reply, etc.). Drop or rewrite rules that do not fit your setup — especially confirmation and “read before act” if you run unattended hooks.

```markdown
You are triaging the user's unified inbox via the Void MCP server.
Void aggregates configured connectors (WhatsApp, Telegram, Slack, Gmail,
Calendar, LinkedIn, GitHub, HN, Google News, Reddit, …) into one local inbox.

## Loop
1. **Triage** — call `inbox` for unprocessed messages. Scope with
   `connector`/`connection`, or use `search` for a topic.
2. **Understand** — call `messages` with the conversation id when you need
   thread context; use `conversations` / `contacts` / `channels` to orient.
3. **Act** — `reply`, `send`, or `forward` when a response is needed.
   Draft email or Slack react via `run` (e.g. `["gmail","draft","create",...]`
   or `["slack","react",...]`).
4. **Archive** — after handling, `archive` so the item leaves the inbox.
   Use `mute` for noisy channels you never want to see.
5. **Done** — when `inbox` returns nothing, you are at Inbox Zero.

## Rules (interactive triage)
- Read the thread before replying.
- Confirm with the user before sending external messages or deleting anything.
- For commands without a named tool, use `run` with a CLI args array
  (e.g. `["calendar","create",...]`, `["hook","list"]`).
- Reads come from a local cache kept fresh by `void sync --daemon`.

Start by calling `inbox` and summarize what needs attention.
Optional focus: prioritize anything related to <connector or topic>.
```

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

Successful commands return the same JSON envelope printed by the CLI (`{ "data", "error" }` or paginated). Non-JSON stdout is wrapped as `{ "stdout": "..." }` with optional `stderr` when present. On success, informational stderr is omitted when stdout is already a JSON envelope (agents should not treat status lines as failures).

Do **not** pass global CLI flags (`--store`, `--config`, `--verbose`/`-v`, `--no-context`, `--local-store`) inside `run` args — the server already injects `--store`/`--config`, and `no_context` is a tool parameter.

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
| `health` | `void doctor` (connectivity subset) | Per-connection health checks (**live network I/O**, unlike the DB-only read tools) |

### Write tools

| Tool | CLI equivalent | Description |
|------|----------------|-------------|
| `send` | `void send` | Send a message. Optional `signature` / `signature_from` (Gmail only) append the account HTML send-as signature |
| `reply` | `void reply` | Reply to a message. Same optional Gmail `signature` / `signature_from` |
| `forward` | `void forward` | Forward a message. Same optional Gmail `signature` / `signature_from` (signature sits between comment and quote) |
| `archive` | `void archive` | Archive by IDs or bulk `--before` |
| `mute` | `void mute` | Mute/unmute conversations |

`signature` / `signature_from` on `send` / `reply` / `forward` match the CLI `--signature` / `--signature-from` flags. They require a prior interactive grant of `gmail.settings.basic` (run one terminal command with `--signature`); the MCP server will not open a browser. Pass a body/comment without an existing signature — append is not idempotent.

In-process write tools (`send`, `reply`, `forward`, `archive`) require **local store mode**. Use **`run`** instead when on a remote client — it follows the same SSH proxy path as the CLI.

## Architecture

```
Agent ──stdio JSON-RPC──► void mcp ──┬── named tools ──► service/ ──► void.db
                                     └── run tool ──► void subprocess (full CLI)
```

Named tools and the CLI share `void-cli/src/service/` so behavior stays in lockstep.
