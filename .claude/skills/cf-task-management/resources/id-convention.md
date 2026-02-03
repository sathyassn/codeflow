# ID Convention Reference

## Epic ID Format

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

### Example Epic IDs

- `FRT-EPC-FEAT-AUTH-001` - Frontend feature epic for auth domain
- `BKD-EPC-FIX-API-003` - Backend fix epic for API domain
- `INF-EPC-CICD-GENL-001` - Infrastructure CI/CD epic (general)

## Task ID Format

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

### Example Task IDs

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

- Epic IDs: `cf-db-operations:epic-create` generates the ID
- Task IDs: `cf-db-operations:task-create` generates the ID

These operations use `.codeflow/scripts/db/generate-id.sh` internally.

## Validation

To validate an existing ID format, use:

```bash
.codeflow/scripts/db/validate-id.sh "{id}"
```

Returns exit code 0 if valid, 1 if invalid.
