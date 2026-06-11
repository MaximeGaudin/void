# Graphify — codebase knowledge graph

This repo ships a [graphify](https://github.com/safishamsi/graphify) knowledge graph in `graphify-out/`. It maps functions, modules, and docs into a queryable graph so AI assistants (and humans) can explore the codebase without reading every file.

- [Cursor (zero setup)](#cursor-zero-setup)
- [Install the CLI](#install-the-cli)
- [Install git hooks (required)](#install-git-hooks-required)
- [Query the graph](#query-the-graph)
- [Keep the graph fresh](#keep-the-graph-fresh)
- [What gets committed](#what-gets-committed)
- [Contributing](#contributing)

## Cursor (zero setup)

After cloning, Cursor picks up the graph automatically:

1. **Project rule** — `.cursor/rules/graphify.mdc` tells the agent to run `graphify query`, `path`, or `explain` before grepping or reading files.
2. **Pre-built graph** — `graphify-out/graph.json` and `GRAPH_REPORT.md` are committed, so no extraction step is required on first checkout.

Open Agent chat and ask architecture questions as usual; the agent should query the graph first, then open only the files it needs.

Browse the graph visually: open `graphify-out/graph.html` in a browser, or skim `graphify-out/GRAPH_REPORT.md` for community hubs and suggested questions.

> **If you change code**, the committed graph will go stale unless you [install the git hooks](#install-git-hooks-required). Without hooks, Cursor agents query outdated structure and miss new symbols or dead references.

## Install the CLI

The graph works in Cursor without installing anything on first checkout. **Install the CLI before you start editing code** — it is required for git hooks and for refreshing the graph manually.

```bash
# recommended — puts graphify on PATH automatically
uv tool install graphifyy

# alternative
pipx install graphifyy
```

Verify:

```bash
graphify --version
```

## Install git hooks (required)

**Run this once after every clone.** Hooks are not committed to the repo; each developer must install them locally.

```bash
graphify hook install
graphify hook status   # should show post-commit and post-checkout: installed
```

What they do:

| Hook | When it runs | Effect |
|------|--------------|--------|
| **post-commit** | after every commit | runs `graphify update .` (AST-only, no API cost) so `graphify-out/` stays aligned with your changes |
| **post-checkout** | after branch switch | refreshes the graph for the checked-out branch |

This is the main way graph maintenance stays automatic. Without hooks, you must remember to run `graphify update .` before every commit that touches code — easy to forget, and a stale graph misleads Cursor agents.

Hooks also register a merge driver for `graph.json`, so parallel commits union-merge the graph instead of leaving conflict markers.

Re-run `graphify hook install` after upgrading graphify so the embedded interpreter path stays correct.

## Query the graph

```bash
# natural-language exploration
graphify query "how does the sync engine work?"

# dependency path between two symbols
graphify path "main()" "SyncEngine"

# explain a concept and its neighbors
graphify explain "void-core"
```

Useful flags: `--budget 500` to cap output size, `--graph graphify-out/graph.json` to point at a specific graph file.

## Keep the graph fresh

**With hooks installed:** the post-commit hook updates the graph after each commit. Stage and commit the updated `graphify-out/` files alongside your code changes.

**Without hooks:** refresh manually before committing code changes:

```bash
graphify update .
```

For a full semantic re-extraction (requires an LLM API key, rarely needed day-to-day):

```bash
graphify extract .
```

## What gets committed

| Path | Committed? | Notes |
|------|------------|-------|
| `graphify-out/graph.json` | yes | main queryable graph |
| `graphify-out/GRAPH_REPORT.md` | yes | architecture summary |
| `graphify-out/graph.html` | yes | interactive visualization |
| `graphify-out/manifest.json` | yes | portable cache manifest |
| `.cursor/rules/graphify.mdc` | yes | Cursor agent instructions |
| `graphify-out/cache/` | no | local rebuild cache (~4 MB) |
| `graphify-out/cost.json` | no | local extraction cost log |

When committing code changes, include the hook-updated `graphify-out/` files in the same commit so everyone (and every Cursor session) sees an accurate graph.

## Contributing

Follow [CONTRIBUTING.md](../CONTRIBUTING.md) for the full workflow. Graphify-specific expectations:

1. **Hooks** — run `graphify hook install` once after every clone (see above).
2. **Same PR as code** — when your change touches Rust or repo docs, commit the updated `graphify-out/` files in the same PR.
3. **Checks** — run `./scripts/check.sh` before pushing; CI must be green.
4. **Commits** — use [Conventional Commits](https://www.conventionalcommits.org/): `docs:` for documentation-only changes, `feat(scope):` / `fix(scope):` for code (include graph updates in that commit, not a separate one).
5. **Changelog** — add a line under `[Unreleased]` in [CHANGELOG.md](../CHANGELOG.md) when the change is user- or contributor-visible (new guides, workflow changes).
