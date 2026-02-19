# Work Classification Guide

## Area Type Classification

Determine the primary area affected by the work.

| Code | Area | Keywords | Examples |
|------|------|----------|----------|
| FRT | Frontend | frontend, UI, component, view, page, style, CSS | "Fix login button", "Add dark mode toggle" |
| BKD | Backend | backend, API, endpoint, server, database, query | "Add user endpoint", "Fix database connection" |
| INF | Infrastructure | infra, deploy, CI, CD, docker, kubernetes, terraform | "Update CI pipeline", "Add staging environment" |
| SHR | Shared | shared, common, util, lib, helper | "Add date utility", "Create validation helpers" |
| DOC | Documentation | doc, readme, guide, tutorial, ADR | "Update README", "Add API documentation" |
| PLN | Planning | plan, planning, epic, roadmap, ADR | "Plan the next phase", "Create epic for feature X" |

## Work Type Classification

Determine the nature of the work being done.

| Code | Type | Keywords | Examples |
|------|------|----------|----------|
| FEAT | Feature | add, implement, create, new, introduce | "Add dark mode", "Implement user profiles" |
| FIX | Bug Fix | fix, bug, broken, issue, problem, error | "Fix crash on login", "Resolve memory leak" |
| RFCT | Refactor | refactor, improve, clean, restructure, optimize | "Refactor auth module", "Clean up utils" |
| DOCS | Documentation | document, explain, describe, guide | "Document API endpoints", "Add usage guide" |
| TEST | Testing | test, coverage, spec, unit, integration | "Add unit tests", "Improve test coverage" |
| HTFX | Hotfix | hotfix, urgent, critical, emergency | "Urgent: fix payment processing" |
| CHOR | Chore | chore, maintenance, update deps, bump | "Update dependencies", "Bump version" |
| CICD | CI/CD | pipeline, workflow, action, deploy script | "Add staging deploy", "Fix CI workflow" |
| SPKE | Spike | spike, research, investigate, explore, POC | "Investigate caching options", "POC for new DB" |

## Classification Decision Flow

```text
1. Read the request carefully
2. Identify primary keywords
3. Match to Area Type first
4. Match to Work Type second
5. If ambiguous, ask for clarification
```

## Domain Assignment

Domains are project-specific. Common patterns:

| Domain | Typical Scope |
|--------|---------------|
| AUTH | Authentication, authorization, identity |
| API | REST/GraphQL endpoints, API contracts |
| UI | User interface components |
| DATA | Data models, persistence, queries |
| GENL | General/unspecified (fallback) |

## Area-to-Folder Mapping

| Area Code | Folder Name |
|-----------|-------------|
| FRT | FRT/ |
| BKD | BKD/ |
| INF | INF/ |
| SHR | SHR/ |
| DOC | DOC/ |
| PLN | PLN/ |

## Example Classifications

| Request | Area | Type | Domain |
|---------|------|------|--------|
| "Fix the login button styling" | FRT | FIX | AUTH |
| "Add a new user REST endpoint" | BKD | FEAT | API |
| "Update the deployment scripts" | INF | CHOR | GENL |
| "Refactor authentication across the app" | INF | RFCT | AUTH |
| "Research caching solutions" | BKD | SPKE | DATA |
