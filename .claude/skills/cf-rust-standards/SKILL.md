---
name: cf-rust-standards
description: Rust development standards, patterns, and validation reference. Covers workspace structure, error handling, trait design, serde, async patterns, testing, naming, Clippy config, unsafe policy, and approved crates. Targets Rust 2024 edition.
---

# Rust Standards Skill

## Type

**Procedural** - On-demand reference for Rust development conventions and validation.

## Purpose

**Quick reference for Rust workspace structure, naming, error handling, trait design, async patterns, testing conventions, lint rules, and common patterns used in CodeFlow.**

## Responsibilities

- Define standard workspace and crate structure, module organization, and visibility conventions
- Document common patterns (error handling, traits, serde, builder pattern, async, string handling)
- Specify Clippy configuration, unsafe policy, and approved crate list
- Define testing standards and conventions (see `apply-testing`)
- NOT: Linting execution (cf-development SOPs run `cargo clippy`)

## Decision Tree

```text
Working with .rs file:
├── New crate or module?       → apply-structure
├── Editing existing?          → Check conventions
│   ├── Naming/visibility?     → apply-conventions
│   ├── Logic/patterns?        → apply-patterns
│   ├── Async code?            → apply-async
│   └── Writing tests?         → apply-testing
└── Before commit?             → validate-crate
```

## Operations

| # | Operation | Enforcement | Purpose |
|---|-----------|-------------|---------|
| 1 | apply-structure | ENF-L3 Advisory | Workspace layout, crate skeleton, module organization, visibility |
| 2 | apply-conventions | ENF-L3 Advisory | Naming, Clippy config, unsafe policy |
| 3 | apply-patterns | ENF-L3 Advisory | Trait design, builder pattern, serde, borrow/clone, lifetimes, heap allocation, Cow |
| 4 | apply-async | ENF-L3 Advisory | Tokio runtime, async vs sync decision tree |
| 5 | apply-testing | ENF-L3 Advisory | Test conventions, proptest, insta, cargo-nextest |
| 6 | validate-crate | ENF-L3 Advisory | Clippy, format, approved crate list, coverage |

## Operation Details

### apply-structure

```text
When: Creating a new crate, module, or reviewing workspace skeleton
Purpose: Ensure correct workspace layout, crate type, module organization, and visibility rules
Enforcement: ENF-L3 Advisory

Workspace Layout:

  codeflow/                        # Cargo workspace root
  ├── Cargo.toml                   # [workspace] definition, shared dependencies
  ├── Cargo.lock                   # Committed for binaries, gitignored for libs
  ├── codeflow-core/               # Library crate (codeflow-core lib)
  │   ├── Cargo.toml               # lib crate: [lib], crate-type = ["rlib"]
  │   └── src/
  │       ├── lib.rs               # Crate root: pub use, module declarations
  │       ├── error.rs             # Crate-level error types (thiserror)
  │       └── {domain}/            # Domain sub-modules
  │           ├── mod.rs           # Module root: pub(crate) use, sub-module declarations
  │           └── {component}.rs   # Component implementation
  └── codeflow-cli/                # Binary crate (codeflow-cli bin)
      ├── Cargo.toml               # bin crate: [[bin]], links codeflow-core
      └── src/
          ├── main.rs              # Entry point: tokio::main or fn main()
          ├── error.rs             # CLI error types (anyhow)
          └── cmd/                 # Command modules
              └── {command}.rs     # Command implementation

Module Organization Rules:

  | Rule | Description |
  |------|-------------|
  | mod.rs for directories | Use mod.rs (not lib.rs) for non-crate-root modules |
  | Flat over nested | Prefer flat module trees; nest only when grouping is semantically meaningful |
  | One type per file | Major types (structs, enums, traits) get their own file when complex |
  | Private by default | All items private unless explicitly exposed |
  | pub(crate) default | Internal sharing uses pub(crate), not pub |
  | pub only for APIs | pub visibility only for stable, documented public trait APIs |

Visibility Rules:

  pub(crate)  // Default for cross-module sharing within a crate
  pub(super)  // Sharing only with parent module
  pub         // Only for items that form a stable public API (trait definitions, key types)

  // Good: pub(crate) for internal utility
  pub(crate) fn parse_session_id(s: &str) -> Option<SessionId> { ... }

  // Good: pub for trait that external consumers implement
  pub trait Store {
      fn get(&self, id: &str) -> Result<Option<Record>>;
  }

  // Bad: pub for internal helper
  pub fn internal_helper() { ... }   // Should be pub(crate)

Workspace Cargo.toml Pattern:

  [workspace]
  members = ["codeflow-core", "codeflow-cli"]
  resolver = "2"

  [workspace.package]
  edition = "2024"
  rust-version = "1.85"

  [workspace.dependencies]
  # Pin exact versions; update deliberately
  serde = { version = "1", features = ["derive"] }
  tokio = { version = "1", features = ["full"] }
  anyhow = "1"
  thiserror = "2"

Procedure:
  1. Create workspace Cargo.toml with [workspace] and [workspace.dependencies]
  2. Create codeflow-core/Cargo.toml as lib crate with edition = "2024"
  3. Create codeflow-cli/Cargo.toml as bin crate linking codeflow-core
  4. Create src/lib.rs (core) or src/main.rs (cli) as crate roots
  5. Create src/error.rs for crate-level error types
  6. Apply visibility rules: pub(crate) by default, pub only for stable APIs
  7. Verify workspace builds: cargo build --workspace

Output: Workspace with correct layout, visibility, and build configuration
```

### apply-conventions

```text
When: Writing or editing Rust code — naming, Clippy configuration, or unsafe usage
Purpose: Ensure consistent naming, Clippy compliance, and safe code practices
Enforcement: ENF-L3 Advisory

Naming Conventions:

  | Element | Convention | Example |
  |---------|-----------|---------|
  | Modules | snake_case | session_manager, task_store |
  | Files | snake_case.rs | session_manager.rs |
  | Structs / Enums / Traits | CamelCase | SessionManager, TaskStore, WorkGraph |
  | Type aliases | CamelCase | SessionId, TaskResult |
  | Functions / methods | snake_case | open_database(), build_config() |
  | Variables / fields | snake_case | session_id, task_count |
  | Constants | SCREAMING_SNAKE_CASE | MAX_RETRIES, DEFAULT_TIMEOUT |
  | Statics | SCREAMING_SNAKE_CASE | GLOBAL_CONFIG |
  | Lifetimes | 'lowercase | 'a, 'session, 'input |
  | Generic params | CamelCase (short) | T, E, S, Store |
  | Feature flags | snake_case | surrealdb-backend |
  | Test functions | snake_case with descriptive name | test_open_database_returns_error_on_missing_file |

Clippy Configuration:

  // In lib.rs or main.rs (crate root only):
  #![deny(clippy::all)]         // All correctness and style lints as errors
  #![warn(clippy::pedantic)]    // Pedantic lints as warnings (not errors)
  #![allow(clippy::module_name_repetitions)]  // Allow SessionSession-style when needed

  // Per-function or per-block suppression (use sparingly, always with reason):
  #[allow(clippy::too_many_arguments)]  // Only when refactor is not feasible
  fn complex_function(/* 7 params */) { ... }

  Clippy groups and their enforcement levels:
    deny(clippy::all)      → All lints in correctness, style, complexity, perf groups
    warn(clippy::pedantic) → Pedantic lints: stricter naming, exhaustive patterns, etc.

Unsafe Policy:

  FORBIDDEN by default. The unsafe keyword MUST NOT appear in any new code unless:
    1. Explicitly approved by team lead (Tier 3 decision)
    2. Justification documented in a comment block immediately above the unsafe block
    3. All safety invariants documented in a SAFETY comment

  // Required format for any approved unsafe block:
  //
  // SAFETY: {specific invariants that make this sound}
  // APPROVED: {reason unsafe is necessary, alternatives considered}
  // REVIEW: {date and reviewer}
  unsafe {
      // ... unsafe operations
  }

  Common safe alternatives to unsafe:
    | Unsafe Pattern | Safe Alternative |
    |----------------|-----------------|
    | Raw pointer manipulation | Use Arc<T>, Box<T>, or safe wrappers |
    | FFI without bindgen | Use bindgen-generated safe wrappers |
    | Manual memory layout | Use repr(C) structs with bytemuck |
    | Mutable statics | Use OnceLock<T> or LazyLock<T> |
    | transmute for casting | Use From/Into or bytemuck::cast |

Procedure:
  1. Apply naming convention from table for each element
  2. Add #![deny(clippy::all)] and #![warn(clippy::pedantic)] to crate root
  3. Run cargo clippy --workspace -- -D warnings to verify
  4. If unsafe is present, verify SAFETY and APPROVED comments exist
  5. Replace unsafe patterns with safe alternatives where possible

Output: Code following consistent naming, Clippy compliance, and safe code practices
```

### apply-patterns

```text
When: Implementing logic — error handling, traits, serde, builder pattern, borrow/clone, lifetimes, heap allocation, string handling
Purpose: Use correct patterns and avoid known anti-patterns
Enforcement: ENF-L3 Advisory

Error Handling:

  Library crate (codeflow-core) → thiserror:
    use thiserror::Error;

    #[derive(Debug, Error)]
    pub enum StoreError {
        #[error("record not found: {0}")]
        NotFound(String),
        #[error("database error: {0}")]
        Database(#[from] surrealdb::Error),
        #[error("serialization error: {0}")]
        Serialization(#[from] serde_json::Error),
    }

  CLI binary (codeflow-cli) → anyhow:
    use anyhow::{Context, Result};

    fn load_config(path: &Path) -> Result<Config> {
        let content = fs::read_to_string(path)
            .with_context(|| format!("reading config from {}", path.display()))?;
        serde_json::from_str(&content)
            .with_context(|| format!("parsing config from {}", path.display()))
    }

  Error message style (same as Go):
    - Lowercase, no trailing punctuation
    - Include context: "reading config from {path}"
    - Chain from inner to outer via thiserror's #[from] or anyhow's .context()

Trait Design Patterns:

  Decision tree for trait objects vs generics:

    Does the concrete type need to be chosen at runtime?
    ├── YES → Use trait objects: Box<dyn Trait> or Arc<dyn Trait>
    └── NO → Does the function/struct need to work with multiple types?
        ├── YES → Use generics: fn foo<S: Store>(store: S) or fn foo<S>(store: S) where S: Store
        └── NO → Use concrete type directly

  Trait object pattern (runtime dispatch):
    // Good: trait object for runtime-swappable storage
    pub trait Store: Send + Sync {
        fn get(&self, id: &str) -> Result<Option<Record>, StoreError>;
        fn put(&self, record: Record) -> Result<(), StoreError>;
    }
    pub struct WorkGraph {
        store: Arc<dyn Store>,
    }

  Generic pattern (compile-time dispatch):
    // Good: generic when type is fixed at call site
    pub fn process<S: Store>(store: &S, id: &str) -> Result<(), StoreError> {
        let record = store.get(id)?.ok_or_else(|| StoreError::NotFound(id.to_string()))?;
        // ...
    }

  Trait design rules:
    - Keep traits focused (1-5 methods is a good target)
    - Add Send + Sync bounds when the trait will be used with async or Arc<dyn Trait>
    - Define traits where they are consumed, not where implementations live
    - Prefer associated types over generic parameters when the type is determined by Self

Serde Serialization Conventions:

  Derive macros (preferred):
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Serialize, Deserialize)]
    pub struct Task {
        pub id: String,
        pub title: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub description: Option<String>,
        #[serde(default)]
        pub tags: Vec<String>,
    }

  Field renaming:
    #[derive(Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]  // For JSON APIs expecting camelCase
    pub struct ApiResponse {
        pub task_id: String,     // serializes as "taskId"
        pub created_at: String,  // serializes as "createdAt"
    }

  Custom serialization (when derive is insufficient):
    use serde::{Serializer, Deserializer};

    impl Serialize for SessionId {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            serializer.serialize_str(&self.0)
        }
    }

  Anti-patterns:
    | Anti-Pattern | Correct Alternative |
    |--------------|---------------------|
    | #[serde(skip)] on required fields | Use Option<T> with #[serde(skip_serializing_if)] |
    | Custom serializer for simple types | Use #[serde(with = "...")] or newtype wrapper |
    | serde_json::Value for typed data | Define proper structs with Serialize/Deserialize |

Builder Pattern:

  Use when a struct has more than 3 optional fields or complex construction logic.

  // Typed builder (preferred when all fields are known at compile time):
  pub struct SessionBuilder {
      id: Option<String>,
      name: Option<String>,
      tags: Vec<String>,
  }

  impl SessionBuilder {
      pub fn new() -> Self {
          Self { id: None, name: None, tags: Vec::new() }
      }
      pub fn id(mut self, id: impl Into<String>) -> Self {
          self.id = Some(id.into()); self
      }
      pub fn name(mut self, name: impl Into<String>) -> Self {
          self.name = Some(name.into()); self
      }
      pub fn tag(mut self, tag: impl Into<String>) -> Self {
          self.tags.push(tag.into()); self
      }
      pub fn build(self) -> Result<Session, BuildError> {
          Ok(Session {
              id: self.id.ok_or(BuildError::MissingId)?,
              name: self.name.unwrap_or_default(),
              tags: self.tags,
          })
      }
  }

  // Usage:
  let session = SessionBuilder::new()
      .id("ses-001")
      .name("my-session")
      .tag("feat")
      .build()?;

Borrow vs Clone Guidance:

  Prefer &T over T.clone() when the lifetime allows:

    // Good: borrow when the reference outlives the function call
    fn process_name(name: &str) -> usize { name.len() }

    // Bad: unnecessary clone
    fn process_name(name: String) -> usize { name.len() }  // clone at call site

    // Good: take ownership when the function needs to store or transform
    fn store_name(name: String) { self.names.push(name); }

    // Guideline by method type:
    | Situation | Parameter Type | Reason |
    |-----------|---------------|--------|
    | Read-only access | &T or &str | Borrow, no copy needed |
    | May need owned later | impl Into<T> | Caller decides |
    | Function stores the value | T (owned) | Take ownership directly |
    | Shared across threads | Arc<T> | Reference-counted shared ownership |
    | Mutating in-place | &mut T | Mutable borrow |

Lifetime Annotations in Trait Definitions:

  Lifetimes are required when a trait returns references or stores references:

    // Return reference tied to self
    pub trait Named {
        fn name(&self) -> &str;  // Elided: 'self tied to return
    }

    // Explicit lifetime for complex cases
    pub trait Parser<'input> {
        fn parse(&self, input: &'input str) -> Result<Token<'input>, ParseError>;
    }

    // Lifetime bounds on associated types
    pub trait Store {
        type Record<'a>: Deserialize<'a> where Self: 'a;
        fn get<'a>(&'a self, id: &str) -> Option<Self::Record<'a>>;
    }

  Rules:
    - Elide lifetimes when the compiler can infer them unambiguously
    - Use named lifetimes when a reference in the return depends on a specific input
    - Prefer 'a, 'b for generic lifetimes; use descriptive names ('input, 'session) for complex cases
    - Avoid 'static unless you truly need the value to live for the entire program

Heap Allocation Avoidance:

  Prefer stack-allocated types; avoid unnecessary Box<T> and Vec<T>:

    | Situation | Prefer | Avoid |
    |-----------|--------|-------|
    | Fixed-size collection | [T; N] | Vec<T> |
    | Small string (< 32 bytes) | &str or SmallStr | String |
    | Single known concrete type | T | Box<T> |
    | Error propagation | Result<T, E> | Box<dyn Error> |
    | Optional value | Option<T> | Box<Option<T>> |
    | Recursive types | Box<T> (required) | Unbounded recursion |

    // Good: stack-allocated fixed array
    let status_codes: [u8; 4] = [200, 201, 400, 404];

    // Bad: heap-allocated when size is fixed
    let status_codes: Vec<u8> = vec![200, 201, 400, 404];

    // Good: pass-through string without allocation
    fn get_label(&self) -> &str { &self.label }

    // Bad: allocates on every call
    fn get_label(&self) -> String { self.label.clone() }

  When Box<T> IS appropriate:
    - Recursive type definitions (e.g., tree nodes, linked lists)
    - Large structs on the stack that cause stack overflow
    - Trait objects: Box<dyn Trait> for dynamic dispatch
    - Owned data returned from functions where the size is not known at compile time

Cow<'a, str> for Flexible String Handling:

  Use Cow<'a, str> when a function may return either a borrowed or owned string:

    use std::borrow::Cow;

    // Good: returns borrowed when no transformation needed, owned otherwise
    fn normalize_id<'a>(id: &'a str) -> Cow<'a, str> {
        if id.starts_with("ses-") {
            Cow::Borrowed(id)            // Zero allocation when already normalized
        } else {
            Cow::Owned(format!("ses-{}", id))  // Allocates only when needed
        }
    }

    // Good: function accepting both &str and String
    fn log_event(msg: impl Into<Cow<'static, str>>) {
        let msg = msg.into();
        // ...
    }

  Use cases:
    | Scenario | Use Cow? | Why |
    |----------|----------|-----|
    | May or may not transform input string | Yes | Avoid unconditional allocation |
    | Always returns owned String | No | Use String directly |
    | Always returns borrowed &str | No | Use &str with lifetime |
    | String literal or transformed string | Yes | Cow<'static, str> |

Common Pattern Anti-patterns:

  | Anti-Pattern | Correct Alternative |
  |--------------|---------------------|
  | .clone() on every string access | Borrow with &str or &String |
  | Box<dyn Error> in library APIs | Define thiserror enum |
  | unwrap() in non-test code | ? operator or explicit match |
  | expect() without informative message | expect("context: reason for panic") |
  | Returning Vec when slice suffices | Return &[T] or impl AsRef<[T]> |
  | String::new() + push_str in loop | Use write! on String with fmt::Write |
  | Nested match for Option chaining | Use .map(), .and_then(), .unwrap_or() |
  | Struct with pub fields for config | Builder pattern + private fields |

Procedure:
  1. Check error handling: thiserror for codeflow-core, anyhow for codeflow-cli
  2. Determine trait vs generic using the decision tree
  3. Verify serde: prefer derive macros, add skip_serializing_if for Option fields
  4. Check borrow vs clone guidance for all function parameters
  5. Check heap allocation: avoid Box/Vec when stack allocation is feasible
  6. Use Cow<'a, str> when return may be borrowed or owned depending on input
  7. Replace anti-patterns with correct alternatives

Output: Code using correct patterns with no anti-patterns present
```

### apply-async

```text
When: Working with async functions, tokio runtime, or concurrent operations
Purpose: Ensure correct async patterns, tokio usage, and sync vs async decision tree
Enforcement: ENF-L3 Advisory

Async vs Sync Decision Tree:

  Should this function be async?
  ├── Does it perform I/O (network, disk, database)? → YES → async fn
  ├── Does it call other async functions? → YES → async fn
  ├── Is it CPU-bound computation? → NO → sync fn (offload with spawn_blocking if needed)
  └── Is it a pure transformation? → NO → sync fn

  Rule: Async functions propagate — one async call makes the caller async.
  Keep the "sync core" pattern: pure logic in sync fns, I/O at the boundary.

Tokio Runtime Setup:

  // CLI binary (main.rs): multi-threaded runtime (default)
  #[tokio::main]
  async fn main() -> anyhow::Result<()> {
      run().await
  }

  // CLI binary with explicit runtime config:
  fn main() -> anyhow::Result<()> {
      tokio::runtime::Builder::new_multi_thread()
          .worker_threads(4)
          .enable_all()
          .build()?
          .block_on(run())
  }

  // Library tests: use #[tokio::test]
  #[tokio::test]
  async fn test_store_roundtrip() {
      // test body
  }

  // Library: never create a runtime internally — accept &Runtime or be called from async context
  // BAD: library creates its own runtime
  pub fn fetch_sync(url: &str) -> Result<Response> {
      tokio::runtime::Runtime::new()?.block_on(fetch_async(url))  // BAD
  }

Async Task Patterns:

  // Spawn concurrent tasks with JoinSet (tokio 1.x):
  use tokio::task::JoinSet;

  let mut set = JoinSet::new();
  for item in items {
      set.spawn(async move { process(item).await });
  }
  while let Some(result) = set.join_next().await {
      result??;  // propagate panics and errors
  }

  // Timeout on async operations:
  use tokio::time::{timeout, Duration};

  let result = timeout(Duration::from_secs(30), fetch_data(url))
      .await
      .map_err(|_| anyhow::anyhow!("fetch timed out after 30s"))??;

  // Cancellation via select!:
  use tokio::select;

  select! {
      result = do_work() => result?,
      _ = shutdown_signal() => {
          tracing::info!("shutting down");
          return Ok(());
      }
  }

  // Channel patterns:
  use tokio::sync::{mpsc, oneshot};

  // mpsc for streaming many items
  let (tx, mut rx) = mpsc::channel::<Event>(32);

  // oneshot for single response
  let (tx, rx) = oneshot::channel::<Result<Response>>();

Blocking Code in Async Context:

  // Offload CPU-bound work to the blocking thread pool:
  let result = tokio::task::spawn_blocking(|| {
      expensive_cpu_computation()
  }).await?;

  // File I/O: prefer tokio::fs over std::fs in async functions
  use tokio::fs;
  let content = fs::read_to_string(path).await?;

Async Anti-patterns:

  | Anti-Pattern | Correct Alternative |
  |--------------|---------------------|
  | std::thread::sleep in async fn | tokio::time::sleep(...).await |
  | std::fs in async fn | tokio::fs async equivalents |
  | block_on inside async fn | .await directly |
  | Arc<Mutex<T>> blocking in async | tokio::sync::Mutex for async-safe locking |
  | Unbounded JoinSet/spawn | Bound concurrency with semaphore or JoinSet::len() check |
  | Returning impl Future without Send | Use async fn or ensure Send bound for multi-thread runtime |

Procedure:
  1. Apply async vs sync decision tree to determine if function should be async
  2. Use #[tokio::main] for CLI binary entry point
  3. Use #[tokio::test] for async tests
  4. Prefer JoinSet for concurrent task spawning
  5. Use tokio::time::timeout for operations with time bounds
  6. Replace std::fs and std::thread::sleep with tokio equivalents in async functions
  7. Offload CPU-bound work with spawn_blocking

Output: Async code with correct tokio usage, bounded concurrency, and no sync/async anti-patterns
```

### apply-testing

```text
When: Writing or editing test files, setting coverage targets, or reviewing test quality
Purpose: Apply consistent Rust testing conventions — file layout, naming, test attributes, proptest, insta, cargo-nextest
Enforcement: ENF-L3 Advisory

Test Placement (codeflow-rs convention):

  Unit tests MUST be inline in the same file as the code under test, using
  #[cfg(test)] mod tests { } at the bottom of the file. Separate test files
  (tests.rs, test_*.rs in the module directory) are FORBIDDEN. This matches
  the established convention across all modules in codeflow-core and codeflow-cli.

  // Required pattern — inline at bottom of source file:
  #[cfg(test)]
  mod tests {
      use super::*;

      #[tokio::test]
      async fn test_create_task() {
          let (service, _) = new_service();
          let task = service.create_task(default_task_input(epic_id)).await.unwrap();
          assert!(task.id.as_str().starts_with("task-"));
          assert_eq!(task.status, TaskStatus::Todo);
          assert_eq!(task.title, "Test Task");  // verify specific field values
      }
  }

  // FORBIDDEN — separate file in the module directory:
  // src/workgraph/tests.rs
  // src/workgraph/task_tests.rs

Test Helpers (shared fixtures):

  Shared fixtures (MockStore, MockLedger, default inputs) go in
  #[cfg(test)] pub(crate) mod test_support in the module root (mod.rs)
  for cross-file reuse within the module.

  // Pattern from codeflow-core/src/workgraph/mod.rs:
  #[cfg(test)]
  pub(crate) mod test_support {
      use crate::store::mock::MockStore;

      pub struct MockLedger { ... }
      impl LedgerWriter for MockLedger { ... }

      pub fn new_service() -> (WorkgraphService<MockStore, MockLedger>, ...) { ... }
      pub fn default_epic_input() -> CreateEpicInput { ... }
      pub fn default_task_input(epic_id: EpicId) -> CreateTaskInput { ... }
  }

  // In submodule tests, import from parent test_support:
  use super::super::test_support::{default_epic_input, new_service};

  MockStore is defined at codeflow-core/src/store/mod.rs under #[cfg(test)] pub mod mock.
  MockLedger is defined per-module in the module root's test_support block.

File Organization:

  Unit tests: in the same file as the code under test, in a #[cfg(test)] module
  Integration tests: in tests/ directory at crate root
  Test helpers and fixtures: in #[cfg(test)] pub(crate) mod test_support in mod.rs

  // Integration test file: tests/store_integration.rs
  // (has access to the crate's public API only)

Test Quality (no fluff tests):

  Every test MUST have at least one assertion that would FAIL if the feature
  under test broke. Tests that only verify "doesn't panic" or check trivial
  properties are not acceptable.

  FORBIDDEN (insufficient assertions):
    assert!(result.is_ok())                         // does not verify the value
    assert!(!tasks.is_empty())                      // does not verify count or content
    assert!(task.id.as_str().starts_with("task-"))  // alone is insufficient

  REQUIRED (specific value assertions):
    assert_eq!(task.status, TaskStatus::Todo);            // verify specific field
    assert_eq!(task.format_id.as_str(), "INF-TSK-001-001"); // verify exact value
    assert_eq!(evts[1].event_type, "task_created");        // verify event emitted

  Filter and query tests MUST verify inclusion AND exclusion:
    // FORBIDDEN — only checks one side:
    let tasks = service.tasks_by_epic(&epic1.id).await.unwrap();
    assert!(!tasks.is_empty());

    // REQUIRED — verify correct records included, wrong records excluded:
    let tasks = service.tasks_by_epic(&epic1.id).await.unwrap();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].epic_id, epic1.id);    // correct epic included
    // verify epic2's task is NOT in the result by checking count is exactly 1

  Round-trip tests MUST verify all significant fields:
    // FORBIDDEN — only checks ID and one field:
    assert_eq!(fetched.id, created.id);
    assert_eq!(fetched.title, "Test Task");

    // REQUIRED — also verify other significant fields:
    assert_eq!(fetched.id, created.id);
    assert_eq!(fetched.title, "Test Task");
    assert_eq!(fetched.status, TaskStatus::Todo);
    assert_eq!(fetched.epic_id, created.epic_id);

Required Test Categories for CRUD Modules:

  When implementing a CRUD module (task, epic, session, etc.), provide tests
  for ALL of these categories:

  Create:
    - Happy path: all fields verified (not just id)
    - Validation error: empty required fields return WorkgraphError::Validation
    - Constraint violation: FK reference to nonexistent parent returns NotFound

  Read:
    - By ID: happy path, fields verified
    - By alternative key (format_id): happy path
    - Not found: returns WorkgraphError::NotFound

  Update:
    - Field changes: verify updated value persists on subsequent read
    - Nonexistent entity: returns WorkgraphError::NotFound
    - Invalid transitions: returns WorkgraphError::InvalidTransition

  List:
    - Filtered results: verify inclusion AND exclusion
    - Empty results: filter that matches nothing returns empty vec
    - Default filter (no criteria): returns all records

  State transitions (state machine modules):
    - All valid transitions: each valid (from, to) pair is tested
    - All invalid transitions: representative invalid pairs tested
    - ALL terminal states: every terminal state rejects ALL outgoing transitions
    - ALL self-transitions: every status rejects self-transition

  Example (from codeflow-core/src/workgraph/transitions.rs):
    #[test]
    fn test_terminal_task_states() {
        // Complete is terminal — ALL outgoing transitions rejected
        assert!(validate_task_transition(TaskStatus::Complete, TaskStatus::Todo).is_err());
        assert!(validate_task_transition(TaskStatus::Complete, TaskStatus::InProgress).is_err());
        assert!(validate_task_transition(TaskStatus::Complete, TaskStatus::Blocked).is_err());
        assert!(validate_task_transition(TaskStatus::Complete, TaskStatus::Cancelled).is_err());
        // Cancelled is terminal — ALL outgoing transitions rejected
        assert!(validate_task_transition(TaskStatus::Cancelled, TaskStatus::Todo).is_err());
        assert!(validate_task_transition(TaskStatus::Cancelled, TaskStatus::InProgress).is_err());
        assert!(validate_task_transition(TaskStatus::Cancelled, TaskStatus::Blocked).is_err());
        assert!(validate_task_transition(TaskStatus::Cancelled, TaskStatus::Complete).is_err());
    }

Test Function Naming:

  | Element | Convention | Example |
  |---------|-----------|---------|
  | Unit test | test_{function}_{scenario} | test_parse_id_valid_format |
  | Integration test | test_{integration_scenario} | test_store_roundtrip |
  | Async test | test_{function}_{scenario} (same) | test_fetch_returns_404 |
  | Proptest | proptest_{property_name} | proptest_serialize_roundtrip |
  | Snapshot test | test_{component}_{scenario}_snapshot | test_render_output_snapshot |

Test Attributes:

  #[test]            // Standard synchronous test
  #[tokio::test]     // Async test with tokio runtime
  #[ignore]          // Skip test (add reason comment: // TODO: flaky, fix in issue #...)
  #[should_panic]    // Test that must panic (add expected = "..." when possible)

  // Async test with explicit runtime config:
  #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
  async fn test_concurrent_writes() { ... }

Property-Based Testing (proptest):

  Use proptest for functions with invariants that hold across many input values.
  Do NOT replace all unit tests with proptest — use it to complement, not replace.

  use proptest::prelude::*;

  proptest! {
      #[test]
      fn proptest_serialize_roundtrip(id in "[a-z0-9]{8,32}") {
          let task = Task { id: id.clone(), ..Default::default() };
          let json = serde_json::to_string(&task).unwrap();
          let parsed: Task = serde_json::from_str(&json).unwrap();
          prop_assert_eq!(task.id, parsed.id);
      }
  }

  When to use proptest:
    - Serialization/deserialization roundtrips
    - String parsing functions (any valid input should parse without panic)
    - Mathematical invariants (commutativity, associativity)
    - Functions where the output type determines correctness

Snapshot Testing (insta):

  Use insta for testing complex structured output (CLI output, JSON responses, formatted strings).

  use insta::assert_snapshot;

  #[test]
  fn test_format_task_output() {
      let task = Task { id: "t-001".to_string(), title: "Do the thing".to_string() };
      assert_snapshot!(format_task(&task));
      // First run creates snapshot in snapshots/ directory
      // Subsequent runs compare against stored snapshot
      // Use `cargo insta review` to accept/reject new snapshots
  }

  // JSON snapshot (preserves structure):
  use insta::assert_json_snapshot;
  assert_json_snapshot!(response_struct);

  Rules:
    - Commit snapshot files in snapshots/ alongside test files
    - Run `cargo insta review` to review and accept new/changed snapshots
    - Never manually edit snapshot files — use `cargo insta review`

Running Tests with cargo-nextest:

  cargo nextest run                    # Run all tests (faster than cargo test)
  cargo nextest run --test-threads=1   # Serial execution (for tests with shared state)
  cargo nextest run -p codeflow-core   # Run single crate's tests
  cargo llvm-cov nextest               # Coverage with cargo-llvm-cov

  cargo test is still valid but cargo-nextest is preferred:
    - Better output formatting (per-test timing, color)
    - Parallel test isolation (each test in its own process)
    - Retry support for flaky tests: --retries 2

Coverage:

  Use cargo-llvm-cov for line and branch coverage:
    cargo llvm-cov --workspace --lcov --output-path lcov.info
    cargo llvm-cov nextest --workspace  # Coverage + nextest in one step

  Coverage thresholds configured in `codeflow-cli/config/testing/test-config.json`:
    - `file_threshold`: per-file default (85%)
    - `crate_threshold`: per-crate default (85%)
    - `crate_overrides`: per-crate exceptions (e.g., codeflow-cli: 80%)
    - `conventions.exceptions`: per-file exceptions with documented justification

Anti-Patterns (DO NOT):

  | Anti-Pattern | Correct Alternative |
  |--------------|---------------------|
  | unwrap() in test assertions | Use assert!/assert_eq! or ? in #[tokio::test] |
  | std::thread::sleep in tests | Use tokio::time::pause()/advance() for time control |
  | Global mutable state in tests | Use test-local setup; prefer #[cfg(test)] mod tests |
  | Hardcoded file paths in tests | Use tempfile::TempDir or env::temp_dir() |
  | Skipping cleanup (temp files) | Use Drop-based cleanup: tempfile::TempDir auto-cleans |
  | Testing private functions directly | Test through public API; refactor if private logic is complex |
  | Single mega-test function | Split into focused tests per scenario |
  | Separate test files (task_tests.rs) | Inline #[cfg(test)] mod tests in the source file |
  | assert!(!vec.is_empty()) alone | assert_eq!(vec.len(), N) + field-level assertions |
  | Shared fixtures in a test submodule | test_support in module root (mod.rs) |
  | Terminal state only partially tested | Test ALL outgoing transitions from each terminal state |

Procedure:
  1. Place unit tests inline in #[cfg(test)] mod tests at bottom of the source file
     (NEVER in a separate *_tests.rs or tests.rs file in the module directory)
  2. Use #[tokio::test] for async tests
  3. Name tests test_{function}_{scenario}
  4. For CRUD modules: cover create, read, update, list, and transition categories
  5. For state machines: test ALL valid, invalid, terminal, and self-transitions
  6. For filter/query tests: verify inclusion AND exclusion
  7. Add proptest for serialization roundtrips and parsing functions
  8. Use insta for complex output snapshot tests
  9. Place shared fixtures in #[cfg(test)] pub(crate) mod test_support in mod.rs
  10. Run with cargo nextest run for faster feedback
  11. Verify coverage meets thresholds: cargo llvm-cov nextest --workspace

Output: Test files following consistent naming, correct attributes, inline placement, specific assertions, complete CRUD and transition coverage, and coverage thresholds
```

### validate-crate

```text
When: Before committing Rust code or during pre-commit checks
Purpose: Run Clippy, format, and verify approved crate list compliance
Enforcement: ENF-L3 Advisory

Lint / Format Commands:

  | Command | Usage |
  |---------|-------|
  | cargo fmt --all | Format all files (mandatory) |
  | cargo clippy --workspace -- -D warnings | All lints as errors |
  | cargo clippy --workspace -- -D warnings -W clippy::pedantic | Pedantic lints as warnings |
  | cargo test --workspace | Run all tests |
  | cargo nextest run --workspace | Run tests (preferred, faster) |
  | cargo llvm-cov nextest --workspace | Coverage (preferred) |
  | cargo build --workspace | Verify builds clean |

Approved Crate List:

  These 14 crates are approved for use in CodeFlow Rust code.
  Adding a new crate requires a Tier 3 decision (ADR via cf-planning).

  | Crate | Category | Purpose |
  |-------|----------|---------|
  | clap | CLI | Argument parsing (use derive feature) |
  | serde | Serialization | Derive-based serialization framework |
  | serde_json | Serialization | JSON serialization/deserialization |
  | thiserror | Error handling | Library error type derivation |
  | anyhow | Error handling | CLI/binary error propagation with context |
  | surrealdb | Database | Embedded or remote SurrealDB client |
  | loro | CRDT | Conflict-free replicated data types for document sync |
  | git2 | VCS | libgit2 bindings for git operations |
  | tokio | Async runtime | Multi-threaded async runtime (features = ["full"]) |
  | proptest | Testing | Property-based testing (dev-dependency only) |
  | insta | Testing | Snapshot testing (dev-dependency only) |
  | cargo-nextest | Testing | Test runner (installed tool, not Cargo dep) |
  | cargo-llvm-cov | Coverage | LLVM-based coverage (installed tool, not Cargo dep) |
  | ort | ML inference | ONNX Runtime bindings for model inference |
  | fs2 | File locking | Cross-platform flock for file-level advisory locks |

  cargo-nextest and cargo-llvm-cov are installed tools (cargo install), NOT Cargo.toml dependencies.
  proptest and insta are dev-dependencies: under [dev-dependencies] in Cargo.toml.

  Cargo.toml example:
    [dependencies]
    clap = { version = "4", features = ["derive"] }
    serde = { version = "1", features = ["derive"] }
    serde_json = "1"
    thiserror = "2"
    anyhow = "1"
    tokio = { version = "1", features = ["full"] }

    [dev-dependencies]
    proptest = "1"
    insta = { version = "1", features = ["json"] }

Build Verification:

  cargo build --workspace                         # Must succeed
  cargo build --workspace --release               # Verify release build
  RUSTFLAGS="-D warnings" cargo build --workspace # Treat warnings as errors

Rust Edition and MSRV:

  edition = "2024"           # Rust 2024 edition (in Cargo.toml)
  rust-version = "1.85"      # Minimum Supported Rust Version (MSRV)

  Prefer Rust 1.80+ features:
    | Feature | Version | Use Case |
    |---------|---------|---------|
    | LazyLock<T> | 1.80 | Lazy static initialization (replaces once_cell) |
    | impl Trait in type aliases | 1.79 | Cleaner return types |
    | Pattern matching on &str | 1.0+ | match s.as_str() { "a" => ... } |
    | let-else | 1.65 | Early return from Option/Result |
    | ? in main / async fn | 1.65 | Error propagation from main |
    | async fn in traits | 1.75 | Direct async trait methods (no workaround needed) |

Procedure:
  1. Run cargo fmt --all to format all files
  2. Run cargo clippy --workspace -- -D warnings to check for lint errors
  3. Verify Cargo.toml only includes approved crates (see list above)
  4. Run cargo nextest run --workspace to verify all tests pass
  5. Run cargo llvm-cov nextest --workspace to check coverage thresholds
  6. Run cargo build --workspace to verify clean build
  7. Fix all issues before commit

Output: Clean format, lint, build, and test output with approved crates only
```
