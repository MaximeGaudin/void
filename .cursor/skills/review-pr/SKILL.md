---
name: review-pr
description: >-
  Review a user-submitted pull request on void-cli end to end: qualify intent
  against CONTRIBUTING.md and project philosophy, run a paranoid security audit
  to catch malicious or risky code, verify test coverage is merge-worthy,
  confirm the diff follows existing implementation patterns, and review for
  correctness, style, and quality. Uses gh to fetch the PR, diff, and CI, and
  self-updates with durable lessons learned after each review. Use when the user
  asks to review a PR, vet a contribution, security-check a pull request, or
  decide whether to merge external code.
---

# Review a Submitted PR

Review an external/user-submitted PR on `MaximeGaudin/void` across intent, security, implementation patterns, test coverage, and code quality — in that order. **Treat the author as untrusted.** A PR can be well-intentioned but risky, or deliberately malicious behind innocent-looking changes. Default posture: skeptical. The smallest suspicious element gets investigated until explained.

Run all `gh`/`cargo` commands from the void-cli repository root.

## Inputs

The user gives a PR number or URL. If missing, ask. Then gather context:

```bash
PR=<number>
gh pr view "$PR" --json number,title,body,author,headRefName,baseRefName,additions,deletions,changedFiles,url,mergeable,labels
gh pr diff "$PR"                       # full diff
gh pr diff "$PR" --name-only           # changed file list
gh pr view "$PR" --json files --jq '.files[] | "\(.additions)+ \(.deletions)- \(.path)"'
gh pr checks "$PR"                     # CI status
```

For larger PRs, also skim commit-by-commit (`gh pr view "$PR" --json commits`) — malicious changes are sometimes buried in a noisy "formatting" commit.

#### Determine the *effective* delta (not just the raw diff)

A PR's diff is only meaningful relative to a current `main`. Branches are often cut from an old `main`, which inflates and confuses the diff. Before reviewing, establish what is genuinely new:

```bash
git fetch origin main "$headRefName"
gh pr view "$PR" --json mergeable,mergeStateStatus,commits
# Are any of the PR's commits already on main (merged independently)?
gh pr view "$PR" --json commits --jq '.commits[].messageHeadline' \
  | while read -r h; do git log origin/main --oneline | rg -qF "$h" && echo "ALREADY ON MAIN: $h"; done
# Or per touched file:
gh pr diff "$PR" --name-only | while read -r f; do echo "== $f =="; git log origin/main --oneline -1 -- "$f"; done
```

- **`mergeable: CONFLICTING`** almost always means a stale base. Find out *why* it conflicts — frequently because some of the PR's commits already landed on `main` separately, leaving only one genuinely-new commit buried in noise.
- Review the **effective delta** (the novel commit[s]), and call out the redundant/already-merged commits as a focus problem (Step 1) rather than reviewing them as if new.

## Workflow

```
Review Progress:
- [ ] Step 0: Fetch PR metadata, diff, CI status
- [ ] Step 1: Qualify intent & fit (CONTRIBUTING.md + philosophy)
- [ ] Step 2: Security audit (paranoid, exhaustive)
- [ ] Step 3: Code review (correctness, quality, conventions)
  - [ ] Step 3a: Implementation pattern conformance
  - [ ] Step 3b: Test coverage assessment
- [ ] Step 4: Verdict & report
- [ ] Step 5: Capture lessons learned into this skill (always)
```

---

### Step 1: Qualify intent & fit

Decide whether this PR *should exist at all* before reviewing the code. A clean implementation of an unwanted change is still a reject.

Read `CONTRIBUTING.md`, `README.md` (philosophy), and the relevant `docs/` page. Then check:

| Question | Reject / push back if… |
|----------|------------------------|
| Is the intent clear? | PR body is empty or doesn't explain *what* and *why*. |
| Does it fit the philosophy? | Breaks **local-first** (adds a phone-home, external DB, cloud dependency, telemetry), or **agent/terminal-first** ergonomics. |
| Is it focused? | Bundles unrelated changes — CONTRIBUTING requires "one logical change per PR". Also flag commits already merged into `main` (see *effective delta* above): they add nothing and cause conflicts. |
| New connector? | Should follow `docs/adding-a-connector.md` and ideally have had a prior issue to agree on the approach (auth model, push vs. polling). |
| Cross-platform? | Introduces Unix-only behavior, hardcoded `/tmp`/`~/.config`/`HOME`, or non-portable deps without `#[cfg]` gating (CI tests Linux/macOS/Windows). |
| Conventions | Commits not [Conventional Commits]; user-visible change missing a `CHANGELOG.md` `[Unreleased]` entry; behavior change missing docs. |
| License | Contribution must be AGPL-3.0 compatible; flag vendored code with incompatible licenses. |

Output: a short verdict on fit (**fits** / **needs changes** / **out of scope**) with reasoning, independent of code quality.

---

### Step 2: Security audit (paranoid)

This is the highest-priority step. Goal: guarantee nothing malicious or risky lands in `main`. Investigate every anomaly until you can explain it. When in doubt, flag it — do not give the benefit of the doubt.

Void handles sensitive data (OAuth tokens, message contents, session files stored unencrypted under the store dir, protected by file perms only — see `SECURITY.md`). The attack surface that matters most: credential exfiltration, token theft, arbitrary code/command execution, and supply-chain injection.

#### 2a. Dependency & supply-chain changes

A new dependency is the single easiest way to slip attacker-controlled code into the project: it runs with full process privileges, can ship a `build.rs` that executes at compile time, and updates outside this PR's review. **Default posture: every newly added crate is a blocker until proven both _necessary_ and _trustworthy_.** "It compiles" / "it works" is not a justification — nothing stops a contributor from adding a dependency they control to do shady things later.

Inspect every change to `Cargo.toml`, `Cargo.lock`, and `deny.toml`, and enumerate exactly what entered the tree (direct **and** transitive):

```bash
gh pr diff "$PR" -- '**/Cargo.toml' 'Cargo.lock' 'deny.toml'
# Every crate newly added to the lockfile (catches transitive deps the PR didn't name):
git diff "main...HEAD" -- Cargo.lock | rg '^\+name = ' | sort -u
gh pr checkout "$PR"
cargo audit                          # RUSTSEC advisories
cargo deny check advisories licenses bans sources
cargo tree -i <new-crate>            # who pulls it in, and why?
git checkout "$baseRefName" 2>/dev/null || git checkout main
```

**For each newly added crate (direct or transitive), ALL of the following must hold. If any fails, it is a Blocker — push back before merge:**

1. **Necessary.** The PR cannot reasonably reach its goal with `std`, an already-vendored crate, or a few lines of local code. Reject deps added for trivial convenience (left-pad–class), or a heavyweight crate pulled in for one helper function. Ask explicitly: "what does *removing* this dependency cost?"
2. **Reputable source.** Published on **crates.io** (never a `git` / `path` / alternate-registry source), owned by a recognizable publisher/org, links to a real public repo, and **actively maintained** (recent releases, responsive issues). Sanity-check scale: meaningful download counts and reverse-dependencies — not a days-old crate with a single `0.0.x` version.
3. **Not a typo-squat / impersonation.** The crate name *and* its repo match the well-known crate it appears to be (`reqwest` vs `request`, `tokio` vs `tokey`, `serde` look-alikes). Open the repo link and confirm it actually hosts that crate.
4. **Clean advisories & license.** No open RUSTSEC advisory (`cargo audit`); license is on the `deny.toml` allow-list (`cargo deny`).
5. **Proportional blast radius.** Note how many transitive crates it drags in and whether any ship a `build.rs` / `proc-macro`. A one-line feature that adds dozens of transitive crates is itself a red flag.

Record a one-line justification per new crate in the report: `name · why necessary · source/publisher · downloads/maintenance · advisories`. An unexplained or unjustified dependency means the security verdict **cannot** be `clean`.

| Red flag | Action |
|----------|--------|
| New dependency with no necessity justification | **Blocker** — request removal or justification; prefer `std`/existing deps. |
| Obscure / new / unmaintained crate (low downloads, single version, dead or missing repo) | **Blocker** until a reputable equivalent is used or ownership is verified. |
| Dep from a `git` / `path` / alt-registry source instead of crates.io | **Blocker** — inspect the target repo/commit; do not accept opaque sources. |
| Name or repo resembles a typo-squat / impersonation | **Do-not-merge** until the crate's identity is confirmed. |
| Version bound widened, wildcarded (`*`), or pointed at a pre-release | Flag — loosened bounds let a future malicious release in silently. |
| Lockfile churn unrelated to the stated change | Investigate why; a PR "fixing a typo" should not rewrite `Cargo.lock`. |
| New license outside `deny.toml` allow-list | Block until intentionally allow-listed. |
| `build.rs` added or modified | Read it line by line — build scripts run arbitrary code at compile time on the maintainer's/CI machine. |
| `proc-macro` crate added | Same risk class as `build.rs`; scrutinize. |

#### 2b. Code-level malicious patterns

Grep the diff (and changed files) for high-risk constructs, then read each hit in context:

```bash
gh pr checkout "$PR"
```

Search for, and justify every occurrence of:

- **Exfiltration:** network calls in unexpected places (`reqwest`, `ureq`, `TcpStream`, raw sockets, DNS lookups) — especially near token/credential/message handling. Any URL or IP literal that isn't a known service API endpoint.
- **Command/code execution:** `std::process::Command`, `Command::new`, shell invocations, `sh -c`, `eval`-like patterns, `libloading`/`dlopen`, `include!`/`include_bytes!` of unexpected paths.
- **Unsafe & FFI:** new `unsafe` blocks, `extern "C"`, `mem::transmute`, raw pointer arithmetic — verify each is sound and necessary.
- **Filesystem reach:** reads/writes outside the store dir, touching `~/.ssh`, `~/.aws`, `~/.config`, `.env`, `/etc`, browser profiles, or other credential stores.
- **Env & secrets:** new `std::env::var` reads of sensitive vars, code that logs/prints tokens or message bodies, weakened redaction.
- **Permission downgrades:** changes to the `0600`/`0700` file-permission logic on credential/session files (see `SECURITY.md` threat model).
- **Obfuscation:** base64/hex/byte-array blobs, dynamically built strings, char-code arithmetic, unusually encoded constants — decode and explain them.
- **Network/endpoint swaps:** changes to API base URLs, OAuth endpoints, or redirect URIs.
- **CI/workflow tampering:** changes under `.github/` (new `permissions:`, `secrets:`, `run:` steps, untrusted actions, `pull_request_target`) — a classic exfiltration vector.
- **Hooks:** changes to hook execution / `extra_args` handling that could broaden what the agent CLI is allowed to do.

```bash
# Example sweeps — read each hit, do not trust counts alone
git diff "main...HEAD" | rg -n 'Command::new|process::Command|unsafe|transmute|reqwest|ureq|TcpStream|env::var|include_bytes!|build\.rs|base64|from_str_radix'
git diff "main...HEAD" -- '.github/'
```

#### 2c. Data-handling review

For connector code, confirm: credentials still written with restrictive perms, tokens never logged, message content not sent anywhere except the intended service API, no new persistence outside the store dir.

Return to the base branch when done: `git checkout main`.

Output: **security verdict** — `clean` / `concerns` / `do-not-merge`, with each finding, its location (`file:line`), why it's risky, and what would resolve it. If anything is unexplained, the verdict cannot be `clean`.

---

### Step 3: Code review (quality & correctness)

Only meaningful after Steps 1–2. Review the diff for:

- **Correctness:** logic bugs, unhandled edge cases, off-by-one, race conditions, incorrect async/`.await`, error paths that `unwrap()`/`panic!` where they should propagate `Result`.
- **Error handling:** consistent with the codebase's error types; no swallowed errors; no silent failures.
- **Idiomatic Rust:** would `cargo clippy -- -D warnings` complain? Unnecessary clones/allocations, blocking calls in async, `unwrap`/`expect` on fallible paths, missing `?`.
- **Docs & changelog:** updated when behavior changes.
- **Cross-platform:** path handling via `std::path`, no Unix-only assumptions without `#[cfg]` gating.

For **removal / refactor PRs**, verify the change is *complete against current `main`*, not just internally consistent with its own diff:

```bash
# Leftover references anywhere in the tree (symbols, modules, commands, docs, config tables, issue templates)
rg -n '<feature-or-crate-name>' --glob '!CHANGELOG.md'
# A file the PR deletes may have been restructured on main (e.g. api.rs split into api/). After a conceptual
# rebase the stale deletion won't cover the new files — confirm the path is truly gone:
git ls-files crates/<removed-crate>/
```

- Removals routinely miss: `docs/configuration.md` tables, `docs/commands.md`, `.github/ISSUE_TEMPLATE/*`, the README feature line, and the CHANGELOG `Removed` entry. Grep docs for the feature name.
- Exclude unrelated false-positives (e.g. a Slack `gdrive` filetype string is unrelated to a `void-gdrive` crate) — read each hit, don't trust the match.

Then complete **Step 3a** and **Step 3b** before classifying findings — a correct implementation that ignores project patterns or ships without adequate tests is not merge-ready.

#### Step 3a: Implementation pattern conformance

Do not review the diff in isolation. For each area touched, read how the codebase already solves the same kind of problem and compare the PR against that reference implementation.

Start by identifying the closest analogue(s):

```bash
gh pr diff "$PR" --name-only
# Then read 1–2 existing modules in the same crate (or an analogous connector) that solve the same problem
```

| Area | Expected pattern in this repo | Reject / push back if… |
|------|-------------------------------|------------------------|
| **Crate boundaries** | `void-core` = models, traits, config, DB, sync, hooks; `void-<connector>` = API client, auth, mapping, `Connector` impl; `void-cli` = clap parsing, output formatting, orchestration only | Business logic lands in `void-cli`; connector logic duplicated across crates instead of shared via `void-core` |
| **Errors** | `thiserror` types in library crates; `anyhow` in the binary crate | Wrong error layer, swallowed errors, `.unwrap()`/`.expect()` on fallible paths in production code |
| **Connectors** | Follows `docs/adding-a-connector.md`: `Connector` trait, resumable backfill cursor, incremental sync until `CancellationToken` fires, ingest `eprintln!` format `[connector:connection_id] …` | Missing sync contract pieces, ad-hoc persistence outside the store dir, divergent module layout (`auth`, `sync`, `api`, `models`) |
| **Module layout** | Domain-named modules; large files split into submodules; no `utils.rs`/`helpers.rs` catch-alls | New catch-all modules, logic placed in the wrong crate or layer |
| **Async & sync** | `tokio` + `CancellationToken` for long-running work; no blocking I/O in async paths | Blocking calls in async without justification; sync loops that ignore cancellation |
| **Config & models** | Extend existing `ConnectorType`, `ConnectionSettings`, serde patterns in `void-core` | One-off config structs, stringly-typed enums, schema changes without migration tests |
| **Database** | Access via `Database` methods in `void-core/src/db/`; migrations tested for data preservation | Raw SQL or schema logic outside `db/`; migration without a preservation test |
| **HTTP clients** | Test constructor like `with_base_url(...)`; production client separate from parsing helpers | Real network in unit tests; parsing mixed into transport layer |
| **CLI output** | Existing formatters and JSON envelope shapes; read-path changes may need `insta` snapshots | Ad-hoc printing, breaking JSON contract without snapshot update |
| **Hooks & remote store** | Match existing hook runner and fake-`ssh`/`scp` on `PATH` patterns (Unix-gated where needed) | New subprocess or filesystem patterns without cross-platform handling |
| **Shared fixtures** | Reuse `void_core::test_fixtures` (feature `test-fixtures`) for DB seeds | Duplicate fixture builders inline in every test module |

Flag every deviation as **Blocker** (architectural mismatch), **Should-fix** (works but inconsistent), or **Nit** (minor style drift). Cite the reference file the PR should have matched.

#### Step 3b: Test coverage assessment

Read `docs/testing.md` before judging tests. Green CI alone is not enough — evaluate whether the **amount and coverage** of tests is adequate for merge.

1. **Map changes to tests.** For each production file or public API change in the diff, locate the corresponding test module or integration test. List what is covered and what is not.

```bash
gh pr diff "$PR" --name-only | rg '\.rs$' | rg -v '/tests\.rs$|/tests/'
# For each changed source file, find its #[cfg(test)] mod or sibling tests.rs
gh pr diff "$PR" -- '**/tests/**' '**/tests.rs' '**/*_test.rs'
```

2. **Compare to area conventions** (from `docs/testing.md`):

| Changed area | Minimum expected tests |
|--------------|---------------------|
| **Connector API parsing / mapping** | Happy path + error paths (401, 429, 5xx, malformed JSON) via `wiremock::MockServer` and the client's `with_base_url` constructor |
| **Sync engine / orchestration** | Mock `Connector` test double (see `void-core/src/sync/tests.rs`); failure isolation, cancellation, lock release |
| **Database / migrations** | In-memory `Database::open_in_memory()`; schema snapshot + data-preservation on migration |
| **Config (de)serialization** | Round-trip and legacy-format migration cases |
| **CLI read paths / JSON output** | `void-cli/tests/read_paths.rs` and/or `read_paths_snapshots.rs` (`insta`) when output shape changes |
| **CLI commands (surface)** | `cli_contract.rs` `--help` / required-arg checks when new commands or flags are added |
| **Hooks** | Trigger matching, scheduling, stub agent execution (Unix-gated where applicable) |
| **Pure helpers / formatters** | Unit tests for success + error/boundary cases; deterministic inputs (fixed `chrono` instants, no wall clock) |

3. **Judge merge-worthiness.** Assign a test verdict:

- **adequate** — behavioral changes are exercised at the same depth as analogous code elsewhere; error and edge paths covered where the production code branches; conventions followed (no real network, `tempfile`/`temp_dir`, no `#[ignore]`).
- **needs more** — core behavior is tested but important branches, error paths, or regression cases are missing; request tests before merge unless the gap is truly trivial.
- **insufficient for merge** — non-trivial behavior change with no new or updated tests, critical path untested (auth, sync ingest, credential handling, migrations, CLI contract), or tests that violate project conventions.

Treat **insufficient for merge** as a **Blocker**. Treat **needs more** as **Blocker** for high-risk areas (sync, auth, DB, credential I/O) and **Should-fix** elsewhere.

4. **Sanity-check test quality**, not just presence: names follow `<function>_<scenario>`; one behavior per test; no flaky wall-clock or real filesystem dependencies; platform-specific tests are `#[cfg]`-gated, not `#[ignore]`.

Verify CI rather than trusting it blindly:

```bash
gh pr checks "$PR"
# If unsure, reproduce locally on the PR branch:
gh pr checkout "$PR"
./scripts/check.sh                          # fmt + clippy + test
cargo +1.95.0 check --workspace --locked    # MSRV gate (match rust-version in Cargo.toml)
git checkout main
```

Reading CI state:

- **"no checks reported"** = CI never ran for this branch tip (often a stale branch that was never re-pushed). Don't read it as "passing" — reproduce locally or push to trigger.
- **`mergeStateStatus: UNSTABLE` with `mergeable: MERGEABLE`** = non-required checks are pending/failing but nothing blocks the merge. If checks aren't configured as *required* in branch protection, `gh pr merge --auto` merges **immediately** rather than waiting for the visible checks — so a local `./scripts/check.sh` pass is your real gate.
- The MSRV moves over time; read `rust-version` in `Cargo.toml` instead of hardcoding a toolchain.

Classify each code finding by severity:

- **Blocker** — bug, broken CI, or correctness issue that must be fixed before merge
- **Should-fix** — quality/maintainability/convention issue worth addressing
- **Nit** — optional polish

---

### Step 4: Verdict & report

Produce a single report. Lead with the recommendation.

```markdown
# PR Review — #<N>: <title>

**Recommendation:** Merge / Request changes / Do not merge / Needs discussion
**Author:** <login>  ·  +<additions>/-<deletions> across <n> files  ·  CI: <status>

## 1. Intent & fit
<fits / needs changes / out of scope> — <1–3 sentences of reasoning>

## 2. Security  ·  Verdict: clean / concerns / do-not-merge
- **Dependencies:** <none added> — or one line per new crate: `name · necessary? · source/publisher · downloads/maintenance · advisories`
- [severity] <finding> — `path:line` — <why it's risky> — <resolution>
- (or) No security concerns found after auditing deps, exec/network/unsafe surface, file perms, and CI workflows.

## 3. Code review

### Implementation patterns
<conforms / minor drift / significant mismatch> — <1–2 sentences; cite reference files the PR should match>

### Test coverage  ·  Verdict: adequate / needs more / insufficient for merge
- <what changed> → <what is tested / what is missing>
- (or) Test coverage matches project conventions for this change type.

### Blockers
- <finding> — `path:line`
### Should-fix
- <finding> — `path:line`
### Nits
- <finding> — `path:line`

## Summary
<2–4 sentences: overall quality, what must change before merge, any follow-ups>
```

---

### Step 5: Capture lessons learned (always)

**After every review, update this skill with any durable, reusable knowledge the review surfaced.** This is mandatory, not optional — the skill should get sharper with each PR it reviews.

- Add only **generalizable** insights that would speed up or improve a *future* review: a repo convention you discovered, a recurring failure mode, a faster command, a gotcha in CI/merge mechanics, a class of bug to watch for.
- Do **not** record PR-specific trivia (file names, one-off details, the verdict of this particular PR).
- Put each lesson where it belongs: fold command/process tips into the relevant step, and log the rest under **## Lessons learned** below (newest first, one concise bullet each). Refine or merge existing bullets rather than letting the list sprawl.
- Make the edit with the file-edit tool and mention in your final summary that you updated the skill (and what you added). If the review genuinely surfaced nothing new, say so explicitly instead of forcing an edit.

---

## Guardrails

- **Read-only by default.** Reviewing means inspecting, not editing the PR — except for Step 5, which always updates *this skill file*. Do not push to the author's branch or merge unless the user explicitly asks.
- **Never run untrusted PR code outside vetting commands.** `cargo build`/`test` on a PR branch executes that branch's `build.rs`, proc-macros, and test code on this machine. Complete Step 2a (inspect `build.rs`, deps) *before* building. If anything looks malicious, do **not** build/test locally — report instead.
- Always return to `main` after `gh pr checkout`.
- A `clean` security verdict requires that **every** anomaly was explained. Unexplained ≠ clean.
- **Every new dependency is guilty until proven necessary _and_ reputable** (Step 2a). Vet direct and transitive crates; an unjustified or obscure dep blocks the merge regardless of how clean the code looks.
- Don't let a polished diff lower your guard — clean code is the easiest place to hide a malicious line.
- Be specific: every finding cites `file:line` and explains impact, not just "looks off".

## Quick reference

```bash
PR=<number>
gh pr view "$PR" --json title,body,author,additions,deletions,changedFiles,url
gh pr diff "$PR"
gh pr checks "$PR"

# Security sweeps (inspect deps BEFORE checkout/build)
gh pr diff "$PR" -- '**/Cargo.toml' 'Cargo.lock' 'deny.toml' '.github/'
git diff "main...HEAD" -- Cargo.lock | rg '^\+name = ' | sort -u   # every new (incl. transitive) crate
gh pr checkout "$PR"
cargo audit && cargo deny check advisories licenses bans sources
cargo tree -i <new-crate>                                          # justify each new dep
git diff main...HEAD | rg -n 'Command::new|unsafe|transmute|reqwest|ureq|env::var|build\.rs|base64'

# Quality (only after security clears)
./scripts/check.sh
cargo +"$(rg '^rust-version' Cargo.toml | grep -oE '[0-9]+\.[0-9]+(\.[0-9]+)?')" check --workspace --locked
git checkout main
```

## Lessons learned

Append new, durable review insights here (newest first), per Step 5.

- **Stale branches masquerade as big PRs.** A `CONFLICTING` PR with a noisy multi-commit diff is usually cut from an old `main` with one or more commits already merged independently. Compute the effective delta first (commit-headline / per-file `git log origin/main` checks) — the real change is often a single commit.
- **Removal PRs are easy to leave half-done.** Verify completeness against *current* `main`: grep the whole tree for leftover references and check docs/config tables, `.github/ISSUE_TEMPLATE`, README, and CHANGELOG. Watch for files the PR deletes that `main` has since restructured (e.g. `api.rs` → `api/`) — the stale deletion misses the new files.
- **CI signals are subtle.** "no checks reported" ≠ passing (CI never ran); `UNSTABLE` + `MERGEABLE` means non-required checks aren't blocking and `--auto` merges immediately. Treat a local `./scripts/check.sh` pass as the real gate.
- **Read the MSRV from `Cargo.toml` (`rust-version`)** rather than hardcoding a toolchain — it moves (1.89 → 1.95 → …).
