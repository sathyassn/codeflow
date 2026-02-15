# ID Convention Reference

## Dual-ID System

Every epic and task has two identifiers:

| ID Type | Format | Purpose | Example |
|---------|--------|---------|---------|
| **ULID PK** (`id`) | `epic-{ulid}` / `task-{ulid}` | Database primary key, FK references, joins | `epic-01JQ3KM7V8...`, `task-01JQ3KN2X9...` |
| **Format ID** (`format_id`) | `{AREA}-EPC-{TYPE}-{DOMAIN}-{NNN}` / `{AREA}-TSK-{TYPE}-{DOMAIN}-{NNN}` | User-facing display, filenames, commits, branches | `FRT-EPC-FEAT-AUTH-001`, `BKD-TSK-FIX-API-023` |

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
{AREA}-EPC-{TYPE}-{DOMAIN}-{NNN}
```

### Components

| Component | Values | Example |
|-----------|--------|---------|
| AREA | FRT, BKD, INF, SHR, DOC, XCUT | FRT |
| EPC | Literal "EPC" for epic | EPC |
| TYPE | FEAT, FIX, RFCT, DOCS, TEST, HTFX, CHOR, CICD, SPKE | FEAT |
| DOMAIN | Project-configured or GENL | AUTH |
| NNN | Zero-padded sequence number | 001 |

### Examples

- `FRT-EPC-FEAT-AUTH-001` - Frontend feature epic for auth domain
- `BKD-EPC-FIX-API-003` - Backend fix epic for API domain
- `INF-EPC-CICD-GENL-001` - Infrastructure CI/CD epic (general)

## Format ID: Task

```text
{AREA}-TSK-{TYPE}-{DOMAIN}-{NNN}
```

### Components

| Component | Values | Example |
|-----------|--------|---------|
| AREA | FRT, BKD, INF, SHR, DOC, XCUT | BKD |
| TSK | Literal "TSK" for task | TSK |
| TYPE | FEAT, FIX, RFCT, DOCS, TEST, HTFX, CHOR, CICD, SPKE | FEAT |
| DOMAIN | Project-configured or GENL | API |
| NNN | Zero-padded sequence number | 012 |

### Examples

- `BKD-TSK-FEAT-API-012` - Backend feature task for API domain
- `FRT-TSK-FIX-UI-005` - Frontend fix task for UI domain
- `SHR-TSK-RFCT-GENL-001` - Shared refactor task (general)

## Sequence Number Rules

1. Sequence numbers are scoped per AREA-TYPE-DOMAIN combination
2. Always zero-padded to 3 digits (001, 002, ... 999)
3. Never reuse deleted IDs within same scope
4. Use `cf-db-operations:task-create` which handles sequencing automatically

## Generating IDs

**Do not construct IDs manually.** Always use the appropriate operation:

- **ULID PK** (`id`): Generated automatically — `epic-{ulid}` or `task-{ulid}`
- **Format ID** (`format_id`): Generated automatically — `{AREA}-EPC-{TYPE}-{DOMAIN}-{NNN}` or `{AREA}-TSK-{TYPE}-{DOMAIN}-{NNN}`

Both are produced by:

- Epic: `cf-db-operations:epic-create` generates both `id` and `format_id`
- Task: `cf-db-operations:task-create` generates both `id` and `format_id`

These operations use `.codeflow/scripts/db/generate-id.sh` internally.

## Area-to-Folder Mapping

When resolving area codes to filesystem paths, use this mapping:

| Area Code | Folder Name | Path Example |
|-----------|-------------||--------------|
| FRT | frontend/ | project-management/epics/frontend/ |
| BKD | backend/ | project-management/epics/backend/ |
| INF | infrastructure/ | project-management/epics/infrastructure/ |
| SHR | shared/ | project-management/epics/shared/ |
| DOC | documentation/ | project-management/epics/documentation/ |
| XCUT | cross-cutting/ | project-management/epics/cross-cutting/ |

## Validation

To validate an existing ID format, use:

```bash
.codeflow/scripts/db/validate-id.sh "{id}"
```

Returns exit code 0 if valid, 1 if invalid.
