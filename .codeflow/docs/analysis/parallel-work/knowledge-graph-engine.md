---
title: "Knowledge Graph Engine Analysis"
type: analysis
status: draft
author: cf-planning
created_at: "2026-03-06"
updated_at: "2026-03-06"
parent: "parallel-work/README.md"
---

# Knowledge Graph Engine Analysis

[<- Back to Overview](README.md)

## Table of Contents

- [1. Executive Summary](#1-executive-summary)
- [2. Research Findings](#2-research-findings)
- [3. Proposed Architecture: CodeFlow Knowledge Engine](#3-proposed-architecture-codeflow-knowledge-engine)
- [4. Gap Analysis](#4-gap-analysis)
- [5. Implementation Recommendations](#5-implementation-recommendations)
- [6. Research References](#6-research-references)
- [7. Open Questions](#7-open-questions)
- [8. Multi-User Synchronization Architecture](#8-multi-user-synchronization-architecture)
- [9. Trigger Points and Scheduling](#9-trigger-points-and-scheduling)
- [10. Configuration](#10-configuration)
- [11. Bootstrap and Incremental Strategy](#11-bootstrap-and-incremental-strategy)

---

## 1. Executive Summary

### Why Knowledge Graphs Matter for CodeFlow

CodeFlow's three-tier data model (JSONL ledger, SQLite/SurrealDB, Markdown) stores *what happened* -- events, tasks, sessions, decisions. But it does not capture *how things relate semantically* -- which decisions informed which patterns, which problems recur across sessions, which components are conceptually linked even when they share no direct dependency.

A knowledge graph transforms flat records into a connected intelligence layer where relationships are first-class citizens. This enables:

- **Contextual retrieval**: Instead of keyword search, find "decisions related to the authentication component that affected session management" via graph traversal.
- **Cross-project reasoning**: Link patterns discovered in one project to problems in another.
- **Self-improving memory**: Strengthen connections that prove useful, prune those that don't.

### Three Layers of Graph Intelligence

| Layer | Name | When Built | Source |
|-------|------|-----------|--------|
| 1 | **Structural Graph** | Epic 0 (Rust CLI) | Explicit RELATE statements when creating records (task->epic, session->task) |
| 2 | **Extracted Knowledge Graph** | Epic C (Global Intelligence) | LLM-based entity and relationship extraction from text |
| 3 | **Self-Evolving Graph** | Epic C+ (Future) | Memify-style refinement: edge reweighting, stale pruning, transitive inference |

### Critical Insight

**SurrealDB provides graph storage and traversal, NOT automatic graph construction.** You must explicitly:

1. Define relation tables (`DEFINE TABLE TYPE RELATION`)
2. Create edges (`RELATE record:a->edge->record:b`)
3. Define vector indices (`DEFINE INDEX ... HNSW DIMENSION N`)
4. Build the extraction pipeline (LLM-based entity/relationship extraction)
5. Build the refinement pipeline (graph maintenance and evolution)

This is fundamentally different from expecting the database to "know" about relationships. SurrealDB is the storage engine; the knowledge graph construction logic must be built in application code.

---

## 2. Research Findings

### 2.1 SurrealDB Graph Capabilities

#### DEFINE TABLE TYPE RELATION

SurrealDB supports explicit relation table definitions that enforce graph edge semantics:

```surql
-- Basic relation table
DEFINE TABLE belongs_to TYPE RELATION;

-- With type constraints (recommended)
DEFINE TABLE belongs_to TYPE RELATION IN task OUT epic;

-- With ENFORCED keyword (validates records exist before creating edge)
DEFINE TABLE depends_on TYPE RELATION IN task OUT task ENFORCED;

-- Relation with properties
DEFINE TABLE can_access TYPE RELATION IN project OUT project;
DEFINE FIELD scopes ON can_access TYPE array<string>;
DEFINE FIELD granted_at ON can_access TYPE datetime;
```

**Key behaviors:**

- `TYPE RELATION` ensures the table can only hold edges (not standalone records)
- `IN ... OUT ...` constrains which record types can be connected
- `ENFORCED` validates both endpoints exist before creating the edge
- Relation tables are full tables -- they can have fields, indexes, and events
- Without `TYPE RELATION`, edges can still be created via RELATE but without schema enforcement

#### RELATE Statement

The RELATE statement creates graph edges between records:

```surql
-- Basic edge creation
RELATE task:abc->belongs_to->epic:inf_022;

-- Edge with properties
RELATE project:frontend->can_access->project:backend
  SET granted_at = time::now(),
      scopes = ["tasks", "sessions", "memory"];

-- Edge with metadata
RELATE session:ses_123->produced_in->task:inf_tsk_022_009
  SET timestamp = time::now(),
      artifact_type = "code",
      files_changed = 12;
```

**Important constraints:**

- Graph edges are **deleted if either linked record is deleted** (cascade deletion)
- RELATE can execute before records exist (unless ENFORCED is set on the relation table)
- Edges are unidirectional by default; bidirectional traversal uses `<->` syntax

#### Graph Traversal Syntax

SurrealQL uses arrow notation for graph traversal:

```surql
-- Forward traversal: task -> its epic
SELECT ->belongs_to->epic.* AS epic FROM task:abc;

-- Backward traversal: epic <- all its tasks
SELECT <-belongs_to<-task.* AS tasks FROM epic:inf_022;

-- Bidirectional traversal
SELECT <->depends_on<->task.* AS related FROM task:abc;

-- Multi-hop traversal: task -> epic -> all sessions that produced tasks in that epic
SELECT ->belongs_to->epic<-belongs_to<-task<-produced_in<-session AS sessions
FROM task:abc;

-- Recursive depth notation
SELECT @.{3}->depends_on->task AS deep_deps FROM task:root;

-- Combined graph + filter
SELECT title, status
FROM task
WHERE project IN (SELECT ->can_access->project FROM project:frontend)
AND work_type = "PLAN"
ORDER BY created_at DESC
LIMIT 10;
```

#### HNSW Vector Indices

SurrealDB supports HNSW (Hierarchical Navigable Small World) indices for approximate nearest-neighbor vector search:

```surql
-- Define HNSW index on task embeddings (384-dim for all-MiniLM-L6-v2)
DEFINE INDEX task_embedding ON task FIELDS embedding HNSW DIMENSION 384;

-- Define HNSW index on session summaries
DEFINE INDEX session_embedding ON session FIELDS embedding HNSW DIMENSION 384;

-- Define HNSW index on memory entries
DEFINE INDEX memory_embedding ON memory FIELDS embedding HNSW DIMENSION 384;
```

#### DEFINE EVENT ASYNC (v3.0.0+)

SurrealDB events enable event-driven processing when records change:

```surql
-- Trigger extraction when a new task is created
DEFINE EVENT task_ingest ON TABLE task
  WHEN $event = "CREATE"
  THEN {
    CREATE extraction_queue SET
      source_table = "task",
      source_id = $after.id,
      text = $after.title + " " + $after.description,
      status = "pending",
      created_at = time::now();
  };

-- Trigger on session completion
DEFINE EVENT session_complete ON TABLE session
  WHEN $event = "UPDATE" AND $after.status = "completed"
  THEN {
    CREATE extraction_queue SET
      source_table = "session",
      source_id = $after.id,
      text = $after.summary,
      status = "pending",
      created_at = time::now();
  };
```

#### Schema Enforcement

| Feature | Syntax | Purpose |
|---------|--------|---------|
| Schemaless mode | `DEFINE TABLE task SCHEMALESS` | Accept any fields |
| Schemafull mode | `DEFINE TABLE task SCHEMAFULL` | Reject undefined fields |
| Relation typing | `TYPE RELATION IN task OUT epic` | Constrain edge endpoints |
| Field enforcement | `ENFORCED` on relation | Validate records exist |
| Symmetric dedup | `DEFINE INDEX ... FIELDS key UNIQUE` | Prevent duplicate bidirectional edges |

**Symmetric relationship deduplication:**

```surql
-- For bidirectional relations (e.g., similar_to), prevent A->B and B->A duplicates
DEFINE FIELD key ON TABLE similar_to VALUE <string>array::sort([in, out]);
DEFINE INDEX only_one_similarity ON similar_to FIELDS key UNIQUE;
```

#### What is NOT Automatic

| Capability | SurrealDB Provides | Must Be Built |
|-----------|-------------------|---------------|
| Edge storage | RELATE, TYPE RELATION | Application logic to decide WHEN to create edges |
| Graph traversal | Arrow syntax (-> <- <->) | Query construction for specific use cases |
| Vector search | HNSW index, cosine similarity | Embedding generation (ONNX/API) |
| Event triggers | DEFINE EVENT | Extraction pipeline consuming events |
| Entity extraction | -- | LLM-based NER/entity extraction |
| Relationship discovery | -- | LLM-based relationship inference |
| Graph refinement | -- | Pruning, reweighting, deduplication logic |
| Ontology management | -- | Schema evolution and type hierarchy |

### 2.2 Neo4j Comparison

#### Neo4j LLM Knowledge Graph Builder

Neo4j provides an official tool ([LLM Knowledge Graph Builder](https://neo4j.com/labs/genai-ecosystem/llm-graph-builder/)) that automates knowledge graph construction:

**Pipeline (6 steps):**

1. Document ingestion (PDFs, web pages, images, transcripts)
2. LLM-based entity extraction (supports OpenAI, Gemini, Claude, Llama3, Diffbot, Qwen)
3. LLM-based relationship extraction
4. Knowledge graph consolidation (automatic or schema-guided)
5. Embedding generation and storage
6. Graph database population (Cypher CREATE/MERGE)

**2025 features:**

- Community summary generation
- Local and global retrievers (parallel execution)
- Custom prompt instructions for extraction guidance
- Automatic graph consolidation (schema-free mode generates entity types and relationships automatically)
- Schema-guided extraction (provides type constraints to reduce noise)

#### Feature Comparison

| Capability | SurrealDB | Neo4j |
|-----------|-----------|-------|
| **Graph model** | Multi-model (graph + document + vector + relational) | Graph-native (property graph only) |
| **Query language** | SurrealQL (SQL-like + arrow syntax) | Cypher (pattern-matching focused) |
| **Automatic KG construction** | Not available (must build) | LLM Knowledge Graph Builder |
| **Schema enforcement** | Optional (SCHEMALESS/SCHEMAFULL/RELATION) | Optional (flexible labels) |
| **Vector search** | Native HNSW index | Requires plugin or GDS library |
| **Combined graph+vector query** | Single unified query plan | Separate query layers |
| **Graph algorithms** | Basic traversal, shortest path | GDS library: PageRank, community detection, centrality, etc. |
| **Deployment** | Embedded (Rust native) or server | Server only (JVM-based) |
| **Scaling model** | Multi-master distributed | Enterprise clustering |
| **Static vs dynamic graphs** | Optimized for continuously updated graphs | Optimized for relatively static graphs |
| **Embedded mode** | Yes (surrealkv://) | No |
| **Language** | Rust native | JVM (Java/Kotlin) |

#### Key Trade-offs for CodeFlow

**Neo4j advantages:**

- Mature graph algorithm library (GDS) -- PageRank, Louvain community detection, betweenness centrality
- Production-grade LLM Knowledge Graph Builder with schema-free and schema-guided modes
- Large ecosystem of graph visualization and analysis tools

**SurrealDB advantages (why we chose SurrealDB):**

- **Embedded mode**: Sub-millisecond access without server overhead -- critical for hook latency
- **Multi-model**: Graph + vector + document + relational in one query -- no bolt-on engines
- **Rust native**: Same language as the CLI, embedded as library dependency
- **Continuously updated graphs**: Designed for high-write workloads (sessions, events, tasks)
- **No external dependency**: The `codeflow` binary IS the database (no Docker, no JVM)

### 2.3 Cognee Architecture (Key Reference)

[Cognee](https://github.com/topoteretes/cognee) is an open-source knowledge engine that transforms raw data into persistent, dynamic AI memory for agents. Its architecture provides a strong reference model for CodeFlow's knowledge graph engine.

#### Pipeline: add -> cognify -> memify -> search

```text
                                    Cognee Pipeline
    +--------+     +-----------+     +-----------+     +---------+
    |  ADD   | --> |  COGNIFY  | --> |  MEMIFY   | --> |  SEARCH |
    | Ingest |     | Extract & |     | Refine &  |     | Retrieve|
    | Data   |     | Build KG  |     | Evolve    |     | Context |
    +--------+     +-----------+     +-----------+     +---------+
```

**ADD (Ingest):**

- Pythonic data pipelines for 30+ data sources
- Content normalized to plain text
- Hash-based deduplication
- Organized into datasets

**COGNIFY (Extract and Build Knowledge Graph):**

Six-stage pipeline:

1. **Classify documents** -- determine document type and processing strategy
2. **Check permissions** -- verify access rights
3. **Extract chunks** -- split into semantic units
4. **LLM entity and relationship extraction** -- extract subject-predicate-object triplets
5. **Generate summaries** -- create summary layers
6. **Embed and commit** -- generate vector embeddings, commit edges to graph store

**MEMIFY (Refine and Evolve):**

Optional post-cognify enrichment:

- **Prune stale nodes** -- remove entities no longer referenced
- **Strengthen frequent connections** -- increase edge weights for commonly traversed paths
- **Reweight edges based on usage signals** -- adapt graph based on query patterns
- **Add derived facts** -- transitive inference (if A->B and B->C, infer A->C)
- **Coding rules and triplet embeddings** -- additional enrichment layers

**SEARCH (Retrieve):**

14 retrieval modes including:

- `GRAPH_COMPLETION` (default) -- vector search as hint, then graph traversal for structured context
- Triplet-based search
- Multi-hop traversal
- Temporal-aware queries
- Hybrid vector + graph ranking

#### Two-Layer Memory Model

| Layer | Purpose | Scope |
|-------|---------|-------|
| **Session Memory** | Short-term working context | Loads relevant embeddings and graph fragments into runtime context for fast reasoning |
| **Permanent Memory** | Long-term knowledge artifacts | User data, interaction traces, external documents, derived relationships -- continuously cross-connected |

This maps directly to CodeFlow's needs: session-scoped context for the current PathFlow session, and permanent knowledge for cross-session and cross-project intelligence.

#### DataPoints

All entities in Cognee are `DataPoint` objects (Pydantic models) that become graph nodes:

```python
class Product(DataPoint):
    name: str
    description: str
    price: float
    metadata: dict = {"index_fields": ["name", "description"]}
```

Every DataPoint:

- Becomes a node in the graph store
- Has a corresponding embedding in the vector store
- Carries content and metadata for indexing
- Can have custom fields marked for embedding via `index_fields`

#### Key Design Patterns from Cognee

1. **Graph-vector hybrid**: Every node has a corresponding embedding -- bidirectional linking
2. **Async pipeline**: `add()`, `cognify()`, `memify()` are async operations
3. **Incremental processing**: Only new or updated files processed on re-runs (hash-based dedup)
4. **Ontology-driven validation**: Schema guides extraction quality
5. **Modular tasks**: Individual processing units compose into pipelines
6. **Evolving memory**: Not static storage -- adapts based on feedback and interaction traces

### 2.4 Production Knowledge Graph Patterns (2025-2026)

Research across production knowledge graph systems reveals converging patterns:

| Pattern | Details | Source |
|---------|---------|--------|
| **LLM-based extraction** | 89.7% precision, 92.3% recall for entity extraction with guided prompts | Nature Scientific Reports 2026 |
| **Hybrid batch-stream** | Batch processing for initial graph construction, streaming for incremental updates | Industry standard |
| **Incremental over full rebuild** | Hash-based dedup + delta processing vs full re-extraction | Cognee, Neo4j Builder |
| **Dynamic ontology evolution** | Schema evolves as new entity types and relationships are discovered | Neo4j LLM Builder 2025 |
| **Agent-driven automation** | NER agents propose entities, approval pipelines validate before graph commit | Neo4all, Agentic KG construction |
| **Governed pipelines** | AI extracts, surfaces quality issues, humans approve -- audit trail | Neo4all |
| **Relik** | Library for entity linking and relationship extraction -- identifying and categorizing relationships between entities | Neo4j/LlamaIndex |

---

## 3. Proposed Architecture: CodeFlow Knowledge Engine

### 3.1 Architecture Diagram

```text
PROJECT-LEVEL (Embedded SurrealDB)
=====================================================

  Session / Task / Memory writes (normal operations)
           |
           v
  +------------------+     +-------------------+
  |  INGEST          |     |  SEARCH           |
  |  DEFINE EVENT    | --> |  Hybrid Query     |
  |  on record write |     |  vector + graph   |
  |  -> queue entry  |     |  combined ranking |
  +------------------+     +-------------------+
           |                        ^
           v                        |
  +------------------+     +-------------------+
  |  COGNIFY         |     |  MEMIFY           |
  |  Idle-time LLM   | --> |  Prune, reweight  |
  |  entity extract  |     |  strengthen, infer|
  |  relationship    |     |  community detect |
  |  embedding gen   |     |  entity dedup     |
  +------------------+     +-------------------+
           |
           | Ledger sync (30s interval)
           v
GLOBAL-LEVEL (Daemon SurrealDB)
=====================================================

  +------------------+     +-------------------+
  |  AGGREGATE       |     |  Cross-Project    |
  |  Replay ledger   |     |  SEARCH           |
  |  from all        | --> |  Unified vector + |
  |  registered      |     |  graph traversal  |
  |  projects        |     |  across all repos |
  +------------------+     +-------------------+
           |                        ^
           v                        |
  +------------------+     +-------------------+
  |  Cross-Project   |     |  Global           |
  |  COGNIFY         |     |  MEMIFY           |
  |  Cross-project   | --> |  Cross-project    |
  |  relationships   |     |  edge reweighting |
  |  pattern linking |     |  pattern dedup    |
  +------------------+     +-------------------+
```

### 3.2 CodeFlow Ontology

#### Entity Types (16)

| # | Entity Type | Description | Source | Example |
|---|-------------|-------------|--------|---------|
| 1 | Concept | Abstract technical concept | LLM extraction | "event-driven architecture", "CRDT consistency" |
| 2 | Decision | Architectural or design decision (ADR) | Planning docs | "chose SurrealDB over SQLite" |
| 3 | Pattern | Recurring code/design pattern | Code analysis | "newtype wrapper pattern", "builder pattern" |
| 4 | Component | System module or subsystem | Code structure | "PathFlow engine", "hook pipeline" |
| 5 | Problem | Bug, issue, or challenge | Task/issue text | "session ID duplication", "context overflow" |
| 6 | Solution | Resolution to a problem | Task completion | "guard session creation behind source check" |
| 7 | Technology | External tool, library, platform | Dependencies | "SurrealDB", "Loro CRDT", "ONNX Runtime" |
| 8 | Developer | Team member or contributor | Git history | "developer:alice" |
| 9 | Requirement | Business/functional requirement | Planning docs | "sub-millisecond hook latency" |
| 10 | TestCase | Individual test or test suite | Test files | "test-cf-pre-tool-use-gate-check.sh" |
| 11 | Deployment | Environment + deployment event | CI/CD | "v0.4.0 deployed to production" |
| 12 | Incident | Production issue or outage | Issue tracker | "sentinel creation failure in autorun" |
| 13 | Convention | Coding standard, naming rule | Skills/standards | "cf-shell-standards: function naming" |
| 14 | Lesson | Post-mortem insight, learned experience | Session summaries | "context overflow causes lost teammate state" |
| 15 | API | Endpoint, interface, or contract | Code/docs | "DataStore trait public interface" |
| 16 | Milestone | Release, version, or project milestone | Epics/releases | "Epic 0 Phase 0C complete" |

Sources for extended ontology: Nathan Lasnoski article on enterprise SDLC knowledge graphs (entity types and relationship types for software development lifecycle), Cognee ontology patterns (DataPoints with custom fields), Neo4j knowledge graph builder (dynamic ontology evolution).

#### Relationship Types (18)

| # | Relationship | From | To | Meaning | Example |
|---|-------------|------|-----|---------|---------|
| 1 | relates_to | Concept | Concept | Semantic association | "CRDT" relates_to "eventual consistency" |
| 2 | solved_by | Problem | Solution | Resolution link | "session duplication" solved_by "source guard" |
| 3 | caused_by | Problem | Component | Root cause | "context overflow" caused_by "lead context window" |
| 4 | uses_pattern | Component | Pattern | Implementation pattern | "type system" uses_pattern "newtype wrapper" |
| 5 | decided_in | Decision | Session | Provenance | "chose SurrealDB" decided_in session:ses_abc |
| 6 | depends_on | Task | Task | Task dependency | task:009 depends_on task:006 |
| 7 | expert_in | Developer | Technology | Expertise signal | developer:alice expert_in "SurrealDB" |
| 8 | similar_to | Any | Any | Semantic similarity | pattern:A similar_to pattern:B |
| 9 | implements | Component | Requirement | Fulfillment | "SurrealStore" implements "sub-ms latency" |
| 10 | tested_by | Component | TestCase | Validation | "hook pipeline" tested_by "test-gate-check.sh" |
| 11 | deployed_to | Component | Deployment | Deployment mapping | "codeflow binary" deployed_to "v0.4.0" |
| 12 | owns | Developer | Component | Ownership | developer:alice owns "PathFlow engine" |
| 13 | evolved_from | Decision | Decision | Superseded ADR chain | "SurrealDB-only" evolved_from "hybrid SQLite+Surreal" |
| 14 | validates | TestCase | Requirement | Acceptance proof | "test-gate-check" validates "Edit blocked before PF3" |
| 15 | learned_from | Lesson | Problem | Post-mortem insight | "check liveness before respawn" learned_from "zombie teammates" |
| 16 | replaces | Component | Component | Migration/deprecation | "SurrealStore" replaces "SqliteStore" |
| 17 | conflicts_with | Decision | Decision | Mutual exclusion | "embedded-only" conflicts_with "always-on daemon" |
| 18 | references | Any | Any | Documentation coverage | ADR:017 references "data-layer-protection.md" |

### 3.3 Four Operations Detail

#### INGEST

Event-driven capture using SurrealDB's DEFINE EVENT:

```surql
-- Queue entity extraction when a task is created or updated
DEFINE EVENT task_ingest ON TABLE task
  WHEN $event IN ["CREATE", "UPDATE"]
  THEN {
    CREATE extraction_queue SET
      source_table = "task",
      source_id = $after.id,
      text = string::concat($after.title, " ", $after.description),
      status = "pending",
      priority = IF $event = "CREATE" THEN "normal" ELSE "low" END,
      created_at = time::now();
  };

-- Queue extraction when a session summary is written
DEFINE EVENT session_ingest ON TABLE session
  WHEN $event = "UPDATE" AND $after.summary IS NOT NONE
  THEN {
    CREATE extraction_queue SET
      source_table = "session",
      source_id = $after.id,
      text = $after.summary,
      status = "pending",
      priority = "normal",
      created_at = time::now();
  };

-- Queue extraction when memory entries are created
DEFINE EVENT memory_ingest ON TABLE memory
  WHEN $event = "CREATE"
  THEN {
    CREATE extraction_queue SET
      source_table = "memory",
      source_id = $after.id,
      text = string::concat($after.key, ": ", $after.value),
      status = "pending",
      priority = "high",
      created_at = time::now();
  };
```

#### COGNIFY

Daemon idle-time processing (not blocking hook operations):

```text
COGNIFY Pipeline (runs during idle time)
=========================================

1. Poll extraction_queue WHERE status = "pending" ORDER BY priority DESC
2. For each queue entry:
   a. Read source record text
   b. LLM entity extraction:
      - Input: text + ontology prompt (entity types above)
      - Output: list of (entity_type, name, description)
   c. LLM relationship extraction:
      - Input: text + entities + ontology prompt (relationship types above)
      - Output: list of (from_entity, relationship_type, to_entity, confidence)
   d. Deduplication:
      - Hash entity (type + normalized name)
      - Check existing: SELECT * FROM entity WHERE hash = $hash
      - If exists: merge properties, don't create duplicate
      - If new: CREATE entity
   e. Edge creation:
      - For each relationship: RELATE entity:a->rel_type->entity:b
      - Set confidence score and source reference
   f. Embedding generation:
      - Generate 384-dim embedding via ONNX (all-MiniLM-L6-v2)
      - UPDATE entity SET embedding = $vec WHERE id = $entity_id
   g. Mark queue entry processed:
      - UPDATE extraction_queue SET status = "processed" WHERE id = $queue_id
```

**SurrealQL for entity and edge storage:**

```surql
-- Entity table
DEFINE TABLE entity SCHEMAFULL;
DEFINE FIELD entity_type ON entity TYPE string;
DEFINE FIELD name ON entity TYPE string;
DEFINE FIELD description ON entity TYPE option<string>;
DEFINE FIELD hash ON entity TYPE string;
DEFINE FIELD embedding ON entity TYPE option<array<float>>;
DEFINE FIELD source_records ON entity TYPE array;
DEFINE FIELD created_at ON entity TYPE datetime;
DEFINE FIELD updated_at ON entity TYPE datetime;

DEFINE INDEX entity_hash ON entity FIELDS hash UNIQUE;
DEFINE INDEX entity_embedding ON entity FIELDS embedding HNSW DIMENSION 384;

-- Knowledge relationship (extracted by LLM)
DEFINE TABLE knows TYPE RELATION IN entity OUT entity;
DEFINE FIELD relationship_type ON knows TYPE string;
DEFINE FIELD confidence ON knows TYPE float;
DEFINE FIELD source_record ON knows TYPE string;
DEFINE FIELD extracted_at ON knows TYPE datetime;
```

#### MEMIFY

Graph refinement and evolution (runs periodically):

```text
MEMIFY Pipeline (runs on schedule, e.g., daily)
================================================

1. Edge Reweighting:
   - Query access logs: which graph paths are traversed in SEARCH queries
   - Increase weight of frequently traversed edges
   - Decrease weight of never-traversed edges

2. Entity Deduplication:
   - Find entities with high embedding similarity (cosine > 0.95)
   - Merge if same entity_type and similar name (fuzzy match)
   - Redirect all edges from merged entity to canonical entity

3. Stale Pruning:
   - Identify entities with zero inbound edges AND zero outbound edges
   - Identify entities not referenced by any source record that still exists
   - Delete orphaned entities (with audit log)

4. Transitive Inference:
   - If A->uses_pattern->B AND B->similar_to->C, infer A->uses_pattern->C (low confidence)
   - If Problem P->caused_by->Component C AND C->uses_pattern->Pattern T,
     check if pattern T is related to the problem class

5. Community Detection:
   - Group densely connected entities into communities
   - Generate community summaries (LLM)
   - Store as community nodes in graph for higher-level traversal
```

#### SEARCH

Hybrid vector + graph query with combined ranking:

```surql
-- GRAPH_COMPLETION mode: vector hint -> graph expansion -> context assembly

-- Step 1: Find semantically similar entities (vector search)
LET $hints = (
  SELECT id, name, entity_type,
         vector::similarity::cosine(embedding, $query_vec) AS score
  FROM entity
  WHERE vector::similarity::cosine(embedding, $query_vec) > 0.7
  ORDER BY score DESC
  LIMIT 10
);

-- Step 2: Expand via graph traversal (1-2 hops)
LET $expanded = (
  SELECT id, name, entity_type,
         ->knows->entity.* AS related,
         <-knows<-entity.* AS reverse_related
  FROM $hints
);

-- Step 3: Retrieve source records for context
LET $context = (
  SELECT source_records FROM entity
  WHERE id IN $expanded.id OR id IN $expanded.related.id
);

-- Step 4: Combined ranking
SELECT *, $hints.score AS vector_score
FROM $expanded
ORDER BY vector_score DESC;
```

### 3.4 Project-Level vs Global-Level Comparison

| Capability | Project-Level (Embedded) | Global-Level (Daemon) |
|-----------|-------------------------|----------------------|
| **Database** | surrealkv:// (single process) | Unix socket (multi-process) |
| **Latency** | <1ms | ~1-2ms |
| **Scope** | Single repository | All registered projects |
| **Structural graph** | task->epic, session->task | Same + cross-project edges |
| **Knowledge graph** | Entities from this project only | Entities merged across projects |
| **Vector search** | Project-scoped embeddings | All-project embeddings |
| **Graph traversal** | Within-project relations | Cross-project traversal via can_access |
| **INGEST trigger** | DEFINE EVENT on local tables | Ledger replay from all projects |
| **COGNIFY** | Idle-time in codeflow process | Daemon background thread |
| **MEMIFY** | Per-project refinement | Global refinement + cross-project linking |
| **SEARCH** | Fast local queries | Broader but slightly slower |

### 3.5 Comparison with Cognee and Neo4j

| Capability | CodeFlow KG Engine | Cognee | Neo4j LLM Builder |
|-----------|-------------------|--------|-------------------|
| **Graph storage** | SurrealDB (embedded + daemon) | Pluggable (Neo4j, etc.) | Neo4j (server only) |
| **Vector storage** | SurrealDB native HNSW | Pluggable (Qdrant, etc.) | Plugin/GDS required |
| **Unified query** | Single SurrealQL statement | Separate graph + vector queries | Separate Cypher + vector |
| **Entity extraction** | LLM-based (to build) | LLM-based (built-in) | LLM-based (built-in) |
| **Ontology** | Development-domain specific | General purpose | General purpose |
| **Incremental updates** | Ledger replay + hash dedup | Hash-based dedup | Full or incremental |
| **Graph refinement** | MEMIFY pipeline (to build) | memify() (built-in) | Manual |
| **Embedded mode** | Yes (zero overhead) | No (requires server) | No (JVM server) |
| **Offline operation** | Yes (ONNX embeddings) | Requires LLM API | Requires LLM API |
| **Cross-project** | Native (global daemon) | Not designed for | Not designed for |
| **Session + permanent memory** | Native (PathFlow integration) | Built-in two-layer | Manual implementation |
| **Domain optimization** | Development workflow tuned | Generic knowledge | Generic knowledge |

**CodeFlow's unique advantages:**

1. **Embedded + daemon hybrid**: Zero-overhead local access with optional multi-project aggregation
2. **Development-domain ontology**: Entity and relationship types tuned for software development
3. **Ledger-backed reconstruction**: Global graph always rebuildable from JSONL events
4. **PathFlow integration**: Knowledge graph feeds session context automatically
5. **ONNX offline**: No API dependency for embeddings

---

## 4. Gap Analysis

### What Exists in Current Architecture

| Component | Status | Location |
|-----------|--------|----------|
| Vector search infrastructure | Planned (Epic 0) | HNSW index definition in schema |
| Relation table definitions | Planned (Epic 0, tasks 009/011/013) | DEFINE TABLE TYPE RELATION for belongs_to, depends_on, produced_in |
| RELATE statements | Planned (Epic 0, tasks 011/013) | Graph edge creation in session and workgraph modules |
| Daemon infrastructure | Designed (global-intelligence.md) | codeflow db daemon start/stop/status |
| Ledger replay | Designed (global-intelligence.md) | Sync engine reads JSONL, imports to global DB |
| Embedding model | Designed (global-intelligence.md) | ONNX all-MiniLM-L6-v2 via `ort` crate |
| Project registration | Designed (global-intelligence.md) | DEFINE TABLE project SCHEMAFULL |
| can_access graph | Designed (global-intelligence.md) | Cross-project visibility via RELATE |

### What Does NOT Exist

| Component | Gap | Priority | Epic |
|-----------|-----|----------|------|
| Entity extraction pipeline | No LLM-based extraction of concepts, decisions, patterns from text | HIGH | Epic C |
| Relationship discovery | No automatic inference of how entities relate | HIGH | Epic C |
| Knowledge graph construction | No cognify-style pipeline to build graph from text | HIGH | Epic C |
| Idle-time refinement (MEMIFY) | No edge reweighting, pruning, or transitive inference | MEDIUM | Epic C+ |
| Cross-project entity linking | No deduplication of entities across projects | MEDIUM | Epic C |
| Community detection | No grouping of related entities into communities | LOW | Epic C+ |
| Extraction queue | No event-driven queue for pending extractions | HIGH | Epic C |
| Entity table schema | No DEFINE TABLE entity with hash and embedding | HIGH | Epic C |
| Knowledge relationship schema | No DEFINE TABLE knows TYPE RELATION for extracted relationships | HIGH | Epic C |
| Ontology configuration | No configurable entity/relationship type definitions | MEDIUM | Epic C |

### Technical Feasibility

| Requirement | Feasibility | Dependencies | Risk |
|-------------|-------------|-------------|------|
| SurrealDB DEFINE EVENT for INGEST | HIGH -- native SurrealDB v3 feature | SurrealDB v3.0.0+ | Low |
| LLM entity extraction (COGNIFY) | HIGH -- well-established pattern | LLM API (Claude/ONNX) | Medium (cost/latency) |
| ONNX local embeddings | HIGH -- `ort` crate is production-ready | all-MiniLM-L6-v2 model (~50MB) | Low |
| Graph refinement (MEMIFY) | MEDIUM -- custom logic required | Graph traversal + heuristics | Medium (complexity) |
| Cross-project entity linking | MEDIUM -- dedup across databases | Global daemon + entity hashing | Medium |
| Community detection | LOW -- requires graph algorithms | Custom implementation or library | High (no SurrealDB GDS equivalent) |

---

## 5. Implementation Recommendations

### Epic 0: Structural Graph (Layer 1) -- Already Planned

The following acceptance criteria have been added to Epic 0 tasks to ensure the structural graph foundation is in place:

| Task | Criterion Added |
|------|----------------|
| INF-TSK-022-009 (SurrealStore) | Schema includes DEFINE TABLE TYPE RELATION for belongs_to, depends_on, produced_in |
| INF-TSK-022-009 (SurrealStore) | Schema includes DEFINE INDEX HNSW DIMENSION for vector-ready fields |
| INF-TSK-022-011 (Session management) | Session module uses RELATE for produced_in graph edges |
| INF-TSK-022-013 (Workgraph operations) | Workgraph module uses RELATE for belongs_to and depends_on edges |

### Epic C: Knowledge Graph Pipeline (Layers 2+3) -- Tasks to Add

Recommended tasks for Epic C (Global Intelligence Layer):

| # | Task | Scope | Estimate | Dependencies |
|---|------|-------|----------|-------------|
| C-01 | Define entity and knowledge relationship schema | DEFINE TABLE entity, DEFINE TABLE knows TYPE RELATION, extended ontology (16 entity types, 18 relationship types) | S | Epic 0 complete |
| C-02 | Implement extraction queue with DEFINE EVENT | INGEST pipeline: event triggers on task/session/memory writes | S | C-01 |
| C-03 | Build LLM entity extraction module | COGNIFY step: prompt engineering + entity parsing, hybrid ONNX + Claude CLI approach | M | C-01 |
| C-04 | Build LLM relationship extraction module | COGNIFY step: relationship inference from text + entities | M | C-03 |
| C-05 | Implement entity deduplication | Hash-based + embedding similarity dedup | S | C-03 |
| C-06 | Build embedding generation pipeline | ONNX integration for entity embeddings (local-only, 384-dim all-MiniLM-L6-v2) | M | C-01 |
| C-07 | Implement hybrid search (GRAPH_COMPLETION) | Vector hint + graph expansion + combined ranking | M | C-04, C-06 |
| C-08 | Build MEMIFY refinement pipeline | Edge reweighting, stale pruning, transitive inference | L | C-04, C-06 |
| C-09 | Cross-project entity linking | Global daemon entity dedup across projects | M | C-05 |
| C-10 | Ontology configuration system | Configurable entity/relationship types per project, codeflow-knowledge.toml | S | C-03 |
| C-11 | Implement Loro CRDT knowledge graph sync | Extend Loro infrastructure for KG entity/relationship sync across developers (teams < 15) | M | C-01, Epic A Loro foundation |
| C-12 | Build configuration system (codeflow-knowledge.toml) | Sources, pruning, steering, sync mode configuration | S | C-01 |
| C-13 | Implement bootstrap CLI command | `codeflow knowledge bootstrap` with full/incremental/since options, 4-phase strategy | M | C-02, C-03, C-06 |
| C-14 | Implement trigger points and scheduling | SessionEnd hook cognify, PF6-COMPLETE trigger, daemon idle processing, manual CLI | M | C-02, C-03 |

---

## 6. Research References

### SurrealDB

| Title | URL |
|-------|-----|
| SurrealDB Graph Data Model | https://surrealdb.com/docs/surrealdb/models/graph |
| SurrealDB vs Neo4j | https://surrealdb.com/blog/surrealdb-vs-neo4j |
| SurrealDB Knowledge Graph Solutions | https://surrealdb.com/solutions/knowledge-graphs |
| SurrealDB DEFINE EVENT | https://surrealdb.com/docs/surrealql/statements/define/event |
| SurrealDB RELATE Statement | https://surrealdb.com/docs/surrealql/statements/relate |
| Knowledge Graph RAG Patterns | https://surrealdb.com/blog/knowledge-graph-rag-two-query-patterns-for-smarter-ai-agents |
| SurrealDB Live Queries | https://surrealdb.com/blog/unlocking-streaming-data-magic-with-surrealdb-live-queries-and-change-feeds |
| DEFINE RELATION Feature Request | https://github.com/surrealdb/surrealdb/issues/1424 |
| SurrealDB Graph Traversal & Recursion | https://surrealdb.com/blog/data-analysis-using-graph-traversal-recursion-and-shortest-path |
| SurrealDB vs Neo4j Comparison Page | https://surrealdb.com/comparison/neo4j |
| SurrealDB Fundamentals: Graph Relations | https://surrealdb.com/learn/fundamentals/relationships/graph-relations |
| Enhancing RAG with SurrealDB | https://surrealdb.com/blog/enhancing-retrieval-augmented-generation-with-surrealdb |

### Neo4j

| Title | URL |
|-------|-----|
| Neo4j LLM Knowledge Graph Builder | https://neo4j.com/labs/genai-ecosystem/llm-graph-builder/ |
| LLM Knowledge Graph Builder -- First Release of 2025 | https://neo4j.com/blog/developer/llm-knowledge-graph-builder-release/ |
| Creating Knowledge Graphs from Unstructured Data | https://neo4j.com/developer/genai-ecosystem/importing-graph-from-unstructured-data/ |
| Entity Linking and Relationship Extraction with Relik | https://neo4j.com/blog/developer/entity-linking-relationship-extraction-relik-llamaindex/ |

### Cognee

| Title | URL |
|-------|-----|
| Cognee GitHub Repository | https://github.com/topoteretes/cognee |
| Cognee AI Memory Architecture | https://www.cognee.ai/blog/fundamentals/how-cognee-builds-ai-memory |
| Cognee GraphRAG Deep Dive | https://www.cognee.ai/blog/deep-dives/cognee-graphrag-supercharging-search-with-knowledge-graphs-and-vector-magic |
| Cognee Memify Pipeline | https://www.cognee.ai/blog/cognee-news/product-update-memify |
| Cognee Self-Improving Memory (Memgraph) | https://memgraph.com/blog/from-rag-to-graphs-cognee-ai-memory |
| Cognee Documentation: Overview | https://docs.cognee.ai/core-concepts/overview |
| Cognee Building Blocks of Knowledge Graphs | https://www.cognee.ai/blog/fundamentals/building-blocks-of-knowledge-graphs |

### Multi-User Sync

| Title | URL |
|-------|-----|
| SurrealDB Pricing | https://surrealdb.com/pricing |
| TiKV (Apache 2.0, CNCF Graduated) | https://tikv.org/ |
| SurrealDB Running with TiKV | https://surrealdb.com/docs/surrealdb/installation/running/tikv |
| TiKV GitHub Repository | https://github.com/tikv/tikv |
| Loro CRDT | https://loro.dev/ |
| Loro API Documentation | https://docs.rs/loro/ |
| Loro GitHub Repository | https://github.com/loro-dev/loro |

### Academic & Industry

| Title | URL |
|-------|-----|
| Knowledge Graph Construction with LLMs (Nature, 2026) | https://www.nature.com/articles/s41598-026-38066-w |
| Entity Relationship Extraction with GNNs (Nature, 2025) | https://www.nature.com/articles/s41598-025-33922-7 |
| SurrealDB in 2025: Comparative Analysis | https://caperaven.co.za/2025/04/01/surrealdb-in-2025-a-comparative-analysis-across-database-categories-briefing-document/ |

---

## 7. Open Questions

| # | Question | Status | Impact | Blocker For | Answer |
|---|----------|--------|--------|-------------|--------|
| 1 | **LLM model selection for entity extraction**: Use ONNX local model (fast, free, limited), Claude API (high quality, cost), or hybrid (ONNX for embeddings, Claude for extraction)? | **ANSWERED** | Architecture | C-03, C-04 | **Hybrid approach.** ONNX (always, free) for embedding generation (all-MiniLM-L6-v2), basic NER, and chunking. Claude/Codex CLI (idle-time, paid) for complex entity extraction, relationship inference, and ontology mapping. Configuration-driven: user chooses which LLM to use or disables paid extraction entirely. CLI tools: `claude -p "prompt"` (headless), `codex -q "prompt"` (quiet mode), `gemini -p "prompt"`. See [Section 9: Trigger Points and Scheduling](#9-trigger-points-and-scheduling). |
| 2 | **Session hook integration points**: Where in the PathFlow pipeline should COGNIFY be triggered? After WS-DEV? After PF6-COMPLETE? On idle? | **ANSWERED** | Integration | C-02 | **Multiple trigger points.** Real-time: SurrealDB `DEFINE EVENT ASYNC` on record writes queues INGEST. Per-work-completion: `complete-work` ledger event queues extraction. Per-session: SessionEnd hook runs `codeflow hooks session-end cognify` batch. Per-tracked-session: PF6-COMPLETE triggers COGNIFY on session artifacts. Continuous: daemon idle-time polls extraction_queue. Scheduled: daemon or cron/launchd runs MEMIFY every 6-24 hours. Manual: `codeflow knowledge cognify` / `codeflow knowledge memify`. Per-commit: post-commit hook queues changed files. See [Section 9: Trigger Points and Scheduling](#9-trigger-points-and-scheduling). |
| 3 | **Configuration-driven scope**: Should knowledge graph construction include all files/docs, or be configurable per project? What's the default? | **ANSWERED** | Scope | C-10 | **Fully configurable via `codeflow-knowledge.toml`.** Default: ledger, tasks, epics, sessions enabled. Code files configurable via glob patterns with exclude patterns. Docs configurable. Git commits/PRs configurable with time window. Agent definitions opt-in. Pruning, steering (boost/suppress entity types), and custom ontology extensions all configurable. See [Section 10: Configuration](#10-configuration). |
| 4 | **Initial knowledge graph bootstrap**: Full project scan on first enable (expensive) vs incremental build from new sessions only? | **ANSWERED** | Migration | C-02 | **Four-phase strategy.** Phase 1: Initial bootstrap via `codeflow knowledge bootstrap [--since "2024-01-01"] [--full]` — scan git history, process all markdown, extract from code files, build initial graph, generate embeddings, run MEMIFY. 5-30 minutes depending on repo size. Phase 2: Incremental per-session — `complete-work` triggers INGEST, SessionEnd runs COGNIFY batch. Phase 3: Incremental per-commit — post-commit hook queues changed files for daemon processing. Phase 4: Periodic MEMIFY maintenance (6-24 hour schedule). See [Section 11: Bootstrap and Incremental Strategy](#11-bootstrap-and-incremental-strategy). |
| 5 | **Multi-user multi-machine synchronization**: How does the global daemon handle multiple developers' local ledgers? CRDT compatibility? | **ANSWERED** | Scale | C-09 | **Loro CRDT for small/medium teams (< 15 devs), TiKV for large/enterprise (15+).** TiKV is free (Apache 2.0) but requires massive infrastructure (7+ servers, 16+ cores each, 32+ GB RAM, NVMe). Loro CRDT extends the already-designed coordination infrastructure (Decision #8) to handle KG entity/relationship sync. Entities and relationships are small records (< 1KB) — perfect for CRDT. Embeddings are local-only (too large for CRDT, regenerated per machine via ONNX). No new infrastructure needed for teams < 15 devs. See [Section 8: Multi-User Synchronization Architecture](#8-multi-user-synchronization-architecture). |
| 6 | **Extended ontology for development domain**: Are the 8 entity types and 8 relationship types sufficient, or do we need domain extension points? | **ANSWERED** | Quality | C-10 | **Extended to 16 entity types and 18 relationship types.** Added: Requirement, TestCase, Deployment, Incident, Convention, Lesson, API, Milestone (entities). Added: implements, tested_by, deployed_to, owns, evolved_from, validates, learned_from, replaces, conflicts_with, references (relationships). Configuration supports `custom_entity_types` and `custom_relationship_types` for project-specific extensions. See [Section 3.2: CodeFlow Ontology](#32-codeflow-ontology). |
| 7 | **Graph algorithm library**: Without a GDS equivalent, how do we implement community detection and PageRank-style importance ranking? | **OPEN** | MEMIFY | C-08 | -- |
| 8 | **Cost control for LLM extraction**: At what scale does LLM entity extraction become cost-prohibitive, and when should we switch to local models? | **OPEN** | Operations | C-03 | -- |

---

## 8. Multi-User Synchronization Architecture

### TiKV Cost Analysis

**TiKV itself is 100% free** -- Apache 2.0 license, CNCF graduated project. No licensing cost.

**SurrealDB pricing tiers:**

| Tier | Cost | Includes |
|------|------|----------|
| Community (self-hosted) | Free | All core open-source features, single-node or multi-node clusters |
| Enterprise (self-hosted) | Custom (contact sales) | Priority security patches, audit logging, FIPS crypto, object storage, distributed live queries, SLA support |
| Cloud Free | $0/month | 1GB storage, 0.25 vCPU, 1GB memory |
| Cloud Start | From $0.021/hr | Single node up to 512GB storage, 16 vCPU, 64GB memory |
| Cloud Dedicated | Custom | Multi-node, up to 1PB, 64 vCPU/node, 256GB/node |

**Production TiKV infrastructure requirements:**

- Minimum: 3+ TiKV nodes + 3+ PD (Placement Driver) nodes + 1 monitoring node
- Per node: 16+ cores, 32+ GB RAM (no swap), 200+ GB NVMe/SSD, 10 Gigabit ethernet
- Total minimum footprint: ~7 servers with high-end specs
- **This is massive overkill for a small development team (2-5 devs)**

**Key insight:** TiKV is free software but the INFRASTRUCTURE COST is substantial. For CodeFlow's target audience (small dev teams, indie developers), running a 7-node TiKV cluster is impractical and expensive.

### Loro CRDT as Primary Sync Mechanism

**Loro capabilities confirmed:**

- 6 container types: LoroText, LoroList, LoroMap, LoroTree, LoroMovableList, LoroCounter
- Graph-like structures via Map of Maps or Tree containers
- Sync via export/import with delta updates and version vectors
- Each peer maintains version vector mapping peer IDs to operation counters
- Deterministic merge -- same operations in any order produce identical result
- Rust-native crate (already planned for CodeFlow)

**Already planned in crdt-coordination.md:**

- Same-machine: shared file (state.loro symlinked into worktrees)
- Multi-machine: git ref sync (refs/coordination/loro/{peer-id}, 10-30s interval)

**Extension for Knowledge Graph Sync:**

Instead of TiKV for multi-user KG sync, use the SAME Loro infrastructure already designed for coordination:

| KG Component | Loro Representation | Sync Via |
|-------------|--------------------| ---------|
| Entity records | LoroMap entries (entity_id -> {type, name, description, hash, source_records}) | Loro delta sync |
| Relationships | LoroMap entries (edge_id -> {from, to, relationship_type, confidence}) | Loro delta sync |
| Entity embeddings | LOCAL ONLY -- too large for CRDT, regenerated per machine from shared entity text | Not synced |
| Extraction queue | LoroList (pending extractions) | Loro delta sync |
| Graph structure metadata | LoroMap (graph stats, last_memify, ontology version) | Loro delta sync |

**Why this works:**

1. Entities and relationships are small records (< 1KB each) -- perfect for CRDT
2. Embeddings are derived from entity text -- each machine can regenerate locally via ONNX
3. The Loro transport (git ref sync) is already designed and needs zero additional infrastructure
4. Deterministic merge means no conflict resolution needed
5. Works offline -- local edits sync when connected
6. No TiKV cluster needed for teams < 10 devs

### Team Size Decision Matrix

| Team Size | KG Sync Mechanism | Database |
|-----------|-------------------|----------|
| Solo | N/A -- single embedded DB | surrealkv:// |
| Small team (2-5) | Loro CRDT via git refs | Each dev: local surrealkv://, KG entities synced via Loro |
| Medium team (5-15) | Loro CRDT + optional daemon | Local surrealkv:// + optional daemon for shared queries |
| Large team (15+) | SurrealDB TiKV cluster (self-hosted or cloud) | Shared TiKV-backed SurrealDB |
| Enterprise (50+) | SurrealDB Cloud Dedicated | Managed multi-node cluster |

**TiKV is ONLY needed at 15+ developers.** Below that, Loro CRDT handles sync with zero infrastructure cost.

### Why Embeddings Are Local-Only

Embeddings are NOT synced via Loro or any other mechanism. Each machine regenerates embeddings locally:

1. **Size:** A 384-dim float32 embedding is 1.5KB per entity. With thousands of entities, embedding sync would dominate CRDT bandwidth.
2. **Determinism:** The same ONNX model (all-MiniLM-L6-v2) with the same input text produces identical embeddings. No need to sync what can be computed.
3. **Privacy:** Embeddings can encode sensitive information about code structure. Local-only avoids transmitting them.
4. **Regeneration cost:** ~5-10ms per entity via ONNX. Regenerating 10,000 entities takes ~50-100 seconds -- acceptable on first sync.

### Architecture Diagram: Loro-Based KG Sync

```text
Developer A                          Developer B
============                         ============

surrealkv://                         surrealkv://
  |                                    |
  v                                    v
Local KG                             Local KG
(entities + relationships            (entities + relationships
 + embeddings)                        + embeddings)
  |                                    |
  v                                    v
Loro KG Doc                          Loro KG Doc
(entities + relationships            (entities + relationships
 -- NO embeddings)                    -- NO embeddings)
  |                                    |
  +--- git ref sync (30s) ------------>+
  +<--- git ref sync (30s) -----------+
  |                                    |
  v                                    v
On sync: merge Loro doc              On sync: merge Loro doc
-> upsert new entities to local DB   -> upsert new entities to local DB
-> regenerate embeddings for          -> regenerate embeddings for
   new/updated entities (ONNX)           new/updated entities (ONNX)
```

---

## 9. Trigger Points and Scheduling

### COGNIFY/MEMIFY Trigger Matrix

| Trigger Point | Mechanism | Process | When |
|--------------|-----------|---------|------|
| Record write | SurrealDB `DEFINE EVENT ASYNC` on task/session/memory tables | INGEST (queue for extraction) | Real-time |
| `complete-work` event | cf-knowledge-layer writes ledger event -> triggers queue entry | INGEST | Every work completion |
| SessionEnd hook | `codeflow hooks session-end cognify` CLI subcommand | COGNIFY batch (process pending queue) | Every session end |
| PF6-COMPLETE | After PR merge, before PF7-END | COGNIFY (session artifacts) | Per tracked session |
| Daemon idle time | Background daemon polls extraction_queue | COGNIFY (continuous) | When daemon idle |
| Scheduled timer | Daemon or cron/launchd | MEMIFY (prune, reweight, infer) | Every 6-24 hours |
| Manual CLI | `codeflow knowledge cognify` / `codeflow knowledge memify` | On-demand | User-initiated |
| Commit hook | Post-commit -> queue changed files for extraction | INGEST | Every commit |

### LLM CLI Tools for Background Entity Extraction

| Tool | Command | Mode | Cost |
|------|---------|------|------|
| Claude Code | `claude -p "prompt"` | Headless, single prompt, exits | Per-token (API) or subscription |
| Claude Code | `claude -p "prompt" --allowedTools Read --output-format json` | With tool access + JSON output | Same |
| OpenAI Codex CLI | `codex -q "prompt"` | Quiet mode, no interactive UI | Per-token (API) |
| Gemini CLI | `gemini -p "prompt"` | Same headless pattern | Per-token (API) |
| ONNX local models | `ort` crate in Rust | In-process, no CLI needed | Free (local compute) |

### Recommended Hybrid Approach

- **ONNX (always, free):** Embedding generation (all-MiniLM-L6-v2), basic NER, chunking
- **Claude/Codex CLI (idle-time, paid):** Complex entity extraction, relationship inference, ontology mapping
- **Configuration-driven:** User chooses which LLM to use, or disables paid extraction entirely

---

## 10. Configuration

### `codeflow-knowledge.toml`

```toml
[knowledge]
enabled = true
extraction_model = "onnx"  # "onnx" | "claude" | "codex" | "gemini" | "none"
extraction_model_paid = "claude"  # For complex extraction (idle-time only)

[knowledge.sources]
ledger = true           # Always (core)
tasks = true            # Always (core)
epics = true            # Always (core)
sessions = true         # Session summaries
code_files = true       # Source code
code_patterns = ["src/**/*.rs", "codeflow-rs/**/*.rs"]
code_exclude = ["**/target/**", "**/node_modules/**"]
docs = true             # Documentation markdown
doc_patterns = ["docs/**/*.md", "*.md", ".codeflow/docs/**/*.md"]
git_commits = true      # Commit messages
git_prs = true          # PR descriptions (requires GitHub API)
git_since = "6 months"  # Only commits within this window
agent_definitions = false  # .claude/agents/*.md (opt-in)

[knowledge.pruning]
max_age_days = 365
confidence_threshold = 0.3
stale_check_interval = "7d"
archive_after_days = 730  # Move to cold storage

[knowledge.steering]
boost = ["architecture_decisions", "design_patterns", "lessons_learned"]
suppress = ["typo_fixes", "formatting_changes", "version_bumps"]
custom_entity_types = []  # User-defined extensions to base ontology
custom_relationship_types = []  # User-defined extensions

[knowledge.sync]
mode = "loro"  # "loro" | "tikv" | "none"
sync_interval = "30s"
sync_embeddings = false  # Embeddings regenerated locally
```

### Configuration Sections Explained

| Section | Purpose | Default |
|---------|---------|---------|
| `knowledge` | Master enable/disable and model selection | enabled=true, onnx for free extraction |
| `knowledge.sources` | What data feeds the knowledge graph | All core sources enabled, code/docs configurable via globs |
| `knowledge.pruning` | When to prune stale entities and low-confidence edges | 365-day max age, 0.3 confidence threshold |
| `knowledge.steering` | Boost/suppress specific entity types in extraction and search ranking | Boost architecture decisions and lessons, suppress trivial changes |
| `knowledge.sync` | Multi-user sync mechanism selection | Loro CRDT with 30s interval, embeddings local-only |

---

## 11. Bootstrap and Incremental Strategy

### Phase 1 -- Initial Bootstrap

```bash
codeflow knowledge bootstrap [--since "2024-01-01"] [--full]
```

1. Scan all git commits (since date or full history)
2. Process all existing markdown (epics, tasks, analysis docs, READMEs)
3. Extract entities from code files matching configured patterns
4. Build initial graph with relationships
5. Generate embeddings for all entities
6. Run MEMIFY once to consolidate (prune duplicates, compute initial edge weights)
7. Estimated time: 5-30 minutes depending on repository size and LLM choice

### Phase 2 -- Incremental Per-Session

- Each `complete-work` event triggers INGEST for the task's changed files
- SessionEnd hook runs COGNIFY batch on accumulated queue
- Changed files are re-extracted; entities from those files are updated (not duplicated, hash-based dedup)
- New relationships merged with existing graph (additive, not destructive)

### Phase 3 -- Incremental Per-Commit

- Post-commit hook queues changed files (git diff) for extraction
- Daemon processes queue during idle time
- Only modified files re-processed (hash comparison)

### Phase 4 -- Periodic Maintenance

- MEMIFY runs on schedule (every 6-24 hours via daemon or launchd/systemd timer)
- Prunes orphaned entities, reweights edges, runs transitive inference
- Community detection runs weekly (expensive operation)
