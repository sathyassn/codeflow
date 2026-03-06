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

#### Entity Types

| Entity Type | Description | Source | Example |
|-------------|-------------|--------|---------|
| Concept | Abstract technical concept | LLM extraction | "event-driven architecture", "CRDT consistency" |
| Decision | Architectural or design decision | ADR/planning docs | "chose SurrealDB over SQLite" |
| Pattern | Recurring code/design pattern | Code analysis | "newtype wrapper pattern", "builder pattern" |
| Component | System module or subsystem | Code structure | "PathFlow engine", "hook pipeline" |
| Problem | Bug, issue, or challenge | Task/issue text | "session ID duplication", "context overflow" |
| Solution | Resolution to a problem | Task completion text | "guard session creation behind source check" |
| Technology | External tool, library, or platform | Dependencies | "SurrealDB", "Loro CRDT", "ONNX Runtime" |
| Developer | Team member or contributor | Git history | "developer:alice" |

#### Relationship Types

| Relationship | From | To | Meaning | Example |
|-------------|------|-----|---------|---------|
| relates_to | Concept | Concept | Semantic association | "CRDT" relates_to "eventual consistency" |
| solved_by | Problem | Solution | Resolution link | "session duplication" solved_by "source guard" |
| caused_by | Problem | Component | Root cause | "context overflow" caused_by "lead context window" |
| uses_pattern | Component | Pattern | Implementation pattern | "type system" uses_pattern "newtype wrapper" |
| decided_in | Decision | session | Provenance | "chose SurrealDB" decided_in session:ses_abc |
| depends_on | task | task | Task dependency | task:009 depends_on task:006 |
| expert_in | Developer | Technology | Expertise signal | developer:alice expert_in "SurrealDB" |
| similar_to | any | any | Semantic similarity | pattern:A similar_to pattern:B |

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
| C-01 | Define entity and knowledge relationship schema | DEFINE TABLE entity, DEFINE TABLE knows TYPE RELATION | S | Epic 0 complete |
| C-02 | Implement extraction queue with DEFINE EVENT | INGEST pipeline: event triggers on task/session/memory writes | S | C-01 |
| C-03 | Build LLM entity extraction module | COGNIFY step: prompt engineering + entity parsing | M | C-01 |
| C-04 | Build LLM relationship extraction module | COGNIFY step: relationship inference from text + entities | M | C-03 |
| C-05 | Implement entity deduplication | Hash-based + embedding similarity dedup | S | C-03 |
| C-06 | Build embedding generation pipeline | ONNX integration for entity embeddings | M | C-01 |
| C-07 | Implement hybrid search (GRAPH_COMPLETION) | Vector hint + graph expansion + combined ranking | M | C-04, C-06 |
| C-08 | Build MEMIFY refinement pipeline | Edge reweighting, stale pruning, transitive inference | L | C-04, C-06 |
| C-09 | Cross-project entity linking | Global daemon entity dedup across projects | M | C-05 |
| C-10 | Ontology configuration system | Configurable entity/relationship types per project | S | C-03 |

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

### Academic & Industry

| Title | URL |
|-------|-----|
| Knowledge Graph Construction with LLMs (Nature, 2026) | https://www.nature.com/articles/s41598-026-38066-w |
| Entity Relationship Extraction with GNNs (Nature, 2025) | https://www.nature.com/articles/s41598-025-33922-7 |
| SurrealDB in 2025: Comparative Analysis | https://caperaven.co.za/2025/04/01/surrealdb-in-2025-a-comparative-analysis-across-database-categories-briefing-document/ |

---

## 7. Open Questions

The following questions remain OPEN and will be addressed in follow-up analysis:

| # | Question | Impact | Blocker For |
|---|----------|--------|-------------|
| 1 | **LLM model selection for entity extraction**: Use ONNX local model (fast, free, limited), Claude API (high quality, cost), or hybrid (ONNX for embeddings, Claude for extraction)? | Architecture | C-03, C-04 |
| 2 | **Session hook integration points**: Where in the PathFlow pipeline should COGNIFY be triggered? After WS-DEV? After PF6-COMPLETE? On idle? | Integration | C-02 |
| 3 | **Configuration-driven scope**: Should knowledge graph construction include all files/docs, or be configurable per project? What's the default? | Scope | C-10 |
| 4 | **Initial knowledge graph bootstrap**: Full project scan on first enable (expensive) vs incremental build from new sessions only? | Migration | C-02 |
| 5 | **Multi-user multi-machine synchronization**: How does the global daemon handle multiple developers' local ledgers? CRDT compatibility? | Scale | C-09 |
| 6 | **Extended ontology for development domain**: Are the 8 entity types and 8 relationship types sufficient, or do we need domain extension points? | Quality | C-10 |
| 7 | **Graph algorithm library**: Without a GDS equivalent, how do we implement community detection and PageRank-style importance ranking? | MEMIFY | C-08 |
| 8 | **Cost control for LLM extraction**: At what scale does LLM entity extraction become cost-prohibitive, and when should we switch to local models? | Operations | C-03 |
