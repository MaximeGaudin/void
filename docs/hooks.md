# Hooks — LLM automation

Hooks run an AI agent (Claude Code by default, or any compatible agent CLI) with a prompt of your choice, triggered by **new messages** or a **cron schedule**. Since the agent can call `void` itself, hooks can triage, draft, label, archive, and summarize on your behalf.

- [How it works](#how-it-works)
- [Creating hooks](#creating-hooks)
- [Triggers](#triggers)
- [Prompt placeholders](#prompt-placeholders)
- [Active window](#active-window)
- [Choosing the agent](#choosing-the-agent)
- [Hook files](#hook-files)
- [Testing and logs](#testing-and-logs)

## How it works

The sync daemon evaluates triggers. When one fires, void expands the placeholders in your prompt and spawns the agent:

```
<agent> -p "<expanded prompt>" --verbose --output-format stream-json --max-turns <N> [extra args...]
```

That's the Claude Code headless contract (`claude -p`), so any agent CLI implementing the same flags works. Results and errors are recorded in the hook log.

## Creating hooks

A hook that summarizes important emails as they arrive:

```bash
void hook create \
  --name "email-triage" \
  --trigger new_message \
  --connector gmail \
  --prompt 'A new email arrived: {message}. If it is important and requires action, reply to me on telegram (chat "Notes") with a one-line summary using void send. Otherwise do nothing.'
```

A morning digest on weekdays:

```bash
void hook create \
  --name "morning-digest" \
  --trigger schedule \
  --cron "0 8 * * mon-fri" \
  --prompt-file ~/.config/void/prompts/digest.md \
  --max-turns 10
```

Key flags for `void hook create`:

| Flag | Description |
|------|-------------|
| `--name <name>` | Unique hook name |
| `--trigger <new_message\|schedule>` | What fires the hook |
| `--connector <type>` | For `new_message`: only fire for this connector |
| `--cron <expr>` | For `schedule`: cron expression |
| `--prompt <text>` / `--prompt-file <path>` | The prompt (one required) |
| `--agent <cli>` | Agent binary to run (default: `claude`) |
| `--max-turns <N>` | Max agent turns (default: 3) |
| `--active-days`, `--active-start`, `--active-end`, `--active-utc-offset` | Restrict when the hook may fire |

## Triggers

**`new_message`** — fires for every newly synced message. Scope it with `--connector` (e.g. only Gmail). Use `{message}` in the prompt to hand the agent the full message as JSON.

**`schedule`** — fires on a cron expression, evaluated by the running sync daemon.

## Prompt placeholders

Expanded just before the agent runs:

| Placeholder | Value |
|-------------|-------|
| `{now}` | Current time, RFC 3339 (UTC) |
| `{today}` | Current date, `YYYY-MM-DD` |
| `{message}` | Full triggering message as pretty-printed JSON (`new_message` only) |
| `{message_id}` | Triggering message ID |
| `{connector}` | Triggering message's connector |
| `{connection_id}` | Triggering message's connection ID |

## Active window

Restrict a hook to certain days and hours. Outside the window, triggers are silently skipped:

```bash
void hook create ... \
  --active-days mon,tue,wed,thu,fri \
  --active-start 08:00 \
  --active-end 19:00 \
  --active-utc-offset 2
```

Windows may wrap midnight (e.g. `22:00` → `06:00`). Without `--active-utc-offset`, system local time is used.

## Choosing the agent

The default agent is `claude`. The `extra_args` field in the hook file forwards additional flags verbatim to the agent process — void doesn't interpret them. Common examples for Claude:

```toml
extra_args = ["--model", "haiku"]                                # cheaper model
extra_args = ["--allowedTools", "Bash(void *)"]                  # restrict tools
```

## Hook files

Hooks are plain TOML files in `<config dir>/hooks/` (e.g. `~/.config/void/hooks/email-triage.toml`) — version them, edit them by hand, or copy them between machines:

```toml
name = "email-triage"
enabled = true
max_turns = 3
agent = "claude"

[trigger]
type = "new_message"
connector = "gmail"

[prompt]
text = """
A new email arrived: {message}
If it is important and requires action, send me a one-line summary on Telegram.
"""

[active_window]
days = ["mon", "tue", "wed", "thu", "fri"]
start = "08:00"
end = "19:00"
utc_offset_hours = 2
```

### Remote store mode

With `store.mode = "remote"`, the sync daemon — and therefore every hook — runs on the remote host. It loads hooks from the `hooks/` directory next to the **remote** config (e.g. `~/.config/void/hooks/` on the server) and never reads the client's hooks directory.

- `void hook list`, `create`, `show`, `enable`/`disable`, `delete`, `test` and `log` are proxied over SSH, so they show and change what the remote daemon runs. `--prompt-file` is uploaded to the server first.
- If hook files exist in the client's hooks directory, every `void hook` command (and `void doctor`) warns that they are ignored.
- `void hook push [<name>...]` copies client hook files (all `*.toml`, or the named ones) to the remote hooks directory.

The daemon loads hooks at startup only: after pushing or creating hooks, restart it on the server (`void sync --stop && void sync --daemon`).

## Testing and logs

```bash
# Dry-run a scheduled hook right now
void hook test morning-digest

# Dry-run a new_message hook against a real message from your inbox
void hook test email-triage --message-id <id>

# Inspect executions: prompt sent, agent output, errors, duration
void hook log -n 20
void hook log --hook email-triage
void hook log --id 42
```

Disable without deleting: `void hook disable <name>` / `void hook enable <name>`.

## Health check

`void doctor` has a Hooks section: each configured hook with its last execution. An enabled hook is flagged `[!!]` (and counted as an issue) when:

- a `new_message` hook has not run for 24h (72h with an `active_window`) while new messages for its connector kept arriving;
- a `schedule` hook missed a cron slot by more than 1h (72h with an `active_window`), or its cron expression is invalid.

No configured hooks is reported as info (`[--]`). In remote mode the hook list comes from the remote host and executions from the synced database snapshot.
