# Epics

Epics are organized by area type. Each area has a corresponding folder named by area code.

## Structure

```text
epics/
  {AREA}/                          # Area folder (e.g., INF/, DOC/, PLN/)
    {format_id}/                   # Epic folder (named by format_id)
      {format_id}.md               # Epic definition
        # frontmatter: id: epic-{ulid} (ULID PK), format_id: {AREA}-EPC-{NNN}
      tasks/                       # Task files
        {format_id}.md             # Task definition
          # frontmatter: id: task-{ulid} (ULID PK), format_id: {AREA}-TSK-{NNN}-{NNN}
```

File names use format_id (human-readable). The ULID primary key is stored in the frontmatter `id` field.

## Area Folders

| Folder | Area Code | Scope |
|--------|-----------|-------|
| `FRT/` | FRT | UI, components, client logic |
| `BKD/` | BKD | API, services, server logic |
| `INF/` | INF | CI/CD, deployment, DevOps, config, infrastructure |
| `SHR/` | SHR | Common libraries, types, utilities |
| `DOC/` | DOC | Docs, guides, ADRs |
| `PLN/` | PLN | Planning sessions, spikes, investigations |

## YAML Frontmatter

Each epic and task markdown file begins with YAML frontmatter containing:

- `id` — ULID primary key (e.g., `epic-01KHSQPQRNQP0XTXCRHXX9YW1T`)
- `format_id` — Human-readable ID (e.g., `INF-EPC-005`, `INF-TSK-005-001`)
- `title`, `status`, `area_type`, `work_type`, and other metadata fields

The `id` field is the authoritative primary key. The `format_id` is for human readability and file naming.

## Ongoing Epics

Some epics are ongoing (`is_ongoing: true`) and should be reused, not duplicated:

- `PLN-EPC-001` — All planning work (add tasks here, don't create new PLN epics)
- `DOC-EPC-001` — Documentation updates (add tasks here, don't create new DOC epics)
- Created on-demand when informal work is first registered
- Check existing epics before creating new ones

## Templates

Canonical templates are at [`../templates/epic-template.md`](../templates/epic-template.md) and [`../templates/task-template.md`](../templates/task-template.md).
