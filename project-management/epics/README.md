# Epics

Epics are organized by area type. Each area has a corresponding folder.

## Structure

```text
epics/
  {area}/                          # Area folder (e.g., frontend/, backend/)
    {FORMAT-ID}/                   # Epic folder (named by format_id)
      {FORMAT-ID}-epic.md          # Epic definition
        # frontmatter: id: epic-{ulid} (ULID PK), format_id: {FORMAT-ID}
      tasks/                       # Task files
        {FORMAT-ID}.md             # Task definition
          # frontmatter: id: task-{ulid} (ULID PK), format_id: {FORMAT-ID}
```

File names use format_id (human-readable). The ULID primary key is stored in the frontmatter `id` field.

## Area Folders

| Folder | Area Code | Scope |
|--------|-----------|-------|
| `frontend/` | FRT | UI, components, client logic |
| `backend/` | BKD | API, services, server logic |
| `infrastructure/` | INF | CI/CD, deployment, DevOps |
| `shared/` | SHR | Common libraries, types |
| `documentation/` | DOC | Docs, guides, ADRs |
| `cross-cutting/` | XCUT | Multi-area work |

## Ongoing Epics

Each area+work_type combination can have one ongoing epic:

- `{AREA}-EPC-{WORK}-GENL-001` (e.g., `INF-EPC-FEAT-GENL-001`)
- Created on-demand when informal work is first registered
- Used as catch-all containers for ad-hoc work
