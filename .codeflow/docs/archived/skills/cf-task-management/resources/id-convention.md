# ID Convention Reference

## Dual-ID System

Every epic and task has two identifiers:

| ID Type | Format | Purpose | Example |
|---------|--------|---------|---------|
| **ULID PK** (`id`) | `epic-{ulid}` / `task-{ulid}` | Database primary key, FK references, joins | `epic-01JQ3KM7V8...`, `task-01JQ3KN2X9...` |
| **Format ID** (`format_id`) | `{AREA}-EPC-{NNN}` / `{AREA}-TSK-{NNN}-{NNN}` | User-facing display, filenames, commits, branches | `INF-EPC-001`, `INF-TSK-001-003` |

### Usage Rules

| Context | Use Which | Why |
|---------|-----------|-----|
| Database FK references | ULID PK (`id`) | Immutable, globally unique |
| `epic_id` / `task_id` columns in tables | ULID PK | Foreign key integrity |
| active-task.json `task_id`, `epic_id` | ULID PK | Internal state references |
| User-facing display | Format ID (`format_id`) | Human-readable |
| Filenames and paths | Format ID | Navigable, meaningful |
| Commit messages and branches | Format ID | Human context in git history |
| API responses | Both | Include `id` (ULID PK) and `format_id` |

## Format ID: Epic

```text
{AREA}-EPC-{NNN}
```

### Components

| Component | Values | Example |
|-----------|--------|---------|
| AREA | FRT, BKD, INF, SHR, DOC, PLN | INF |
| EPC | Literal "EPC" for epic | EPC |
| NNN | Zero-padded sequence number | 001 |

Note: work_type and domain are metadata fields in YAML frontmatter, NOT part of the format ID.

### Examples

- `INF-EPC-001` - Infrastructure Epic #1
- `DOC-EPC-001` - Documentation Epic #1
- `PLN-EPC-001` - Planning Epic #1

## Format ID: Task

```text
{AREA}-TSK-{NNN}-{NNN}
```

### Components

| Component | Values | Example |
|-----------|--------|---------|
| AREA | FRT, BKD, INF, SHR, DOC, PLN | INF |
| TSK | Literal "TSK" for task | TSK |
| NNN (first) | Epic number (zero-padded) | 001 |
| NNN (second) | Task sequence within epic (zero-padded) | 003 |

### Examples

- `INF-TSK-001-003` - Task #3 under INF-EPC-001
- `DOC-TSK-001-001` - Task #1 under DOC-EPC-001
- `PLN-TSK-001-002` - Task #2 under PLN-EPC-001

## Sequence Number Rules

1. Epic sequence numbers are scoped per AREA
2. Always zero-padded to 3 digits (001, 002, ... 999)
3. Never reuse deleted IDs within same scope
4. Use `cf-db-operations:task-create` which handles sequencing automatically

## Generating IDs

**Do not construct IDs manually.** Always use the appropriate operation:

- **ULID PK** (`id`): Generated automatically — `epic-{ulid}` or `task-{ulid}`
- **Format ID** (`format_id`): Generated automatically — `{AREA}-EPC-{NNN}` or `{AREA}-TSK-{NNN}-{NNN}`

Both are produced by:

- Epic: `cf-db-operations:epic-create` generates both `id` and `format_id`
- Task: `cf-db-operations:task-create` generates both `id` and `format_id`

These operations use `.codeflow/scripts/db/generate-id.sh` internally.

## Area-to-Folder Mapping

When resolving area codes to filesystem paths, use this mapping:

| Area Code | Folder Name | Path Example |
|-----------|-------------|--------------|
| FRT | FRT/ | project-management/epics/FRT/ |
| BKD | BKD/ | project-management/epics/BKD/ |
| INF | INF/ | project-management/epics/INF/ |
| SHR | SHR/ | project-management/epics/SHR/ |
| DOC | DOC/ | project-management/epics/DOC/ |
| PLN | PLN/ | project-management/epics/PLN/ |

## Validation

To validate an existing ID format, use:

```bash
.codeflow/scripts/db/validate-id.sh "{id}"
```

Returns exit code 0 if valid, 1 if invalid.
