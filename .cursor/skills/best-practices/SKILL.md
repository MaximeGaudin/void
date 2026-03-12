---
name: best-practices
description: Scan a Rust codebase for bad practices, fix anti-patterns, split large files, reorganize modules by business domain, and apply idiomatic Rust conventions. Use when the user asks to clean up code, refactor, apply best practices, reorganize modules, reduce file size, or improve code quality.
---

# Best Practices — Rust Codebase Refactoring

Systematically audit and refactor a Rust codebase: detect anti-patterns, fix them, split oversized files, enforce domain boundaries, and apply idiomatic Rust.

## Workflow

Copy this checklist and track progress through each phase:

```
Refactoring Progress:
- [ ] Phase 1: Audit — scan for bad practices
- [ ] Phase 2: Fix — resolve anti-patterns in place
- [ ] Phase 3: Split — break large files into focused modules
- [ ] Phase 4: Reorganize — align modules with business domains
- [ ] Phase 5: Polish — final idiom pass and verification
```

---

## Phase 1: Audit

Scan the full codebase and produce a findings report before changing anything.

### 1.1 Automated checks

Run these commands and capture output:

```bash
cargo clippy --workspace --all-targets -- -D warnings 2>&1
cargo fmt --all -- --check 2>&1
```

### 1.2 Manual scan

Read every `.rs` file and check against [rust-checklist.md](rust-checklist.md). Flag each finding with severity:

| Severity | Meaning |
|----------|---------|
| **Critical** | Correctness or safety issue — fix immediately |
| **Warning** | Anti-pattern or code smell — fix in this pass |
| **Info** | Style nit or minor improvement — fix if nearby |

### 1.3 Duplication scan

Look for duplicated code across the workspace. Common patterns to watch for:

- **Copy-pasted functions or blocks** — identical or near-identical logic in multiple files/crates
- **Repeated struct/enum definitions** — the same type defined in more than one crate instead of shared via `void-core`
- **Boilerplate patterns** — similar error handling, API call wrappers, or mapping code across connectors that could be abstracted into a shared trait or helper
- **Repeated constants or config strings** — magic values duplicated instead of defined once

For each duplicate found, note the locations and whether it should be extracted to a shared module, generalized with a trait/generic, or consolidated into `void-core`.

### 1.4 Placeholder & stub code scan

Search for comments and code that signal incomplete or fake implementations. These are **critical** findings because the code appears to work but silently does nothing.

```bash
rg -in "todo|fixme|hack|xxx|stub|placeholder|not yet implemented|unimplemented|to be implemented|future:|later:" --type rust
rg -n "// .*Future:" --type rust
```

Patterns to flag:

| Pattern | Why it's dangerous |
|---------|--------------------|
| `// TODO` / `// FIXME` / `// HACK` / `// XXX` | Marks known incomplete work |
| `// Future:` / `// later:` / `// stub` | Deferred implementation that may be forgotten |
| Empty loop bodies (`loop { ... => { } }`) | Silently drops events/data |
| Empty match arms with only `debug!()` or a comment | Ignores important cases |
| `unimplemented!()` / `todo!()` macro calls | Will panic at runtime |
| Functions that return hardcoded values or immediately `Ok(())` without doing real work | Fake implementations |
| Poll/tick loops where the tick handler is a no-op | Pretends to sync but does nothing |

Each finding is **Critical** severity — these must be either implemented or explicitly removed with a clear rationale.

### 1.5 File size audit

List files exceeding **400 lines** (excluding tests). These are split candidates.

### 1.6 Produce findings report

Before making any changes, present a summary table to the user:

```
| File | Severity | Finding | Proposed fix |
|------|----------|---------|--------------|
| ...  | ...      | ...     | ...          |
```

Wait for user confirmation before proceeding to Phase 2.

---

## Phase 2: Fix Anti-Patterns

Work through the findings report, fixing in dependency order (core crates first, then consumers).

### Fix order

1. **void-core** — models, errors, config, db
2. **Connector crates** — void-slack, void-gmail, void-calendar, void-whatsapp
3. **void-cli** — commands, output

### Key fixes (see [rust-checklist.md](rust-checklist.md) for full list)

- Replace `anyhow` with domain-specific `thiserror` types where errors cross crate boundaries
- Remove unused dependencies from `Cargo.toml`
- Replace `.unwrap()` / `.expect()` in non-test code with proper error propagation
- Convert `clone()` on references to borrows where ownership isn't needed
- Use `&str` over `String` in function parameters when the callee doesn't need ownership
- Add `#[must_use]` to pure functions returning values
- Replace manual `impl Display` with `#[derive(Display)]` or `thiserror` where appropriate
- Use `std::mem::take` / `Option::take` instead of `clone` + reassign
- Deduplicate copy-pasted code — extract shared logic into functions, traits, or `void-core` modules

### After each fix

```bash
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

If any command fails, fix the regression before moving on.

### Commit

Once all Phase 2 fixes pass, commit:

```bash
git add -A && git commit -m "refactor: fix anti-patterns (best-practices phase 2)"
```

---

## Phase 3: Split Large Files

Target: no non-test `.rs` file exceeds **300 lines**.

### Splitting strategy

1. **Identify cohesive groups** — look for `// ---- section ----` comments, impl blocks for distinct types, or logically grouped functions
2. **Extract to submodule** — move the group into its own file under a directory module
3. **Re-export from parent** — keep the public API unchanged via `pub use`

### Module conversion pattern

When splitting `foo.rs` into a directory module:

```
Before:            After:
src/foo.rs    →    src/foo/mod.rs      (re-exports only)
                   src/foo/bar.rs      (extracted group 1)
                   src/foo/baz.rs      (extracted group 2)
```

`mod.rs` should contain only:
```rust
mod bar;
mod baz;

pub use bar::*;
pub use baz::*;
```

### Common split targets in this workspace

| File | Likely splits |
|------|---------------|
| `db.rs` (~900 lines) | `schema.rs` (migrations), `queries.rs` (CRUD), `search.rs` (FTS5) |
| `whatsapp/channel.rs` (~900 lines) | `sync.rs` (sync logic), `mapping.rs` (message mapping), `channel.rs` (trait impl) |
| `gmail/channel.rs` (~500 lines) | `sync.rs`, `channel.rs` |
| `calendar/channel.rs` (~500 lines) | `sync.rs`, `channel.rs` |
| `slack/channel.rs` (~470 lines) | `backfill.rs`, `channel.rs` |
| `output.rs` (~350 lines) | `format.rs` (formatters), `table.rs` (table rendering) |

### After each split

Verify the public API is unchanged:
```bash
cargo check --workspace
cargo test --workspace
```

### Commit

After all splits are done and verified, commit:

```bash
git add -A && git commit -m "refactor: split large files into focused modules (best-practices phase 3)"
```

---

## Phase 4: Reorganize by Business Domain

Ensure each crate has a clear, single responsibility and modules map to domain concepts.

### Domain alignment checklist

- [ ] **void-core**: Only domain models, traits, config, persistence — no connector logic
- [ ] **Each connector crate**: Only that connector's API client, auth, mapping, and `Channel` impl
- [ ] **void-cli**: Only CLI parsing, output formatting, and orchestration — no business logic
- [ ] **Shared types live in void-core**, not duplicated across connectors
- [ ] **No circular dependencies** between crates

### Structural rules

- One public type per file (structs/enums with significant logic); small helper types can coexist
- Module names match the domain concept they represent (`auth`, `sync`, `api`, `models`)
- No `utils.rs` or `helpers.rs` catch-all files — distribute to domain modules
- Test modules stay in the file they test (`#[cfg(test)] mod tests`)

### If moving types between crates

1. Move the type to the target crate
2. Update all imports across the workspace
3. Run `cargo check --workspace` after each move

### Commit

After reorganization is done and verified, commit:

```bash
git add -A && git commit -m "refactor: reorganize modules by business domain (best-practices phase 4)"
```

---

## Phase 5: Polish

Final pass for idiomatic Rust and consistency.

### Idiom checklist

- [ ] All public items have doc comments (`///`)
- [ ] Error types use `thiserror` with descriptive messages
- [ ] `#[derive]` order is consistent: `Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize`
- [ ] Enums use `#[non_exhaustive]` where future variants are expected
- [ ] `impl Default` uses `#[derive(Default)]` when possible
- [ ] No `pub` fields on structs with invariants — use constructor + getters
- [ ] Consistent `use` grouping: std → external crates → internal crates → `self`/`super`
- [ ] `cargo fmt --all` passes
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` passes
- [ ] `cargo test --workspace` passes

### Final verification

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --release
```

All four must pass with zero warnings before considering the refactoring complete.

### Commit

```bash
git add -A && git commit -m "refactor: polish idiomatic Rust and formatting (best-practices phase 5)"
```

---

## Additional Resources

- For the full anti-pattern checklist, see [rust-checklist.md](rust-checklist.md)
