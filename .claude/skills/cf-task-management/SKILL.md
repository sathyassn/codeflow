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
| 4 | create-epic | ENF-L1 Sentinel | Create epic in work graph |
| 5 | update-epic | ENF-L1 Sentinel | Modify epic status/content |
| 6 | create-task | ENF-L1 Sentinel | Create task linked to epic |
| 7 | update-task | ENF-L1 Sentinel | Modify task status/assignee |
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

Procedure:
  1. Search for existing ongoing epic matching area_type + work_type

  2. If no ongoing epic found, create one:
     - id: {AREA}-EPC-{TYPE}-GENL-001
     - title: "Ongoing {Area} {Type}s"
     - is_ongoing: TRUE
     - status: 'in_progress'

  3. Create task under epic:
     - epic_id: {found or created epic}
     - id: {AREA}-TSK-{TYPE}-{DOMAIN}-{NNN}
     - title: {original work description}
     - origin: 'informal'
     - scope_policy: 'soft' (default for informal)
     - status: 'todo'

  4. Return task_id for use in begin-work

Output:
  task_id: {created task ID}
  epic_id: {parent epic ID}
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
Enforcement: ENF-L1 Sentinel
Agent: cf-planner (full creation), Main Agent (ongoing epics only)

Procedure:
  1. Generate epic ID: {AREA}-EPC-{TYPE}-{DOMAIN}-{NNN}

  2. Validate required fields:
     - area_type: FRT | BKD | INF | SHR | DOC | XCUT
     - work_type: FEAT | FIX | RFCT | DOCS | TEST | HTFX | CHOR | CICD | SPKE
     - domain: configured domain or GENL
     - title: descriptive title

  3. Insert into database via cf-db-operations:epic-create

  4. Create markdown file: epics/{id}/{id}.md

  5. Return epic_id

Output:
  epic_id: {generated ID}
  file_path: epics/{id}/{id}.md

📚 Resource: [id-convention.md](resources/id-convention.md)
   Load when: Generating epic IDs or understanding ID structure
```

### 🔧 update-epic

```text
When: Epic status changes or content updates
Purpose: Modify epic status, priority, or metadata
Enforcement: ENF-L1 Sentinel
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
Enforcement: ENF-L1 Sentinel
Agent: cf-planner (full), Main Agent (informal tasks)

Required Fields:
  - epic_id: parent epic
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
  1. Validate epic_id exists
  2. Generate task ID: {AREA}-TSK-{TYPE}-{DOMAIN}-{NNN}
  3. Insert into database via cf-db-operations:task-create
  4. Create markdown file: epics/{epic-id}/tasks/{task-id}.md
  5. Return task_id

Output:
  task_id: {generated ID}
  epic_id: {parent epic}
  file_path: epics/{epic-id}/tasks/{task-id}.md

Query: .state/db/queries/task-queries.sql#create

📚 Resources:
   [id-convention.md](resources/id-convention.md) - Load when: Generating task IDs
   [task-lifecycle.md](resources/task-lifecycle.md) - Load when: Setting initial task status
```

### 🔧 update-task

```text
When: Task status changes, assignment changes, or progress updates
Purpose: Modify task status, assignee, or content
Enforcement: ENF-L1 Sentinel
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
  tasks: [{id, title, status, epic_id, assignee, priority, autorun_eligible}...]
  count: {total matching}

Cross-skill: cf-db-operations (for query execution)
Query: .state/db/queries/task-queries.sql#query
Query (autorun): .state/db/queries/task-queries.sql#ready-for-autorun

📚 Resources:
   [classification-guide.md](resources/classification-guide.md) - Load when: Building query filters
   [task-lifecycle.md](resources/task-lifecycle.md) - Load when: Filtering by status
```

## ID Convention

```text
{AREA}-{ENTITY}-{TYPE}-{DOMAIN}-{NUMBER}

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
