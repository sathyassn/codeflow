---
description: "Deploy to target environment"
argument-hint: "\"<environment>\" [--dry-run]"
---

# /cf-deploy Command

## Working Protocol

**Skill:** `.claude/skills/cf-working-protocol/SKILL.md`

Apply cognitive operations throughout execution:

- 🔧 meta-awareness: Continuous state and context awareness
- 🔧 think-and-act: Before tool calls and teammate dispatch (PAC-5 for deployment safety)
- 🔧 decide: At decision points (environment validation, production safeguards, rollback)
- 🔧 respond-organized: When presenting deployment results and health status

**Note:** cf-working-protocol loaded at SessionStart, applies to all execution.

---

## 1. Purpose & Usage

**Purpose:** Deploy the current work to a target environment by validating prerequisites, executing the deployment procedure, and verifying post-deploy health through cf-git-operations.

**Usage:**

```text
/cf-deploy "<environment>" [--dry-run]
```

**Use When:**

- PR has been merged to main and code is ready for deployment
- Deploying to staging for pre-production validation
- Deploying to production after staging verification
- Deploying to a preview environment for stakeholder review

**Do Not Use When:**

- Code is not yet merged (use `/cf-ship` first)
- Setting up CI/CD pipelines (use `/cf-develop` with CICD classification)
- Running tests or QA verification (use `/cf-test`)
- Reviewing code (use `/cf-review`)

### Pipeline Position

```text
Phase: Post-PF7 (optional)
Pipeline: /cf-ship --> /cf-deploy
Previous: /cf-ship (PR merged to main)
Next: Monitor deployment. Start new session for new work.
```

---

## 2. Arguments & Flags

**Arguments:**

| Argument | Required | Description |
|----------|----------|-------------|
| `environment` | Yes | Target environment: `staging`, `production`, `preview` |

**Flags:**

| Flag | Short | Description | Default |
|------|-------|-------------|---------|
| `--dry-run` | `-d` | Validate deployment prerequisites without executing | false |

**Environment Details:**

| Environment | Purpose | Safeguards |
|-------------|---------|------------|
| `staging` | Pre-production validation | Standard validation |
| `preview` | Stakeholder review | Standard validation |
| `production` | Live environment | Requires explicit confirmation, staging-first enforcement |

**Examples:**

```bash
# Deploy to staging
/cf-deploy "staging"

# Dry run for production (validate without executing)
/cf-deploy "production" --dry-run

# Deploy to preview environment
/cf-deploy "preview"
```

---

## 3. Prerequisites

**Required State:**

- [ ] PathFlow session at PF6-COMPLETE or later (work merged to main)
- [ ] Deployment configuration exists (`.github/workflows/deploy-*.yml` or equivalent)
- [ ] cf-git-operations teammate available
- [ ] cf-knowledge-layer teammate available
- [ ] For production: successful staging deployment since last merge

**Required Infrastructure:**

| Component | Purpose |
|-----------|---------|
| cf-git-operations | Deployment execution, health checks, artifact validation |
| cf-knowledge-layer | Deployment record, query previous deployments |

### 3.5 Pre-Deployment Validation

**Before dispatching deployment to cf-git-operations:**

The team lead validates deployment readiness:

1. Verify target environment is a valid value
2. Check that deployment configuration exists in the repository
3. For production: verify a successful staging deployment exists since the last merge

```text
Pre-Deployment Validation:
    |
    v
Valid environment? ------> NO --------> ERROR: "Unknown environment"
    |                                   "Valid: staging, production, preview"
    YES
    |
    v
Deploy config exists? ---> NO --------> ERROR: "No deployment config"
    |                                   "Create deploy workflow first"
    YES
    |
    v
Is production? ----------> YES -------> Staging deployed since last merge?
    |                                       |
    NO                                      v
    |                               YES --> Confirm with user
    |                               NO ---> ERROR: "Deploy to staging first"
    |                                       |
    +-------+-------------------------------+
            |
            v
        PROCEED
```

---

## 4. Workflow Definition

### 4.1 Workflow Diagram

```text
Phase: Post-PF7 (optional) | Teammate: cf-git-operations

/cf-deploy invoked
    |
    v
Parse arguments (environment, --dry-run)
    |
    v
Validate environment
    |
    v
Valid? ----NO----> ERROR: "Unknown environment"
    |
    YES
    |
    v
Check deployment config exists
    |
    v
Exists? ---NO----> ERROR: "No deployment config found"
    |
    YES
    |
    v
Is production? --YES--> Staging verified?            [cf-knowledge-layer]
    |                       |
    NO                      v
    |               YES --> Confirm with user
    |               NO ---> ERROR: "Deploy to staging first"
    |                       |
    +------+----------------+
           |
           v
Is --dry-run? --YES--> Report validation results, STOP
           |
           NO
           |
           v
Execute deployment                                   [cf-git-operations]
via cf-git-operations
    |
    v
Deployment succeeded? ---NO----> ERROR with failure details
    |
    YES
    |
    v
Post-deploy health check                             [cf-git-operations]
via cf-git-operations
    |
    v
Healthy? ---NO----> WARN: "Health check failed"
    |                "Consider rollback"
    YES
    |
    v
Record deployment                                    [cf-knowledge-layer]
(cf-knowledge-layer)
    |
    v
Present results
    |
    v
Next: Monitor deployment. Start new session for new work.
```

### 4.2 Execution Steps

**Step 1: Parse and Validate**

- Parse environment argument (required)
- Parse `--dry-run` flag
- Validate environment is one of: `staging`, `production`, `preview`
- If invalid: error with list of valid environments

**Step 2: Check Deployment Configuration**

- Search for deployment config in the repository:
  - `.github/workflows/deploy-{environment}.yml`
  - `.codeflow/config/deploy/{environment}.json`
  - `Makefile` with `deploy-{environment}` target
- If no config found: error with guidance to create deployment config via `/cf-develop` with CICD type

**Step 3: Production Safeguards**

- If environment is `production`:
  - Query cf-knowledge-layer for recent staging deployment:
    - `"LEAD: query-deployment -- environment=staging, status=success, since=last-merge"`
  - If no successful staging deployment since last merge: block with `"Deploy to staging first"`
  - If staging verified: require explicit user confirmation before proceeding
- For staging and preview: proceed without additional confirmation

**Step 4: Dry Run (if --dry-run)**

- Report all validation results without executing:
  - Environment: valid/invalid
  - Config: found/not found
  - Staging status (if production): verified/not found
  - Commit to deploy
- Stop execution after report

**Step 5: Execute Deployment**

- Send to cf-git-operations:
  - `"Execute deployment to {environment} using {config-path}"`
- cf-git-operations triggers deployment via appropriate mechanism:
  - GitHub Actions: `gh workflow run deploy-{environment}.yml`
  - Or equivalent deployment command from config
- Monitor deployment status via `gh run view`

**Step 6: Post-Deploy Health Check**

- Send to cf-git-operations:
  - `"Verify deployment health for {environment}"`
- Health checks:
  - Workflow run status: `gh run view --json status,conclusion`
  - Application health endpoint (if configured in deploy config)
- If health check fails: warn with rollback suggestions

**Step 7: Record Deployment**

- Send to cf-knowledge-layer:
  - `"LEAD: record-deployment -- environment={env}, status={success|failed}, commit={hash}, run_id={id}"`
- Logs event: type='deployment' with environment, status, timestamp, commit hash

**Step 8: Present Results**

- Show deployment summary:
  - Environment deployed to
  - Deployment status (success/failed)
  - Commit or version deployed
  - Health check results
  - Workflow run URL (if applicable)
- Show next steps based on environment:
  - Staging: "Verify in staging, then `/cf-deploy \"production\"`"
  - Production: "Deployment complete. Monitor for issues."
  - Preview: "Share preview URL with stakeholders."

---

## 5. Skills Integration

| Teammate/Skill | Operation | Purpose |
|----------------|-----------|---------|
| cf-working-protocol | think-and-act, decide | Cognitive procedures throughout |
| cf-git-operations | sync-remote | Ensure latest code is on remote |
| cf-git-operations | check-branch-status | Verify main branch state |
| cf-git-operations | review-changes | Validate deployment artifacts exist |
| cf-knowledge-layer | query-deployment | Check previous deployment records for staging-first |
| cf-knowledge-layer | record-deployment | Log deployment event to WorkGraph |

---

## 6. Hooks Integration

| Hook | When | Purpose |
|------|------|---------|
| SessionStart | Session start | Load cf-working-protocol |
| PreToolUse:pathflow-gate | Before tool calls | Verify PathFlow phase consistency |
| PostToolUse:logging | After tool calls | Log deployment operations |
| Stop:pathflow-gate | Session stop | Verify work state consistency |

**Note:** `/cf-deploy` operates at PF6-COMPLETE. The command does not modify source files, so Edit/Write hooks (edit-write, protected-resource) are not triggered. Deployment is executed through `gh` CLI commands via cf-git-operations.

---

## 7. Memory Integration

### 7.1 Deployment Recording

**On Deploy (Step 7):**

- Invoke cf-knowledge-layer:record-deployment:
  - environment: target environment
  - status: success or failed
  - commit: deployed commit hash
  - workflow_run: GitHub Actions run ID (if applicable)
  - timestamp: deployment time
- Logs event: type='deployment'

### 7.2 Staging-First Enforcement

- Production deployment queries cf-knowledge-layer for recent staging success
- Validates a staging deployment exists after the last merge to main
- This ensures staging-first workflow without manual tracking
- Staging records are queryable by environment and timestamp

### 7.3 Three-Tier Data Model

| Tier | Location | Purpose |
|------|----------|---------|
| 0 | `.state/ledger/` | Event log (deployment events, immutable) |
| 1 | `.state/db/codeflow.db` | Deployment records, status tracking |
| 2 | `project-management/` | Derived deployment history (if applicable) |

---

## 8. Error Handling

| Error | Cause | Recovery |
|-------|-------|----------|
| Unknown environment | Invalid environment name | Show valid environments: staging, production, preview |
| No deployment config | Missing workflow or config files | Create deployment config with `/cf-develop` (CICD type) |
| Staging not verified | Production deploy without staging | Deploy to staging first: `/cf-deploy "staging"` |
| Deployment failed | Workflow failure or permissions issue | Review logs via cf-git-operations, fix and retry |
| Health check failed | Application not healthy post-deploy | Investigate logs, consider rollback |
| Network error | Sandbox or connectivity issue | Consult cf-security for sandbox configuration |
| User declined confirmation | Production confirmation rejected | Deploy cancelled, no action taken |

**Recovery Procedures:**

```text
ON "Staging not verified" error:
  1. Deploy to staging first: /cf-deploy "staging"
  2. Verify staging health passes
  3. Retry: /cf-deploy "production"

ON "Deployment failed" error:
  1. Review deployment logs via cf-git-operations:
     "Get workflow run logs for run #{run_id}"
  2. Identify failure cause (permissions, config, runtime error)
  3. Fix issue and retry deployment
  4. If persistent: escalate to team lead or user

ON "Health check failed" warning:
  1. Review health check details
  2. Check application logs in the target environment
  3. If critical: deploy previous known-good version
  4. If transient: wait and re-check health
```

---

## 9. Examples

**Example 1: Deploy to Staging**

```bash
/cf-deploy "staging"
```

Output:

```text
Deploying to: staging
Commit: abc1234 (main)

Pre-deploy checks:
  [OK] Environment: staging (valid)
  [OK] Config: .github/workflows/deploy-staging.yml
  [OK] Artifacts available

Deploying...
  Workflow: deploy-staging (run #456)
  Status: success

Post-deploy health:
  [OK] Application healthy

Deployment recorded in WorkGraph.
Next: Verify in staging, then /cf-deploy "production"
```

**Example 2: Production Deploy with Confirmation**

```bash
/cf-deploy "production"
```

Output:

```text
Deploying to: production
Commit: abc1234 (main)

Pre-deploy checks:
  [OK] Environment: production (valid)
  [OK] Config: .github/workflows/deploy-production.yml
  [OK] Staging deployment verified (deployed 2h ago, healthy)

PRODUCTION DEPLOYMENT: This will deploy to the live environment.
Confirm? [y/N]: y

Deploying...
  Workflow: deploy-production (run #457)
  Status: success

Post-deploy health:
  [OK] Application healthy

Deployment recorded in WorkGraph.
Next: Deployment complete. Monitor for issues.
```

**Example 3: Dry Run for Production**

```bash
/cf-deploy "production" --dry-run
```

Output:

```text
DRY RUN: Validating deployment to production

  [OK] Environment: production (valid)
  [OK] Config: .github/workflows/deploy-production.yml
  [OK] Staging deployment verified (deployed 2h ago)
  [OK] Commit: abc1234 on main

All checks passed. Ready to deploy.
Run without --dry-run to proceed.
```

**Example 4: Production Blocked by Missing Staging**

```bash
/cf-deploy "production"
```

Output:

```text
Deploying to: production

Pre-deploy checks:
  [OK] Environment: production (valid)
  [OK] Config: .github/workflows/deploy-production.yml
  [FAIL] No staging deployment found since last merge to main

Cannot deploy to production: staging deployment required first.
Deploy to staging: /cf-deploy "staging"
```

---

## 10. References

- [cf-git-operations agent](../agents/cf-git-operations.md)
- [cf-knowledge-layer agent](../agents/cf-knowledge-layer.md)
- [cf-working-protocol skill](../skills/cf-working-protocol/SKILL.md)
- [PathFlow configuration](../../.codeflow/config/pathflow/pathflow-config.json)
- [cf-ship command](cf-ship.md)
- [cf-develop command](cf-develop.md)
- [cf-test command](cf-test.md)
