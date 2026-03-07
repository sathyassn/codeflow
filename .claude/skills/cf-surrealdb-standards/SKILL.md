---
name: cf-surrealdb-standards
description: SurrealDB development standards, patterns, and validation reference. Covers embedded vs daemon mode selection, SurrealQL conventions, graph relation patterns, vector indices, schema migration, connection management, testing patterns, and data model conventions. Targets SurrealDB 2.x with the surrealdb Rust crate.
---

# SurrealDB Standards Skill

## Type

**Procedural** - On-demand reference for SurrealDB development conventions and validation.

## Purpose

**Quick reference for SurrealDB connection modes, SurrealQL schema definitions, graph traversal patterns, vector index setup, schema migration strategy, connection lifecycle, testing patterns, and per-record data model conventions used in CodeFlow.**

## Responsibilities

- Define when to use embedded (`surrealkv://`) vs daemon (`ws+unix://`) mode
- Document SurrealQL conventions for table, field, and index definitions
- Specify graph relation patterns and traversal syntax
- Document vector index patterns for semantic search (HNSW)
- Define idempotent schema migration strategy (no separate migration files)
- Specify connection management rules (single connection per process)
- Define testing patterns (in-memory `Surreal::new::<Mem>()` for unit tests)
- Document data model conventions (`project` field, standard fields, record ID format)
- Document explicit anti-pattern: no direct SurrealDB MCP connection to project DB
- NOT: Running database operations (cf-development SOPs execute DB calls via the Rust crate)

## Decision Tree

```text
Working with SurrealDB:
├── Choosing connection mode?       → apply-connection-mode
├── Defining schema?                → apply-surrealql-conventions
│   ├── Table or field definition?  → DEFINE TABLE / DEFINE FIELD patterns
│   ├── Index for lookups?          → DEFINE INDEX patterns
│   └── Graph relation?             → apply-graph-relations
├── Vector/embedding search?        → apply-vector-index
├── Schema changed?                 → apply-schema-migration
├── Opening DB connection?          → apply-connection-management
├── Writing tests?                  → apply-testing-patterns
├── Modeling records?               → apply-data-model
└── Using MCP with project DB?      → STOP: explicit anti-pattern (see apply-connection-mode)
```

## Operations

| # | Operation | Enforcement | Purpose |
|---|-----------|-------------|---------|
| 1 | apply-connection-mode | ENF-L3 Advisory | Embedded vs daemon mode selection, anti-pattern documentation |
| 2 | apply-surrealql-conventions | ENF-L3 Advisory | Table, field, index, and RELATE definitions |
| 3 | apply-graph-relations | ENF-L3 Advisory | TYPE RELATION tables, traversal syntax, graph query patterns |
| 4 | apply-vector-index | ENF-L3 Advisory | HNSW index definition, cosine similarity, embedding queries |
| 5 | apply-schema-migration | ENF-L3 Advisory | Idempotent DEFINE statements, startup application order |
| 6 | apply-connection-management | ENF-L3 Advisory | Single connection per process, init pattern, shared reference |
| 7 | apply-testing-patterns | ENF-L3 Advisory | In-memory DB for unit tests, file-backed for integration tests |
| 8 | apply-data-model | ENF-L3 Advisory | project field, standard fields, record ID format |

## Operation Details

### apply-connection-mode

```text
When: Deciding how to connect to SurrealDB — choosing between embedded, daemon, or MCP
Purpose: Select the correct connection mode for the deployment context
Enforcement: ENF-L3 Advisory

Decision Tree:

  Who accesses the DB?
  ├── Single process (CLI, tests, binary) → surrealkv:// (embedded mode, DEFAULT)
  ├── Multiple processes (future multi-agent) → ws+unix:// (daemon mode, RESERVED)
  └── MCP tool (Claude Code MCP server) → FORBIDDEN (explicit anti-pattern, see below)

Embedded Mode (surrealkv://) — CodeFlow Default:

  Use when a single Rust process owns the DB exclusively.
  The surrealdb crate opens the file directly — no separate surrealdb daemon needed.

  Connection pattern:
    use surrealdb::engine::local::SurrealKV;
    use surrealdb::Surreal;

    let db: Surreal<SurrealKV> = Surreal::new::<SurrealKV>("/path/to/codeflow.db").await?;
    db.use_ns("codeflow").use_db("main").await?;

  When to use:
    - codeflow-cli binary (single process, owns DB)
    - Integration tests using a temp-dir file-backed DB
    - Any scenario where only one process opens the DB at a time

  Limitations:
    - Only ONE process can open a SurrealKV file at a time (exclusive lock)
    - Do NOT use surrealkv:// if multiple processes need concurrent DB access

Daemon Mode (ws+unix://) — RESERVED for Future Use:

  Use when multiple processes need concurrent DB access.
  Requires running a separate `surreal start` daemon process.
  Connect via Unix socket or WebSocket.

  Connection pattern (future):
    use surrealdb::engine::remote::ws::Wss;
    use surrealdb::Surreal;

    let db: Surreal<_> = Surreal::new::<Wss>("ws+unix:///path/to/surrealdb.sock").await?;
    db.use_ns("codeflow").use_db("main").await?;

  When to use (future multi-process scenarios only):
    - Multiple codeflow-cli instances running concurrently
    - Agent processes needing shared DB access alongside the CLI

ANTI-PATTERN — No Direct SurrealDB MCP Connection to Project DB:

  ⛔ FORBIDDEN: Connecting an MCP server directly to the CodeFlow project SurrealDB instance.

  Reason: The project DB uses embedded surrealkv:// mode, which requires exclusive file lock
  by the owning process (codeflow-cli). An MCP server cannot open the same surrealkv:// file
  while the CLI holds the lock. Attempting this will either fail with a lock error or corrupt
  the database by violating SurrealKV's single-writer guarantee.

  The correct pattern:
    - ALL DB operations go through the codeflow Rust binary
    - MCP tools (Context7, rust-analyzer) provide code assistance, NOT DB access
    - Claude reads DB state by running codeflow CLI commands (e.g., codeflow task list)
    - There is no MCP server that exposes the project's SurrealDB to Claude Code

  The MCP servers configured for CodeFlow (Context7, rust-analyzer) serve these purposes:
    - Context7: Up-to-date crate documentation (surrealdb, tokio, serde, etc.)
    - rust-analyzer: Type info, trait resolution, diagnostic assistance
    Neither connects to nor queries the project SurrealDB instance.

Procedure:
  1. Identify how many processes will access the DB
  2. Single process → use surrealkv:// (embedded)
  3. Multiple processes → use ws+unix:// (daemon, requires surreal start)
  4. Never configure an MCP server to connect to the project's surrealkv:// DB
  5. Verify the path passed to SurrealKV::new is writable and not held by another process

Output: Correct connection mode with no lock conflicts or MCP anti-pattern violations
```

### apply-surrealql-conventions

```text
When: Defining tables, fields, indexes, or creating graph edges in SurrealQL
Purpose: Ensure consistent, type-safe schema definitions using SCHEMAFULL tables and idempotent DEFINE statements
Enforcement: ENF-L3 Advisory

Table Definitions (SCHEMAFULL):

  All tables use SCHEMAFULL to enforce strict schema — undefined fields are rejected.
  Use OVERWRITE for idempotent application (safe to re-run on startup).

    DEFINE TABLE task SCHEMAFULL OVERWRITE;
    DEFINE TABLE epic SCHEMAFULL OVERWRITE;
    DEFINE TABLE session SCHEMAFULL OVERWRITE;

  Never use SCHEMALESS for production tables — it allows arbitrary fields and
  bypasses type safety. SCHEMALESS is acceptable only for exploratory prototyping.

Field Definitions (DEFINE FIELD):

  Define each field with an explicit TYPE. Use OVERWRITE for idempotence.

    DEFINE FIELD id          ON TABLE task TYPE string OVERWRITE;
    DEFINE FIELD title       ON TABLE task TYPE string OVERWRITE;
    DEFINE FIELD status      ON TABLE task TYPE string OVERWRITE;
    DEFINE FIELD created_at  ON TABLE task TYPE datetime OVERWRITE;
    DEFINE FIELD updated_at  ON TABLE task TYPE datetime OVERWRITE;
    DEFINE FIELD project     ON TABLE task TYPE string OVERWRITE;
    DEFINE FIELD tags        ON TABLE task TYPE array<string> OVERWRITE;
    DEFINE FIELD metadata    ON TABLE task TYPE option<object> OVERWRITE;

  Field type reference:
    | SurrealQL Type     | Rust Equivalent         | Use Case |
    |--------------------|------------------------|---------|
    | string             | String / &str          | Text data |
    | int                | i64                    | Integer counters, IDs |
    | float              | f64                    | Numeric measurements |
    | bool               | bool                   | Flags |
    | datetime           | chrono::DateTime / String | Timestamps |
    | array<T>           | Vec<T>                 | Lists |
    | object             | serde_json::Value / struct | Nested data |
    | option<T>          | Option<T>              | Nullable fields |
    | record(table)      | Thing / String         | Foreign key reference |
    | array<float>       | Vec<f32>               | Embedding vectors |

  Option fields (nullable):
    DEFINE FIELD description ON TABLE task TYPE option<string> OVERWRITE;

  Record references (foreign keys):
    DEFINE FIELD epic_id ON TABLE task TYPE option<record<epic>> OVERWRITE;

Index Definitions (DEFINE INDEX):

  Indexes speed up WHERE clause lookups. Define after table and field definitions.

  Unique index (enforces uniqueness):
    DEFINE INDEX idx_task_format_id ON TABLE task FIELDS format_id UNIQUE OVERWRITE;

  Non-unique index (speeds up lookups without uniqueness):
    DEFINE INDEX idx_task_status    ON TABLE task FIELDS status OVERWRITE;
    DEFINE INDEX idx_task_project   ON TABLE task FIELDS project OVERWRITE;

  Compound index (multi-field):
    DEFINE INDEX idx_task_project_status ON TABLE task FIELDS project, status OVERWRITE;

RELATE (Creating Graph Edges):

  RELATE creates a directed edge between two records. The edge record itself can carry fields.

  Syntax:
    RELATE source_record->relation_table->target_record
      SET field = value, ...;

  Example: task depends on another task
    RELATE task:abc->depends_on->task:xyz
      SET created_at = time::now();

  Example: session produced a commit
    RELATE session:ses123->produced->commit:abc456
      SET timestamp = time::now();

  Creating bidirectional edges (two RELATE statements):
    RELATE epic:e1->contains->task:t1;
    -- The reverse direction (task<-contains<-epic) is queryable without a second RELATE

Anti-patterns:

  | Anti-Pattern | Correct Alternative |
  |--------------|---------------------|
  | SCHEMALESS tables in production | SCHEMAFULL with explicit DEFINE FIELD |
  | DEFINE TABLE without OVERWRITE | Add OVERWRITE for idempotent startup |
  | Implicit field types (no TYPE) | Always specify TYPE explicitly |
  | Storing JSON blobs as strings | Use TYPE object or define a proper typed table |
  | Raw string for record references | Use TYPE record(table) for FK fields |

Procedure:
  1. Define table with SCHEMAFULL OVERWRITE
  2. Define each field with explicit TYPE and OVERWRITE
  3. Define indexes for all fields used in WHERE clauses
  4. Use UNIQUE indexes for natural key fields (format_id, external_id)
  5. Use RELATE for graph edges; do not store IDs as plain strings for relationships
  6. Order definitions: tables → fields → indexes → relations

Output: Strict, typed schema with idempotent definitions and appropriate indexes
```

### apply-graph-relations

```text
When: Designing or querying graph relationships between records
Purpose: Model domain relationships as graph edges and traverse them with SurrealQL graph syntax
Enforcement: ENF-L3 Advisory

TYPE RELATION Tables:

  Graph relation tables use TYPE RELATION to declare them as edge tables.
  Edge tables have built-in `in` (source) and `out` (target) fields.

  Define a relation table:
    DEFINE TABLE depends_on TYPE RELATION SCHEMAFULL OVERWRITE;
    DEFINE TABLE contains   TYPE RELATION SCHEMAFULL OVERWRITE;
    DEFINE TABLE produced   TYPE RELATION SCHEMAFULL OVERWRITE;
    DEFINE TABLE blocked_by TYPE RELATION SCHEMAFULL OVERWRITE;

  Add fields to the edge (optional — for edge metadata):
    DEFINE FIELD created_at ON TABLE depends_on TYPE datetime OVERWRITE;
    DEFINE FIELD reason     ON TABLE depends_on TYPE option<string> OVERWRITE;

Creating Edges with RELATE:

  RELATE source->relation->target;

  Examples:
    -- Task t1 depends on task t2
    RELATE task:t1->depends_on->task:t2 SET created_at = time::now();

    -- Epic e1 contains task t1
    RELATE epic:e1->contains->task:t1;

    -- Session produced a ledger entry
    RELATE session:ses123->produced->ledger:l456 SET timestamp = time::now();

Traversal Syntax:

  Outbound traversal (follow arrows forward):
    SELECT ->depends_on->task FROM task:t1;
    -- Returns all tasks that task:t1 depends on

  Inbound traversal (follow arrows backward):
    SELECT <-contains<-epic FROM task:t1;
    -- Returns the epic(s) that contain task:t1

  Bidirectional traversal:
    SELECT <->depends_on<->task FROM task:t1;
    -- Returns all tasks connected to task:t1 in either direction

  Multi-hop traversal:
    SELECT ->contains->task->depends_on->task FROM epic:e1;
    -- Returns all tasks that tasks in epic e1 depend on

  Fetch related record fields inline:
    SELECT *, ->depends_on->(task.* AS deps) FROM task:t1;
    -- Returns task:t1 with its dependencies embedded

  Count edges:
    SELECT count(->depends_on->task) AS dep_count FROM task:t1;

Common Relation Patterns in CodeFlow:

  | Relation | Source | Target | Meaning |
  |----------|--------|--------|---------|
  | depends_on | task | task | Task has prerequisite task |
  | contains | epic | task | Epic owns task |
  | produced | session | ledger | Session created ledger entry |
  | blocked_by | task | task | Task is blocked by another |
  | tagged_with | task | tag | Task has label |

Graph vs JOIN Anti-patterns:

  | Anti-Pattern | Correct Alternative |
  |--------------|---------------------|
  | Storing related IDs as string array | Use RELATE to create graph edges |
  | Self-join for dependency lookup | Use ->depends_on->task traversal |
  | Separate junction table with FK columns | TYPE RELATION table with RELATE |
  | Multiple round-trips for multi-hop | Single SurrealQL multi-hop query |

Procedure:
  1. Identify domain relationships (has-many, belongs-to, many-to-many)
  2. Define TYPE RELATION table for each relationship
  3. Add DEFINE FIELD for edge metadata (created_at, reason, etc.)
  4. Use RELATE to create edges when records are linked
  5. Use -> (outbound), <- (inbound), or <-> (bidirectional) for traversal
  6. Prefer multi-hop SurrealQL over application-level multiple queries

Output: Graph schema with typed relation tables and efficient traversal queries
```

### apply-vector-index

```text
When: Adding semantic search, embedding storage, or similarity-based retrieval
Purpose: Define HNSW vector indexes for cosine similarity search on float arrays
Enforcement: ENF-L3 Advisory

Vector Field Definition:

  Store embeddings as array<float> fields:
    DEFINE TABLE memory SCHEMAFULL OVERWRITE;
    DEFINE FIELD content    ON TABLE memory TYPE string OVERWRITE;
    DEFINE FIELD embedding  ON TABLE memory TYPE array<float> OVERWRITE;
    DEFINE FIELD project    ON TABLE memory TYPE string OVERWRITE;
    DEFINE FIELD created_at ON TABLE memory TYPE datetime OVERWRITE;

HNSW Index Definition:

  HNSW (Hierarchical Navigable Small World) is SurrealDB's approximate nearest neighbor index.

  Required parameters:
    - DIMENSION N: Size of the embedding vector (must match your model's output)
    - TYPE F32: Use 32-bit floats (F32 = standard for ML embeddings)
    - DIST COSINE: Cosine similarity (best for normalized text embeddings)

  Syntax:
    DEFINE INDEX idx_embedding
      ON TABLE memory
      FIELDS embedding
      HNSW DIMENSION 384 TYPE F32 DIST COSINE
      OVERWRITE;

  Dimension values by model:
    | Model | Dimension | Notes |
    |-------|-----------|-------|
    | all-MiniLM-L6-v2 (ONNX) | 384 | CodeFlow default via ort crate |
    | text-embedding-3-small | 1536 | OpenAI API |
    | text-embedding-3-large | 3072 | OpenAI API |
    | nomic-embed-text | 768 | Local Ollama |

  Distance metric selection:
    | Metric | SurrealQL | When to Use |
    |--------|-----------|-------------|
    | Cosine similarity | DIST COSINE | Normalized text embeddings (DEFAULT) |
    | Euclidean distance | DIST EUCLIDEAN | Raw coordinate vectors |
    | Manhattan distance | DIST MANHATTAN | Sparse vectors |

  Additional HNSW parameters (optional):
    HNSW DIMENSION 384 TYPE F32 DIST COSINE EFC 150 M 16 OVERWRITE;
    -- EFC: ef_construction (higher = better index quality, slower build, default 150)
    -- M: max connections per layer (higher = better recall, more memory, default 16)

Similarity Search Query:

  Find top-K nearest neighbors to a query embedding:
    SELECT id, content, vector::similarity::cosine(embedding, $query_embedding) AS score
    FROM memory
    WHERE project = $project
    ORDER BY score DESC
    LIMIT 10;

  With threshold (only return results above similarity score):
    SELECT id, content, vector::similarity::cosine(embedding, $query_embedding) AS score
    FROM memory
    WHERE project = $project
      AND vector::similarity::cosine(embedding, $query_embedding) > 0.7
    ORDER BY score DESC
    LIMIT 5;

Rust Integration:

  Pass embedding vector as a parameter to SurrealDB queries:

    use surrealdb::sql::Value;

    let query_vec: Vec<f32> = model.embed(text)?;
    let results = db
        .query("SELECT id, content, vector::similarity::cosine(embedding, $vec) AS score
                FROM memory WHERE project = $project ORDER BY score DESC LIMIT $k")
        .bind(("vec", query_vec))
        .bind(("project", project_id))
        .bind(("k", 10_usize))
        .await?;

Anti-patterns:

  | Anti-Pattern | Correct Alternative |
  |--------------|---------------------|
  | Using TYPE F64 for embeddings | Use TYPE F32 (ML models output f32, saves 50% memory) |
  | Wrong DIMENSION value | Always match to model output dimension exactly |
  | DIST EUCLIDEAN for text embeddings | Use DIST COSINE for normalized text embeddings |
  | No index on embedding field | Always define HNSW INDEX before running similarity queries |
  | Storing embeddings as JSON strings | Use TYPE array<float> with HNSW index |

Procedure:
  1. Determine embedding dimension from the model (e.g., 384 for all-MiniLM-L6-v2)
  2. Define the table with SCHEMAFULL OVERWRITE
  3. Define the embedding field: TYPE array<float>
  4. Define HNSW index: DIMENSION {N} TYPE F32 DIST COSINE OVERWRITE
  5. In Rust, bind the query embedding as Vec<f32>
  6. Use vector::similarity::cosine() in SELECT and ORDER BY for ranking

Output: Vector-capable table with HNSW index and similarity search queries
```

### apply-schema-migration

```text
When: Adding tables, fields, or indexes — or initializing the database on startup
Purpose: Apply schema changes idempotently using OVERWRITE, with no separate migration files
Enforcement: ENF-L3 Advisory

Migration Strategy — Idempotent DEFINE Statements:

  CodeFlow does NOT use versioned migration files (no Flyway, no Diesel migrations).
  Instead, ALL schema definitions use OVERWRITE and are applied on every startup.

  This works because:
    - DEFINE TABLE ... OVERWRITE re-applies the table definition without dropping data
    - DEFINE FIELD ... OVERWRITE re-applies field definitions; existing data is preserved
    - DEFINE INDEX ... OVERWRITE rebuilds the index if the definition changed

  The schema is defined in Rust source code and executed at DB init.

  Idempotent startup pattern (Rust):
    pub async fn apply_schema(db: &Surreal<SurrealKV>) -> Result<(), surrealdb::Error> {
        db.query(include_str!("../sql/schema.surql")).await?.check()?;
        Ok(())
    }

  Where schema.surql contains all DEFINE statements in the correct order.

Schema Application Order (mandatory):

  Always apply definitions in this order to satisfy dependencies:
    1. DEFINE NAMESPACE (if needed)
    2. DEFINE DATABASE (if needed)
    3. DEFINE TABLE statements (all tables before fields or indexes)
    4. DEFINE FIELD statements (per table)
    5. DEFINE INDEX statements (after fields they index exist)
    6. DEFINE TABLE ... TYPE RELATION statements (after source/target tables)

  Example schema.surql structure:
    -- 1. Tables
    DEFINE TABLE task SCHEMAFULL OVERWRITE;
    DEFINE TABLE epic SCHEMAFULL OVERWRITE;
    DEFINE TABLE session SCHEMAFULL OVERWRITE;
    DEFINE TABLE memory SCHEMAFULL OVERWRITE;

    -- 2. Fields: task
    DEFINE FIELD id          ON TABLE task TYPE string OVERWRITE;
    DEFINE FIELD title       ON TABLE task TYPE string OVERWRITE;
    DEFINE FIELD project     ON TABLE task TYPE string OVERWRITE;
    DEFINE FIELD status      ON TABLE task TYPE string OVERWRITE;
    DEFINE FIELD created_at  ON TABLE task TYPE datetime OVERWRITE;
    DEFINE FIELD updated_at  ON TABLE task TYPE datetime OVERWRITE;

    -- 3. Indexes: task
    DEFINE INDEX idx_task_format_id ON TABLE task FIELDS format_id UNIQUE OVERWRITE;
    DEFINE INDEX idx_task_project   ON TABLE task FIELDS project OVERWRITE;
    DEFINE INDEX idx_task_status    ON TABLE task FIELDS status OVERWRITE;

    -- 4. Relation tables
    DEFINE TABLE depends_on TYPE RELATION SCHEMAFULL OVERWRITE;
    DEFINE TABLE contains   TYPE RELATION SCHEMAFULL OVERWRITE;

    -- 5. Vector indexes (after fields defined)
    DEFINE INDEX idx_memory_embedding ON TABLE memory FIELDS embedding
      HNSW DIMENSION 384 TYPE F32 DIST COSINE OVERWRITE;

Adding New Fields (workflow):

  1. Add DEFINE FIELD to schema.surql with OVERWRITE
  2. Re-run the schema apply on next startup — existing rows get the field with NULL/default
  3. Backfill existing records if needed (one-time query at startup, guarded by a version marker)
  4. No migration version file needed

Removing Fields (workflow):

  1. Remove the DEFINE FIELD from schema.surql
  2. Optionally run: UPDATE table SET field = NONE to clear the data
  3. REMOVE FIELD (DDL) if strict cleanup required
  4. SCHEMAFULL tables silently ignore undefined fields on existing records after removal

Anti-patterns:

  | Anti-Pattern | Correct Alternative |
  |--------------|---------------------|
  | Separate migration version files | Embed all DEFINE statements in schema.surql with OVERWRITE |
  | DEFINE TABLE without OVERWRITE | Always use OVERWRITE — prevents errors on re-run |
  | Applying schema mid-transaction | Apply schema at startup, before any data operations |
  | Fields defined before their table | Follow the order: tables → fields → indexes → relations |
  | Using DROP TABLE for schema changes | DEFINE TABLE ... OVERWRITE is safe — preserves data |

Procedure:
  1. Add or modify DEFINE statements in schema.surql
  2. Ensure OVERWRITE is present on every DEFINE statement
  3. Verify order: tables → fields → indexes → relation tables → vector indexes
  4. Apply via apply_schema() on startup
  5. Test by running the schema twice (second run must succeed without errors)

Output: Idempotent schema that can be safely re-applied on every startup
```

### apply-connection-management

```text
When: Opening a SurrealDB connection, passing DB to functions, or managing connection lifecycle
Purpose: Ensure a single Surreal instance is initialized once per process and shared correctly
Enforcement: ENF-L3 Advisory

Single Connection Rule:

  One Surreal instance per process. Initialize once at startup. Pass by shared reference.
  Do NOT create a new connection for each request, query, or function call.

  Correct pattern:
    // In main.rs or application init
    use surrealdb::engine::local::SurrealKV;
    use surrealdb::Surreal;

    pub async fn open_db(path: &str) -> anyhow::Result<Surreal<SurrealKV>> {
        let db = Surreal::new::<SurrealKV>(path).await
            .with_context(|| format!("opening surrealdb at {path}"))?;
        db.use_ns("codeflow").use_db("main").await
            .context("selecting namespace and database")?;
        Ok(db)
    }

    // In main():
    let db = open_db("/path/to/codeflow.db").await?;
    apply_schema(&db).await?;
    run_app(&db).await?;

Sharing the Connection:

  Pass as shared reference (&Surreal<Db>) to avoid cloning or wrapping in Arc.
  The Surreal struct is already internally reference-counted — it is cheap to clone
  when sharing across owned structures is needed.

  Pass by reference (preferred for function calls):
    async fn list_tasks(db: &Surreal<SurrealKV>, project: &str) -> Result<Vec<Task>> {
        db.query("SELECT * FROM task WHERE project = $project")
            .bind(("project", project))
            .await?
            .take(0)
    }

  Clone for owned struct (acceptable — internally ref-counted):
    pub struct TaskStore {
        db: Surreal<SurrealKV>,
    }

    impl TaskStore {
        pub fn new(db: Surreal<SurrealKV>) -> Self {
            Self { db }
        }
    }

    // In main:
    let task_store = TaskStore::new(db.clone());  // cheap clone

Namespace and Database Selection:

  Always call use_ns() and use_db() immediately after connecting.
  CodeFlow uses namespace = "codeflow" and database = "main" for production.

    db.use_ns("codeflow").use_db("main").await?;

  For integration tests, use the same namespace but a unique database per test:
    db.use_ns("codeflow").use_db(&format!("test_{}", uuid)).await?;

Connection Lifecycle:

  | Event | Action |
  |-------|--------|
  | Process startup | open_db(), use_ns/use_db(), apply_schema() |
  | Normal operation | Share reference; no reconnect needed |
  | Process shutdown | Drop the Surreal instance (auto-closes connection) |
  | Error on query | Log error, return Err — do NOT reconnect |
  | Connection to in-memory (tests) | Surreal::new::<Mem>().await? |

No Connection Pooling:

  SurrealDB embedded (surrealkv://) does not require connection pooling.
  The single Surreal instance handles all concurrency internally.
  Do NOT wrap in deadpool, r2d2, or any connection pool.

Anti-patterns:

  | Anti-Pattern | Correct Alternative |
  |--------------|---------------------|
  | Open a new connection per query | Share one Surreal instance for the process lifetime |
  | Wrap Surreal in Arc<Mutex<>> | Surreal is already Send + Sync — share directly or clone |
  | Call use_ns/use_db on every query | Call once after open; it persists for the connection |
  | Create runtime inside DB open fn | Accept async context from caller; use .await |
  | Reconnect on query error | Log and return error; let caller decide retry strategy |

Procedure:
  1. Open one Surreal<SurrealKV> connection in main() or application init
  2. Call use_ns("codeflow").use_db("main") immediately after
  3. Run apply_schema() before any data operations
  4. Pass &db to functions, or clone into owned structs (cheap)
  5. Never open additional connections for the same process

Output: Single-connection setup with correct namespace selection and shared reference passing
```

### apply-testing-patterns

```text
When: Writing unit tests, integration tests, or test helpers that involve SurrealDB
Purpose: Use in-memory DB for unit tests and file-backed DB for integration tests; never share DB state between tests
Enforcement: ENF-L3 Advisory

Unit Tests — In-Memory DB:

  Use Surreal::new::<Mem>() for tests that need a real DB but not persistent data.
  Each test gets its own in-memory instance. Schema is applied before each test.

    use surrealdb::engine::local::Mem;
    use surrealdb::Surreal;

    async fn test_db() -> Surreal<Mem> {
        let db = Surreal::new::<Mem>(()).await
            .expect("failed to create in-memory test DB");
        db.use_ns("codeflow").use_db("test").await
            .expect("failed to select test namespace");
        apply_schema(&db).await
            .expect("failed to apply schema");
        db
    }

    #[tokio::test]
    async fn test_insert_and_retrieve_task() {
        let db = test_db().await;
        // insert a task
        let task = Task { id: "t-001".to_string(), title: "Test".to_string(), project: "proj".to_string() };
        db.query("CREATE task:t-001 SET title = $title, project = $project")
            .bind(("title", &task.title))
            .bind(("project", &task.project))
            .await
            .unwrap();
        // retrieve and verify
        let result: Option<Task> = db.select(("task", "t-001")).await.unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().title, "Test");
    }

  Rules for in-memory tests:
    - Each #[tokio::test] function calls test_db() independently (fresh DB)
    - Never reuse a test_db() instance across multiple test functions
    - apply_schema() must run before any data operations in the test

Integration Tests — File-Backed DB:

  Use a temp directory for tests that require persistent behavior or cross-query state.

    use std::path::PathBuf;
    use tempfile::TempDir;
    use surrealdb::engine::local::SurrealKV;
    use surrealdb::Surreal;

    async fn integration_test_db() -> (Surreal<SurrealKV>, TempDir) {
        let dir = TempDir::new().expect("failed to create temp dir");
        let path = dir.path().join("test.db");
        let db = Surreal::new::<SurrealKV>(path.to_str().unwrap()).await
            .expect("failed to open test DB");
        db.use_ns("codeflow").use_db("test").await.unwrap();
        apply_schema(&db).await.unwrap();
        (db, dir)  // Return TempDir to keep it alive; dropped = cleaned up
    }

    #[tokio::test]
    async fn test_schema_migration_idempotent() {
        let (db, _dir) = integration_test_db().await;
        // Apply schema a second time — must succeed without error
        apply_schema(&db).await.expect("second schema application should be idempotent");
    }

Test Isolation:

  | Rule | Reason |
  |------|--------|
  | Fresh DB per test function | Shared state causes flaky, order-dependent tests |
  | No global DB in test module | Global state makes parallel test execution unsafe |
  | TempDir kept alive for test duration | Dropping TempDir deletes the DB file prematurely |
  | No sleep() for async timing | Use tokio::time::pause()/advance() for time control |

Async Test Setup:

  Use #[tokio::test] for all async DB tests:
    #[tokio::test]
    async fn test_task_store() {
        let db = test_db().await;
        // ... test body
    }

  For tests requiring multi-thread runtime:
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_concurrent_writes() {
        let db = test_db().await;
        // ... concurrent write test
    }

Anti-patterns:

  | Anti-Pattern | Correct Alternative |
  |--------------|---------------------|
  | surrealkv:// in unit tests | Use Surreal::new::<Mem>() for unit tests |
  | Shared DB instance across test functions | Fresh test_db() per test function |
  | Global static Surreal in tests | Instantiate in each test function |
  | File path hardcoded in test | Use TempDir::new() for temp paths |
  | Not calling apply_schema() in test | Always apply schema before data operations |
  | Skipping TempDir binding (let _ = dir) | Bind to named variable to keep alive: let (_db, _dir) = ... |

Procedure:
  1. Write a test_db() helper that returns Surreal<Mem> with schema applied
  2. Call test_db() at the start of each unit test function
  3. For file-backed integration tests, use TempDir and return both (db, dir)
  4. Use #[tokio::test] for all async tests
  5. Never share DB state between test functions
  6. Verify schema idempotence by running apply_schema() twice in at least one test

Output: Isolated, deterministic tests with in-memory DB for units and TempDir for integration
```

### apply-data-model

```text
When: Designing record schemas, adding tables, or verifying field conventions
Purpose: Ensure consistent field naming, mandatory fields, and record ID conventions across all tables
Enforcement: ENF-L3 Advisory

Mandatory Fields (all records):

  Every record in every table MUST have these fields:

    | Field      | Type     | SurrealQL Definition | Purpose |
    |------------|----------|---------------------|---------|
    | project    | string   | TYPE string          | Multi-project isolation |
    | created_at | datetime | TYPE datetime        | Creation timestamp |
    | updated_at | datetime | TYPE datetime        | Last-modified timestamp |

  The project field is the primary isolation mechanism for multi-project deployments.
  All queries that fetch user data MUST include WHERE project = $project.

  Example table with mandatory fields:
    DEFINE TABLE task SCHEMAFULL OVERWRITE;
    DEFINE FIELD project    ON TABLE task TYPE string OVERWRITE;
    DEFINE FIELD created_at ON TABLE task TYPE datetime OVERWRITE;
    DEFINE FIELD updated_at ON TABLE task TYPE datetime OVERWRITE;
    -- task-specific fields:
    DEFINE FIELD title      ON TABLE task TYPE string OVERWRITE;
    DEFINE FIELD status     ON TABLE task TYPE string OVERWRITE;
    DEFINE FIELD format_id  ON TABLE task TYPE string OVERWRITE;

Record ID Format:

  SurrealDB record IDs use the format `table:id`, where id is determined by the application.

  CodeFlow ID conventions:
    | Table | ID Format | Example |
    |-------|-----------|---------|
    | task | task:{ulid} | task:01aryz6s41 |
    | epic | epic:{ulid} | epic:01aryz6s41 |
    | session | session:{session-id} | session:ses-1234567890abc |
    | memory | memory:{ulid} | memory:01aryz6s41 |
    | ledger | ledger:{ulid} | ledger:01aryz6s41 |

  Use ULIDs for auto-generated IDs (time-sortable, URL-safe):
    -- Create with explicit ULID-style ID in SurrealQL
    CREATE task:ulid() SET title = $title, project = $project, created_at = time::now();

  In Rust, generate ULIDs and pass them:
    use ulid::Ulid;
    let id = Ulid::new().to_string().to_lowercase();
    db.query("CREATE task:$id SET title = $title, project = $project, created_at = time::now()")
        .bind(("id", id))
        .bind(("title", title))
        .bind(("project", project))
        .await?;

Standard Field Naming Conventions:

  | Convention | Rule | Example |
  |------------|------|---------|
  | snake_case | All field names use snake_case | format_id, created_at |
  | Timestamps | datetime type, name ends in _at | created_at, updated_at, completed_at |
  | Booleans | bool type, name starts with is_ | is_active, is_deleted |
  | Optional fields | option<T> | description option<string> |
  | Arrays | array<T> | tags array<string> |
  | References | record(table) | epic_id record<epic> |
  | Enums (status) | string with value constraints | status string |

Format IDs (human-readable secondary keys):

  Many records have a human-readable format_id separate from the DB record ID:
    DEFINE FIELD format_id ON TABLE task TYPE string OVERWRITE;
    DEFINE INDEX idx_task_format_id ON TABLE task FIELDS format_id UNIQUE OVERWRITE;

  Format patterns:
    | Table | Format ID Pattern | Example |
    |-------|-------------------|---------|
    | task | {AREA}-TSK-{epic-NNN}-{seq-NNN} | INF-TSK-022-004 |
    | epic | {AREA}-EPC-{NNN} | INF-EPC-022 |

Updated At Convention:

  Always update updated_at when modifying a record:
    UPDATE task SET status = $status, updated_at = time::now() WHERE id = $id;

  In Rust:
    db.query("UPDATE task SET status = $status, updated_at = time::now() WHERE id = $id")
        .bind(("status", new_status))
        .bind(("id", task_id))
        .await?;

Project Isolation Query Pattern:

  All data queries MUST filter by project:
    -- Correct:
    SELECT * FROM task WHERE project = $project AND status = 'active';

    -- WRONG (missing project filter — leaks cross-project data):
    SELECT * FROM task WHERE status = 'active';

Anti-patterns:

  | Anti-Pattern | Correct Alternative |
  |--------------|---------------------|
  | Missing project field on any table | Add DEFINE FIELD project TYPE string OVERWRITE to every table |
  | Querying without WHERE project = $project | Always filter by project in data queries |
  | Random UUIDs as record IDs | Use ULID for time-sortable IDs |
  | Storing format_id as the SurrealDB record ID | Use separate id (ULID) and format_id fields |
  | Missing updated_at on UPDATE | Always SET updated_at = time::now() on updates |
  | Omitting created_at on CREATE | Always SET created_at = time::now() on creates |

Procedure:
  1. For every new table, define: project, created_at, updated_at (mandatory)
  2. Assign record IDs using ULID format
  3. Add format_id field + UNIQUE index for human-readable secondary key tables
  4. Use snake_case for all field names
  5. Add option<T> for nullable fields
  6. Always include WHERE project = $project in data queries

Output: Records with consistent mandatory fields, ULID IDs, and project isolation
```
