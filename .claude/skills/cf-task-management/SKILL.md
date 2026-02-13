---
name: cf-task-management
description: Provides task-centric workflow operations including classification, registration, and epic/task CRUD. Ensures all work flows through the task system with proper tracking. Use when creating or managing work items.
context: fork
agent: cf-general-purpose
---

# Task Management Skill

## Type

**Procedural** - Provides step-by-step task and epic management operations.

## Purpose

**Ensures all work flows through the task system with task_id NOT NULL enforcement, enabling proper tracking and organization.**

## Responsibilities

- Parse and understand user requests
- Classify work by area, type, and domain
- Ensure work is registered before modifications
- Create and update epics (cf-planner)
- Create and update tasks
- Query tasks by various criteria
- NOT: Work lifecycle (that's cf-memory-management)
- NOT: Database operations (that's cf-db-operations)

## Decision Tree

```text
Receiving planning request?
└── 🔧 understand-request (parse requirements)

Informal work request (no task_id)?
├── 🔧 classify-work (determine area/type/domain)
└── 🔧 ensure-work-registered (create task in ongoing epic)

Creating new epic?
└── 🔧 create-epic (cf-planner only)

Creating new task?
└── 🔧 create-task (linked to epic)

Updating work item?
├── Epic changes → 🔧 update-epic (cf-planner only)
└── Task changes → 🔧 update-task

Finding tasks?
└── 🔧 query-tasks (by status, epic, assignee)
```

## Operations

| # | Operation | Enforcement | Purpose |
|---|-----------|-------------|---------|
| 1 | understand-request | ENF-L3 Advisory | Parse request, extract requirements |
| 2 | classify-work | ENF-L3 Advisory | Determine area/type/domain for work |
| 3 | ensure-work-registered | ENF-L1 Sentinel | Guarantee task exists before modifications |
| 4 | create-epic | ENF-L3 Advisory | Create epic in work graph |
| 5 | update-epic | ENF-L3 Advisory | Modify epic status/content |
| 6 | create-task | ENF-L3 Advisory | Create task linked to epic |
| 7 | update-task | ENF-L3 Advisory | Modify task status/assignee |
| 8 | query-tasks | None | Query tasks by status/epic |

## Operation Details

### 🔧 understand-request

```text
When: Receiving planning request
Purpose: Parse user request and extract requirements
Enforcement: ENF-L3 Advisory

Procedure:
  1. Parse user request for:
     - Explicit requirements (stated directly)
     - Implicit requirements (inferred from context)
     - Constraints (boundaries, limitations)
     - Success criteria (how to verify)
  2. Identify ambiguities
  3. Ask clarifying questions if needed
  4. Summarize understanding back to user

Output: Requirements summary with questions if ambiguous

📚 Resource: [classification-guide.md](resources/classification-guide.md)
   Load when: Parsing complex requests or clarifying requirements
```

### 🔧 classify-work

```text
When: Informal work request (no task-id provided)
Purpose: Determine work classification for task-centric workflow
Enforcement: ENF-L3 Advisory

Procedure:
  1. Parse work description for keywords

  2. Determine area_type:
     | Code | Keywords | Example |
     |------|----------|---------|
     | FRT | frontend, UI, component, button | "Fix the login button" |
     | BKD | backend, API, endpoint, server | "Add user endpoint" |
     | INF | infra, deploy, CI, pipeline | "Update CI pipeline" |
     | SHR | shared, common, util, type | "Add date util" |
     | DOC | doc, readme, guide | "Update README" |
     | XCUT | cross-cutting (multiple areas) | "Refactor auth across app" |

  3. Determine work_type:
     | Code | Keywords | Example |
     |------|----------|---------|
     | FEAT | add, implement, create, new | "Add dark mode" |
     | FIX | fix, bug, broken, error | "Fix crash on login" |
     | RFCT | refactor, improve, clean | "Refactor auth module" |
     | DOCS | document, explain | "Document API" |
     | TEST | test, coverage | "Add unit tests" |
     | HTFX | hotfix, urgent, critical | "Urgent: fix payment" |
     | CHOR | chore, maintenance | "Update dependencies" |
     | CICD | pipeline, workflow, deploy | "Add staging deploy" |
     | SPKE | spike, research, explore | "Investigate options" |

  4. Determine domain:
     - Match against configured domains
     - Default to GENL if no match

Output:
  area_type: FRT | BKD | INF | SHR | DOC | XCUT
  work_type: FEAT | FIX | RFCT | DOCS | TEST | HTFX | CHOR | CICD | SPKE
  domain: {configured-domain} | GENL

Cross-skill: cf-db-operations:query-domain for domain lookup

📚 Resource: [classification-guide.md](resources/classification-guide.md)
   Load when: Unsure of area/type classification or keyword mapping
```

### 🔧 ensure-work-registered

```text
When: Informal work request (no task-id provided)
Purpose: Guarantee all work flows through task system (task_id NOT NULL)
Enforcement: ENF-L1 Sentinel - Required before any Edit/Write/Bash modifications

Prerequisite: classify-work (this skill) provides area_type, work_type, domain

ID Convention (Dual-ID):
  - id (ULID PK): epic-{ulid} / task-{ulid} — used for DB FK references
  - format_id: {AREA}-EPC-{TYPE}-{DOMAIN}-{NNN} / {AREA}-TSK-{TYPE}-{DOMAIN}-{NNN} — used for display, filenames

Procedure:
  1. Search for existing ongoing epic matching area_type + work_type
     - Ongoing epics are per area + work_type combination
     - e.g., format_id INF-EPC-FIX-GENL-001 for infrastructure fixes
     - e.g., format_id INF-EPC-FEAT-GENL-001 for infrastructure features

  2. If no ongoing epic found, create one:
     - id: epic-{ulid} (ULID PK, auto-generated)
     - format_id: {AREA}-EPC-{TYPE}-GENL-001
     - title: "Ongoing {Area} {Type}s"
     - is_ongoing: TRUE
     - status: 'in_progress'
     - file_path: project-management/epics/{area-folder}/{format_id}/{format_id}-epic.md

  3. Create task under epic:
     - epic_id: {ULID PK of found or created epic}
     - id: task-{ulid} (ULID PK, auto-generated)
     - format_id: {AREA}-TSK-{TYPE}-{DOMAIN}-{NNN}
     - title: {original work description}
     - origin: 'informal'
     - scope_policy: 'soft' (default for informal)
     - status: 'pending'

  4. Create active-task.json via cf-work-state.sh after task registration
     - Location: .state/runtime/active-task.json
     - Fields: task_id (ULID PK), epic_id (ULID PK), task_format_id, epic_format_id, title, status, branch, session_id

  5. Return task_id (ULID PK) for use in begin-work

Output:
  task_id: {ULID PK — task-{ulid}}
  epic_id: {ULID PK — epic-{ulid}}
  task_format_id: {display ID — {AREA}-TSK-{TYPE}-{DOMAIN}-{NNN}}
  epic_format_id: {display ID — {AREA}-EPC-{TYPE}-{DOMAIN}-{NNN}}
  origin: 'informal'
  is_new_epic: true | false

Hook: PreToolUse/pre-tool-use-task-sentinel.sh blocks without this

📚 Resource: [id-convention.md](resources/id-convention.md)
   Load when: Generating task/epic IDs or understanding ID format
```

### 🔧 create-epic

```text
When: /cf-plan creates new epic OR ensure-work-registered needs ongoing epic
Purpose: Create epic in work graph database
Enforcement: ENF-L3 Advisory (not in enforcement-policy.json; calls cf-db-operations internally)
Agent: cf-planner (full creation), Main Agent (ongoing epics only)

Procedure:
  1. Generate both IDs:
     - id (ULID PK): epic-{ulid} (auto-generated)
     - format_id: {AREA}-EPC-{TYPE}-{DOMAIN}-{NNN} (auto-generated)

  2. Validate required fields:
     - area_type: FRT | BKD | INF | SHR | DOC | XCUT
     - work_type: FEAT | FIX | RFCT | DOCS | TEST | HTFX | CHOR | CICD | SPKE
     - domain: configured domain or GENL
     - title: descriptive title

  3. Insert into database via cf-db-operations:epic-create

  4. Create markdown file: project-management/epics/{area-folder}/{format_id}/{format_id}-epic.md

  5. Return both IDs

Output:
  epic_id: {ULID PK — epic-{ulid}}
  epic_format_id: {display ID — {AREA}-EPC-{TYPE}-{DOMAIN}-{NNN}}
  file_path: project-management/epics/{area-folder}/{format_id}/{format_id}-epic.md

📚 Resource: [id-convention.md](resources/id-convention.md)
   Load when: Generating epic IDs or understanding ID structure
```

### 🔧 update-epic

```text
When: Epic status changes or content updates
Purpose: Modify epic status, priority, or metadata
Enforcement: ENF-L3 Advisory (not in enforcement-policy.json; calls cf-db-operations internally)
Agent: cf-planner only

Updatable Fields:
  | Field | cf-planner | Other Agents |
  |-------|------------|--------------|
  | status | Yes | No |
  | priority | Yes | No |
  | title | Yes | No |
  | description | Yes | No |
  | acceptance_criteria | Yes | No |

Procedure:
  1. Validate epic exists
  2. Validate field permissions for agent
  3. Update database via cf-db-operations:epic-update
  4. Re-render markdown file if content changed
  5. Return updated fields

📚 Resource: [task-lifecycle.md](resources/task-lifecycle.md)
   Load when: Understanding epic status transitions
```

### 🔧 create-task

```text
When: /cf-develop creates task OR ensure-work-registered creates informal task
Purpose: Create task linked to epic in work graph
Enforcement: ENF-L3 Advisory (not in enforcement-policy.json; calls cf-db-operations internally)
Agent: cf-planner (full), Main Agent (informal tasks)

Required Fields:
  - epic_id: parent epic (ULID PK — epic-{ulid})
  - title: task description
  - origin: 'formal' | 'informal'
  - scope_policy: 'soft' | 'hard' | 'permissive'

Optional Autorun Fields (cf-planner only):
  | Field | Type | Default | Purpose |
  |-------|------|---------|---------|
  | autorun_eligible | boolean | FALSE | Can run in autorun batch |
  | auto_commit | boolean | TRUE | Commit automatically on completion |
  | raise_pr | boolean | TRUE | Create PR after commit |
  | auto_merge | boolean | FALSE | Merge PR automatically |
  | target_branch | string | main | Branch to merge into |

Procedure:
  1. Validate epic_id (ULID PK) exists
  2. Generate both IDs:
     - id (ULID PK): task-{ulid} (auto-generated)
     - format_id: {AREA}-TSK-{TYPE}-{DOMAIN}-{NNN} (auto-generated)
  3. Insert into database via cf-db-operations:task-create
  4. Create markdown file: project-management/epics/{area-folder}/{epic-format_id}/tasks/{task-format_id}.md
  5. Return both IDs

Output:
  task_id: {ULID PK — task-{ulid}}
  task_format_id: {display ID — {AREA}-TSK-{TYPE}-{DOMAIN}-{NNN}}
  epic_id: {parent epic ULID PK}
  file_path: project-management/epics/{area-folder}/{epic-format_id}/tasks/{task-format_id}.md

Executed by: codeflow db exec (Go CLI; see .codeflow/scripts/db/schema.sql for schema)

📚 Resources:
   [id-convention.md](resources/id-convention.md) - Load when: Generating task IDs
   [task-lifecycle.md](resources/task-lifecycle.md) - Load when: Setting initial task status
```

### 🔧 update-task

```text
When: Task status changes, assignment changes, or progress updates
Purpose: Modify task status, assignee, or content
Enforcement: ENF-L3 Advisory (not in enforcement-policy.json; calls cf-db-operations internally)
Agent: All agents (status), cf-planner (all fields)

Updatable Fields:
  | Field | cf-planner | cf-developer | cf-qa | cf-reviewer |
  |-------|------------|--------------|-------|-------------|
  | status | Yes | Yes | Yes | Yes |
  | assignee | Yes | No | No | No |
  | priority | Yes | No | No | No |
  | file_scope | Yes | Yes* | No | No |
  | blockers | Yes | Yes | Yes | Yes |

*cf-developer can only expand scope per scope_policy

Procedure:
  1. Validate task exists
  2. Validate field permissions for agent
  3. Update database via cf-db-operations:task-update
  4. Re-render markdown file if content changed
  5. Check if status change unblocks other tasks
  6. Return updated fields

📚 Resource: [task-lifecycle.md](resources/task-lifecycle.md)
   Load when: Understanding valid status transitions or permission matrix
```

### 🔧 query-tasks

```text
When: Agent needs to find tasks by criteria
Purpose: Query tasks by status, epic, assignee, or area
Enforcement: None (read-only)
Agent: All agents

Query Parameters:
  | Parameter | Example | Purpose |
  |-----------|---------|---------|
  | epic_id | FRT-EPC-FEAT-AUTH-001 | Tasks in epic |
  | status | pending, in_progress | Filter by status |
  | assignee | cf-developer | Tasks for agent |
  | area_type | FRT | Frontend tasks |
  | work_type | FIX | Bug fixes only |
  | blocked | true | Tasks with blockers |
  | autorun_eligible | true | Autorun-ready tasks |

Autorun Query (ready for execution):
  - autorun_eligible = TRUE
  - status = 'todo'
  - No unresolved blocking dependencies

Output:
  tasks: [{id (ULID PK), format_id, title, status, epic_id (ULID PK), assignee, priority, autorun_eligible}...]
  count: {total matching}

Cross-skill: cf-db-operations (for query execution)
Executed by: codeflow db query (Go CLI; see .codeflow/scripts/db/schema.sql for schema)

📚 Resources:
   [classification-guide.md](resources/classification-guide.md) - Load when: Building query filters
   [task-lifecycle.md](resources/task-lifecycle.md) - Load when: Filtering by status
```

## Area-to-Folder Mapping

| Area Code | Folder Name |
|-----------|-------------|
| FRT | frontend/ |
| BKD | backend/ |
| INF | infrastructure/ |
| SHR | shared/ |
| DOC | documentation/ |
| XCUT | cross-cutting/ |

Used in paths: `project-management/epics/{folder}/...`

## ID Convention (Dual-ID)

Every epic and task has two IDs:

- **ULID PK** (`id`): `epic-{ulid}` / `task-{ulid}` — for DB FK references, internal lookups
- **Format ID** (`format_id`): `{AREA}-{ENTITY}-{TYPE}-{DOMAIN}-{NNN}` — for display, filenames, branches

```text
Format ID pattern: {AREA}-{ENTITY}-{TYPE}-{DOMAIN}-{NUMBER}

Examples:
  FRT-EPC-FEAT-AUTH-001  → Frontend Epic: Feature in Auth domain
  BKD-TSK-FIX-API-023    → Backend Task: Fix in API domain
  INF-EPC-CICD-GENL-001  → Infrastructure Epic: CI/CD general
```

## Resources

| Resource | Purpose | When to Load |
|----------|---------|--------------|
| [id-convention.md](resources/id-convention.md) | ID format rules | When generating IDs |
| [classification-guide.md](resources/classification-guide.md) | Area/type classification | When classifying work |
| [task-lifecycle.md](resources/task-lifecycle.md) | Status transitions | When updating tasks |
