# Rust Anti-Pattern Checklist

Detailed checklist for Phase 1 (Audit) and Phase 2 (Fix). Each section lists what to look for and how to fix it.

---

## Error Handling

| Anti-pattern | Fix |
|---|---|
| `anyhow::Result` used across crate boundaries | Define `thiserror` error enums per crate; reserve `anyhow` for the binary crate and internal fallible helpers |
| `.unwrap()` / `.expect()` in non-test code | Propagate with `?` or return a meaningful error |
| `thiserror` in `Cargo.toml` but unused | Either remove the dep or introduce proper error types |
| Stringly-typed errors (`anyhow!("something failed")`) without context | Use `.context("doing X for Y")` or typed errors with fields |
| Ignoring `Result` with `let _ = fallible_fn()` without justification | Handle or log the error; add a comment if intentionally ignored |

---

## Ownership & Borrowing

| Anti-pattern | Fix |
|---|---|
| `fn foo(s: String)` when `fn foo(s: &str)` suffices | Accept `&str` (or `impl AsRef<str>`) unless ownership is needed |
| `thing.clone()` passed immediately to a function | Pass a reference if the callee only reads |
| `Vec<T>` parameter when `&[T]` works | Accept `&[T]` for read-only access |
| Returning `String` when a `&str` lifetime is available | Return `&str` tied to `&self` or input lifetime |
| `Box<dyn Trait>` when `impl Trait` or generics fit | Prefer static dispatch unless dynamic dispatch is required |

---

## Async & Concurrency

| Anti-pattern | Fix |
|---|---|
| `async fn` that never `.await`s | Remove `async` or justify why it needs to be async |
| Blocking calls (`std::fs`, `std::thread::sleep`) in async context | Use `tokio::fs`, `tokio::time::sleep` |
| `Arc<Mutex<T>>` when `Arc<RwLock<T>>` fits (readers >> writers) | Switch to `RwLock` for concurrent read access |
| Spawning tasks without `JoinHandle` tracking | Store handles and await them for graceful shutdown |
| Holding a lock across an `.await` point | Restructure to drop the guard before awaiting |

---

## Type Design

| Anti-pattern | Fix |
|---|---|
| Boolean parameters (`fn send(msg, true, false)`) | Replace with descriptive enums |
| Stringly-typed IDs (`account_id: String`) | Use newtype wrappers (`struct AccountId(String)`) for type safety |
| Large enum variants creating size disparity | Box the large variant's payload |
| `Option<Option<T>>` | Flatten into an enum with `Absent`, `Null`, `Present(T)` |
| Public struct fields with validation invariants | Make fields private, add `new()` constructor and getters |

---

## API Design

| Anti-pattern | Fix |
|---|---|
| Multiple `bool` or `Option` params in a function signature | Use a builder pattern or config struct |
| Functions longer than ~50 lines | Extract helpers with descriptive names |
| `impl` blocks mixing public API with internal helpers | Separate into `impl Foo` (public) and a private `impl Foo` block, or extract helpers to private functions |
| Inconsistent method naming (`get_x` vs `x` vs `fetch_x`) | Standardize: `x()` for getters, `set_x()` for setters, `fetch_x()` for I/O |
| Missing `#[must_use]` on pure functions | Add `#[must_use]` to functions whose return value should not be silently ignored |

---

## Module & File Organization

| Anti-pattern | Fix |
|---|---|
| Single file > 300 lines (excluding tests) | Split into focused submodules (see Phase 3 in SKILL.md) |
| `utils.rs` or `helpers.rs` catch-all | Distribute functions to the domain module that uses them |
| Deeply nested module hierarchy (> 3 levels) | Flatten; prefer `crate::module::Type` over `crate::a::b::c::Type` |
| `pub use` re-exporting everything from a submodule without curation | Re-export only the public API surface |
| Mixing domain logic with infrastructure (HTTP, DB) in one file | Separate into `domain.rs` (types + logic) and `infra.rs` (I/O) |

---

## Dependencies & Cargo

| Anti-pattern | Fix |
|---|---|
| Unused dependencies in `Cargo.toml` | Remove them; use `cargo machete` to detect |
| Feature flags enabling more than needed | Minimize features (e.g., `tokio = { features = ["rt", "macros"] }` vs `"full"`) |
| Missing `#[cfg(test)]` on test-only dependencies | Move to `[dev-dependencies]` |
| Duplicated dependency versions across workspace members | Use `[workspace.dependencies]` and `dep.workspace = true` |
| No `rust-version` / MSRV set | Set `rust-version` in workspace `Cargo.toml` |

---

## Testing

| Anti-pattern | Fix |
|---|---|
| No tests for public API | Add unit tests covering core paths and edge cases |
| Tests that depend on external services without mocking | Introduce trait-based mocking or use `mockall` |
| Test names like `test1`, `test_it_works` | Name tests after the behavior: `test_returns_error_on_invalid_input` |
| Assertions without messages (`assert!(x)`) | Use `assert!(x, "expected X because ...")` |
| Large integration tests duplicating unit test coverage | Keep unit tests focused; integration tests for cross-crate flows |

---

## Performance & Safety

| Anti-pattern | Fix |
|---|---|
| Collecting into `Vec` only to iterate once | Chain iterators instead of collecting |
| `format!()` for building SQL/queries | Use parameterized queries to prevent injection |
| Allocating in a hot loop (`String::new()` per iteration) | Pre-allocate outside the loop and `.clear()` |
| `to_string()` / `to_owned()` on string literals where `&'static str` works | Use `&str` or `Cow<'_, str>` |
| `clone()` inside `map`/`filter` closures | Use references or restructure to avoid cloning |

---

## Documentation & Style

| Anti-pattern | Fix |
|---|---|
| No doc comments on public items | Add `///` with a one-line summary; add examples for complex APIs |
| Commented-out code left in the source | Remove it; rely on version control |
| Inconsistent `use` import ordering | Group: `std` → external → internal → `self`/`super` |
| Magic numbers / string literals | Extract to named constants |
| Inconsistent `#[derive]` ordering | Standardize: `Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize` |
