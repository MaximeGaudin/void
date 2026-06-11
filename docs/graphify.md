# Graphify — codebase knowledge graph

This repo ships a [graphify](https://github.com/safishamsi/graphify) knowledge graph in `graphify-out/`. It maps functions, modules, and docs into a queryable graph so AI assistants (and humans) can explore the codebase without reading every file.

- [Cursor (zero setup)](#cursor-zero-setup)
- [Install the CLI](#install-the-cli)
- [Keep the graph fresh](#keep-the-graph-fresh)
- [Optional git hooks](#optional-git-hooks)
- [Query the graph](#query-the-graph)
- [What gets committed](#what-gets-committed)
- [Contributing](#contributing)

## Cursor (zero setup)

After cloning, Cursor picks up the graph automatically:

1. **Project rule** — `.cursor/rules/graphify.mdc` tells the agent to run `graphify query`, `path`, or `explain` before grepping or reading files.
2. **Pre-built graph** — `graphify-out/graph.json` and `GRAPH_REPORT.md` are committed, so no extraction step is required on first checkout.

Open Agent chat and ask architecture questions as usual; the agent should query the graph first, then open only the files it needs.

Browse the graph visually: open `graphify-out/graph.html` in a browser, or skim `graphify-out/GRAPH_REPORT.md` for community hubs and suggested questions.

> **The graph is only as current as the last commit.** If you or someone else changed code without updating `graphify-out/`, queries return stale structure — missing new symbols, dead references, outdated docs. Check that the graph matches your branch before relying on it (see [Keep the graph fresh](#keep-the-graph-fresh)).

## Install the CLI

The graph works in Cursor without installing anything on first checkout. Install the CLI when you want to query from the terminal or refresh the graph after edits.

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

## Keep the graph fresh

**Good practice:** run `graphify update .` before you commit code or doc changes, then include the updated `graphify-out/` files in the same commit. That keeps the graph in git aligned with the code everyone else (and every Cursor session) will see.

```bash
graphify update .          # AST-only, no LLM cost
git add graphify-out/
git commit                 # code + graph together
```

`graphify update .` is fast for day-to-day work. For a full semantic re-extraction (requires an LLM API key, rarely needed):

```bash
graphify extract .
```

**Before relying on the graph** — after pulling, switching branches, or starting a long Cursor session — confirm it reflects your tree. If `graphify-out/` was not updated in recent commits, run `graphify update .` locally or query with the understanding that results may be incomplete.

## Optional git hooks

Graphify can install local git hooks (`graphify hook install`). They are **optional** — the repo does not require them, and they do not keep the committed graph in sync by themselves.

```bash
graphify hook install
graphify hook status   # post-commit and post-checkout: installed
```

What they actually do:

| Hook | When it runs | Effect |
|------|--------------|--------|
| **post-commit** | after each commit | rebuilds the graph in the **background** from the commit you just made; updated files stay **unstaged** until you commit them again |
| **post-checkout** | after branch switch | refreshes the graph for the checked-out branch |

Because the rebuild is post-commit and asynchronous, the graph update lands **after** your code commit — not inside it. Hooks are a convenience for local rebuilds, not a substitute for running `graphify update .` and committing `graphify-out/` before you push.

Hooks also register a merge driver for `graph.json`, which helps when two branches both update the graph. Re-run `graphify hook install` after upgrading graphify.

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

When your PR touches Rust or repo docs, include updated `graphify-out/` files so the graph on `main` stays trustworthy.

## Contributing

Follow [CONTRIBUTING.md](../CONTRIBUTING.md) for the full workflow. Graphify-specific expectations:

1. **Before commit** — run `graphify update .` and commit `graphify-out/` with your code or doc changes when the graph should reflect those edits.
2. **Same PR as code** — do not leave the graph stale on a branch that changes behavior or structure.
3. **Checks** — run `./scripts/check.sh` before pushing; CI must be green.
4. **Commits** — use [Conventional Commits](https://www.conventionalcommits.org/): `docs:` for documentation-only changes, `feat(scope):` / `fix(scope):` for code (include graph updates in that commit when relevant).
5. **Changelog** — add a line under `[Unreleased]` in [CHANGELOG.md](../CHANGELOG.md) when the change is user- or contributor-visible (new guides, workflow changes).
