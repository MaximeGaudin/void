---
name: quality-agent
description: Run a full codebase quality sweep by launching parallel sub-agents for dependency grooming, test improvement, and best-practices refactoring. Use when the user asks for a quality check, full audit, codebase health review, or wants to run all maintenance tasks at once.
---

# Quality Agent

Orchestrate a comprehensive codebase quality sweep by launching three specialist sub-agents in parallel using the Task tool. Each sub-agent follows its dedicated skill.

## Prerequisites

Before launching sub-agents, verify the workspace is in a clean git state:

```bash
git status --porcelain
```

If there are uncommitted changes, warn the user and ask whether to proceed.

## Workflow

### Step 1: Launch Sub-Agents

Use the **Task tool** to launch three `generalPurpose` sub-agents concurrently in a single message. Each sub-agent must receive the full instructions from its skill file — read the skill file and pass its contents in the task prompt.

Launch all three in parallel:

| Sub-Agent | Skill Path | Description |
|-----------|-----------|-------------|
| **Dependency Groomer** | `.cursor/skills/groom-dependencies/SKILL.md` | Audit, update, clean, and secure Cargo dependencies |
| **Test Improver** | `.cursor/skills/improve-tests/SKILL.md` | Fix broken tests, audit quality, add missing coverage |
| **Best Practices Auditor** | `.cursor/skills/best-practices/SKILL.md` | Scan for anti-patterns, fix them, split large files, polish |

For each sub-agent, structure the prompt as:

```
You are working on a Rust workspace at <workspace_path>.

Your task: follow the skill instructions below to completion. Track your progress
with the TodoWrite tool. When finished, return a concise summary report of
everything you did, including changes made, issues found, and any items that
need user attention.

<full contents of the skill's SKILL.md>
```

**Important**: The "Best Practices Auditor" skill requires user confirmation after Phase 1 (audit). Instruct that sub-agent to **skip the confirmation wait** and proceed through all phases, since this is an automated sweep. It should still produce the findings table in its final report.

### Step 2: Collect Results

After all three sub-agents complete, gather their summary reports.

### Step 3: README Sync Check

Verify the `README.md` is up to date with the actual codebase capabilities:

1. Read `README.md`
2. Cross-reference with:
   - CLI commands available in `crates/void-cli/src/main.rs` (the `Command` enum) and each subcommand module
   - Connector features (Gmail, Slack, WhatsApp, Calendar) — check each connector's public methods
   - Sync features (daemon, Socket Mode, etc.)
3. Flag any discrepancies:
   - Commands listed in README that no longer exist
   - Commands/features in the code that are missing from README
   - Outdated descriptions or examples
4. Fix the README to match the current state of the code

### Step 4: Build & Verify

Run a final workspace-wide verification to confirm nothing conflicts:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings 2>&1
cargo test --workspace 2>&1
cargo build --release 2>&1
```

Fix any issues introduced by the parallel work. Common conflicts:
- Two agents edited the same `Cargo.toml` — reconcile dependency versions
- Best-practices refactor moved code that test-improver added tests for — update imports
- Dependency removal broke a newly added test — restore the dep or adjust the test

### Step 5: Present Combined Report

Combine the three sub-agent reports into a single summary for the user:

```markdown
# Quality Sweep Report

## Dependency Grooming
<paste Dependency Groomer summary>

## Test Improvements
<paste Test Improver summary>

## Best Practices
<paste Best Practices Auditor summary>

## README Sync
<list any discrepancies found and fixes applied>

## Final Verification
- `cargo fmt`: ✅ / ❌
- `cargo clippy`: ✅ / ❌
- `cargo test`: ✅ / ❌
- `cargo build --release`: ✅ / ❌

## Action Items
<list any items requiring user decision>
```

## Notes

- Do **not** commit changes automatically — let the user review the combined report first.
- If a sub-agent fails or gets stuck, note it in the report and continue with the others.
- The three sub-agents operate on different aspects of the codebase, so conflicts are rare but possible. The verification step catches them.
