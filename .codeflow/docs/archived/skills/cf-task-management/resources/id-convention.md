# ID Convention Reference

## Overview

CodeFlow uses a dual-ID system for epics and tasks. Every record has an internal ULID primary key for database operations and a human-readable format ID for display, filenames, and git history.

## Dual-ID System

| ID Type | Format | Purpose | Example |
|---------|--------|---------|---------|
| **ULID PK** (`id`) | `epic-{ulid}` / `task-{ulid}` | Database primary key, FK references, joins | `epic-01JQ3KM7V8...`, `task-01JQ3KN2X9...` |
| **Format ID** (`format_id`) | `{AREA}-EPC-{NNN}` / `{AREA}-TSK-{NNN}-{NNN}` | User-facing display, filenames, commits, branches | `INF-EPC-001`, `INF-TSK-001-003` |

### When to Use Which

| Context | Use | Why |
|---------|-----|-----|
| Database FK references | ULID PK (`id`) | Immutable, globally unique |
| `epic_id` / `task_id` columns | ULID PK | Foreign key integrity |
| active-task.json `task_id`, `epic_id` | ULID PK | Internal state references |
| User-facing display | Format ID | Human-readable |
| Filenames and paths | Format ID | Navigable, meaningful |
| Commit messages and branches | Format ID | Human context in git history |
| API responses | Both | Include `id` and `format_id` |

## Area Codes

Area codes classify work by system boundary. Each area maps to a top-level folder under `project-management/epics/`.

| Area Code | Description | Folder | Example Use |
|-----------|-------------|--------|-------------|
| FRT | Frontend | `project-management/epics/FRT/` | UI components, client-side logic |
| BKD | Backend | `project-management/epics/BKD/` | Server logic, APIs, data processing |
| INF | Infrastructure | `project-management/epics/INF/` | Build systems, CI/CD, tooling, config |
| SHR | Shared | `project-management/epics/SHR/` | Cross-cutting libraries, shared utilities |
| DOC | Documentation | `project-management/epics/DOC/` | Guides, references, ADRs |
| PLN | Planning | `project-management/epics/PLN/` | Design documents, architecture plans |

Work type and domain are **not** part of the format ID. They are stored as metadata fields (e.g., in YAML frontmatter) on the epic or task record.

## Format ID: Epic

```text
{AREA}-EPC-{NNN}
```

| Component | Description | Example |
|-----------|-------------|---------|
| AREA | Area code (see table above) | `INF` |
| EPC | Literal string indicating an epic | `EPC` |
| NNN | 3-digit zero-padded sequence number, scoped per area | `005` |

### Examples

| Format ID | Meaning |
|-----------|---------|
| `INF-EPC-001` | Infrastructure epic #1 |
| `INF-EPC-005` | Infrastructure epic #5 |
| `DOC-EPC-001` | Documentation epic #1 |
| `FRT-EPC-003` | Frontend epic #3 |
| `PLN-EPC-002` | Planning epic #2 |

## Format ID: Task

```text
{AREA}-TSK-{NNN}-{NNN}
```

| Component | Description | Example |
|-----------|-------------|---------|
| AREA | Area code (same as parent epic) | `INF` |
| TSK | Literal string indicating a task | `TSK` |
| NNN (first) | Parent epic sequence number | `005` |
| NNN (second) | Task sequence number within the epic | `003` |

A task ID encodes its parent epic: `INF-TSK-005-003` belongs to `INF-EPC-005` (task #3 of epic #5).

### Examples

| Format ID | Parent Epic | Meaning |
|-----------|-------------|---------|
| `INF-TSK-001-003` | `INF-EPC-001` | Task #3 under infrastructure epic #1 |
| `INF-TSK-005-003` | `INF-EPC-005` | Task #3 under infrastructure epic #5 |
| `DOC-TSK-001-001` | `DOC-EPC-001` | Task #1 under documentation epic #1 |
| `FRT-TSK-003-012` | `FRT-EPC-003` | Task #12 under frontend epic #3 |
| `PLN-TSK-002-001` | `PLN-EPC-002` | Task #1 under planning epic #2 |

## Sequence Number Rules

1. Epic sequence numbers are scoped per area (each area starts at 001)
2. Task sequence numbers are scoped per epic (each epic starts at 001)
3. Always zero-padded to 3 digits (001, 002, ... 999)
4. Never reuse deleted IDs within the same scope

## Generating IDs

Do not construct IDs manually. Use the database operations which handle sequencing automatically:

| Record Type | Operation | Produces |
|-------------|-----------|----------|
| Epic | `cf-db-operations:epic-create` | Both `id` (ULID PK) and `format_id` |
| Task | `cf-db-operations:task-create` | Both `id` (ULID PK) and `format_id` |

ID generation is handled internally by the database operations layer.

## Validation

Format ID validation is handled by the database layer, which checks structure and sequence integrity when IDs are referenced or created.
