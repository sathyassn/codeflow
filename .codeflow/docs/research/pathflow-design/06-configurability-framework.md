# PathFlow Configurability Framework

> Design specification for how PathFlow's enforcement, node behavior, teammate composition, and session profiles are configured, inherited, and overridden at runtime.
> Designer: teammate-designer | Date: 2026-02-06
> Depends on: 04-outer-shell-pathflow.md (nodes, session profiles), 05-skill-to-teammate-model.md (persistence, blueprints)

---

## 1. Overview

### The Problem

PathFlow has many configurable dimensions:
- 9 outer shell nodes, each with enforcement level, skippability, and behavior variants
- 4 session profiles (interactive, autorun, structured, unstructured), each altering node behavior
- Teammate composition (which roles to spawn, persistence modes, context thresholds)
- Sentinel enforcement (blocking vs advisory vs disabled per node)
- Inner pathway selection and checkpoint behavior
- Runtime modifications (dynamic node insertion, checkpoint re-opening, enforcement relaxation)

Without a unified configurability framework, these dimensions become a tangled mess of ad-hoc settings scattered across JSON files, hooks, and spawn prompts.

### Design Goals

1. **Single source of truth**: One configuration schema that drives all PathFlow behavior
2. **Layered inheritance**: Global defaults -> session profile -> per-node overrides -> runtime modifications
3. **Progressive complexity**: Simple sessions need zero configuration; complex sessions can tune every knob
4. **Auditability**: Every configuration choice and runtime override is logged
5. **Safety**: Enforcement can be relaxed but not silently disabled; all relaxations leave audit trails

### Non-Goals

- This framework does NOT replace `.claude/settings.json` for Claude Code's native configuration (permissions, hooks, etc.)
- This framework does NOT configure individual teammate behavior (that is the blueprint's job -- see doc 05)
- This framework does NOT define inner pathway templates (those are separate JSON files in `.codeflow/config/pathways/`)

---

## 2. Configuration Architecture

### 2.1 Configuration Layers

PathFlow configuration follows a four-layer inheritance model. Each layer can override values from the layer above it. Lower layers take precedence.

```
Layer 0: HARDCODED DEFAULTS (in PathFlow engine code)
    |
    v
Layer 1: PATHWAY TEMPLATE DEFAULTS (in outer-shell.json + inner pathway JSON files)
    |
    v
Layer 2: SESSION PROFILE (selected at OS-2, from pathflow-config.json)
    |
    v
Layer 3: RUNTIME OVERRIDES (applied by team lead during session)
```

**Resolution rule**: For any configuration key, the lowest layer that defines it wins. If Layer 3 sets `verification_tier: "force-tier-1"`, it overrides Layer 2's `verification_tier: "auto-detect"`, which overrides Layer 1's default.

### 2.2 Configuration File Location

```
.codeflow/config/
  pathflow-config.json          # Master PathFlow configuration (Layers 1-2)
  pathways/
    outer-shell.json            # Outer shell pathway template (Layer 1 node defaults)
    development.json            # Inner pathway template
    bugfix.json                 # Inner pathway template
    review.json                 # Inner pathway template
    planning.json               # Inner pathway template
    ...
```

The master configuration file is `.codeflow/config/pathflow-config.json`. It contains:
- Global defaults (Layer 1 overrides of hardcoded defaults)
- Session profile definitions (Layer 2)
- Teammate composition templates
- Enforcement policy

Pathway template files (`outer-shell.json`, `development.json`, etc.) contain node-level defaults that can be overridden by the master config.

### 2.3 Runtime State Location

```
/tmp/claude/managed/pathflow/
  active-session.json           # Current session's resolved configuration
  overrides.json                # Runtime overrides applied during this session
  audit.jsonl                   # Configuration change audit trail
```

Runtime state is ephemeral (in `/tmp/claude/`). It is created at session start and cleaned up at session end. The audit trail is appended to the persistent JSONL ledger before cleanup.

---

## 3. Master Configuration Schema

### 3.1 Full Schema: `pathflow-config.json`

```json
{
  "$schema": "pathflow-config-v1",
  "version": "1.0.0",

  "global_defaults": {
    "enforcement_policy": "standard",
    "sentinel_ttl_session": 14400,
    "sentinel_ttl_short": 600,
    "checkpoint_max_reopens": 3,
    "checkpoint_force_allow": true,
    "dynamic_insertion_allowed": true,
    "dynamic_insertion_max": 3,
    "audit_all_overrides": true
  },

  "enforcement_policies": {
    "strict": {
      "description": "All enforcement active, no skipping, no relaxation",
      "allow_node_skip": false,
      "allow_enforcement_relaxation": false,
      "allow_checkpoint_force_allow": false,
      "min_verification_tier": 2,
      "require_pr": true,
      "require_team": true
    },
    "standard": {
      "description": "Default enforcement with configurable relaxation",
      "allow_node_skip": true,
      "allow_enforcement_relaxation": true,
      "allow_checkpoint_force_allow": true,
      "min_verification_tier": 1,
      "require_pr": false,
      "require_team": false
    },
    "permissive": {
      "description": "Minimal enforcement, all advisory, maximum flexibility",
      "allow_node_skip": true,
      "allow_enforcement_relaxation": true,
      "allow_checkpoint_force_allow": true,
      "min_verification_tier": 1,
      "require_pr": false,
      "require_team": false
    }
  },

  "session_profiles": {
    "interactive": {
      "description": "Manual interactive session with user present",
      "enforcement_policy": "standard",
      "team": {
        "strategy": "auto-detect",
        "max_teammates": 3,
        "persistent_roles": ["cf-developer"],
        "on_demand_roles": ["cf-planner", "cf-reviewer", "cf-qa", "cf-ops", "cf-documenter"],
        "sub_agent_roles": ["cf-support"]
      },
      "nodes": {
        "OS-2": { "resume_behavior": "prompt-always", "meta_awareness": "full" },
        "OS-3": { "team_strategy": "auto-detect" },
        "OS-4": { "classification_mode": "auto-detect", "confirm_with_user": true },
        "OS-5": { "checkpoint_interaction": "interactive", "max_duration": null },
        "OS-6": { "auto_push": "prompt", "auto_pr": "prompt" },
        "OS-7": { "verification_tier": "auto-detect", "display_output": true }
      },
      "context": {
        "warning_threshold": 0.70,
        "recycle_threshold": 0.85,
        "enable_heartbeat": true,
        "heartbeat_interval_ops": 10,
        "heartbeat_interval_seconds": 300
      }
    },

    "autorun": {
      "description": "Automated batch execution with acceptance criteria",
      "enforcement_policy": "standard",
      "team": {
        "strategy": "batch-defined",
        "max_teammates": 3,
        "persistent_roles": ["cf-developer"],
        "on_demand_roles": ["cf-qa", "cf-ops"],
        "sub_agent_roles": ["cf-support"]
      },
      "nodes": {
        "OS-2": { "resume_behavior": "auto-resume-matching", "meta_awareness": "silent" },
        "OS-3": { "team_strategy": "batch-defined" },
        "OS-4": { "classification_mode": "batch-defined", "confirm_with_user": false },
        "OS-5": { "checkpoint_interaction": "auto-evaluate", "max_duration": 1800 },
        "OS-6": { "auto_push": "always", "auto_pr": "always" },
        "OS-7": { "verification_tier": "auto-detect", "display_output": false }
      },
      "context": {
        "warning_threshold": 0.70,
        "recycle_threshold": 0.85,
        "enable_heartbeat": true,
        "heartbeat_interval_ops": 10,
        "heartbeat_interval_seconds": 300
      }
    },

    "structured": {
      "description": "Epic-driven, task-tracked development",
      "enforcement_policy": "standard",
      "team": {
        "strategy": "standard",
        "max_teammates": 5,
        "persistent_roles": ["cf-developer"],
        "on_demand_roles": ["cf-planner", "cf-reviewer", "cf-qa", "cf-ops", "cf-documenter"],
        "sub_agent_roles": ["cf-support"]
      },
      "nodes": {
        "OS-2": { "resume_behavior": "auto-resume-matching", "meta_awareness": "minimal" },
        "OS-3": { "team_strategy": "standard" },
        "OS-4": { "classification_mode": "auto-detect", "confirm_with_user": false },
        "OS-5": { "checkpoint_interaction": "interactive", "max_duration": null },
        "OS-6": { "auto_push": "always", "auto_pr": "always" },
        "OS-7": { "verification_tier": "force-tier-2", "display_output": true }
      },
      "context": {
        "warning_threshold": 0.70,
        "recycle_threshold": 0.85,
        "enable_heartbeat": true,
        "heartbeat_interval_ops": 10,
        "heartbeat_interval_seconds": 300
      }
    },

    "unstructured": {
      "description": "Ad-hoc exploration, quick fixes, research",
      "enforcement_policy": "permissive",
      "team": {
        "strategy": "solo",
        "max_teammates": 0,
        "persistent_roles": [],
        "on_demand_roles": [],
        "sub_agent_roles": ["cf-support"]
      },
      "nodes": {
        "OS-2": { "resume_behavior": "never-resume", "meta_awareness": "minimal" },
        "OS-3": { "team_strategy": "solo" },
        "OS-4": { "classification_mode": "auto-detect", "confirm_with_user": false },
        "OS-5": { "checkpoint_interaction": "auto-evaluate", "max_duration": null },
        "OS-6": { "auto_push": "prompt", "auto_pr": "never" },
        "OS-7": { "verification_tier": "force-tier-1", "display_output": true }
      },
      "context": {
        "warning_threshold": 0.80,
        "recycle_threshold": 0.90,
        "enable_heartbeat": false,
        "heartbeat_interval_ops": null,
        "heartbeat_interval_seconds": null
      }
    }
  },

  "node_defaults": {
    "OS-1": {
      "enforcement": "system",
      "skippable": false,
      "cleanup_age_seconds": 86400
    },
    "OS-2": {
      "enforcement": "ENF-L3",
      "skippable": false,
      "resume_behavior": "prompt-always",
      "meta_awareness": "full",
      "active_work_detection": true
    },
    "OS-3": {
      "enforcement": "ENF-L1",
      "skippable": true,
      "skip_condition": "team_strategy == 'solo' OR agent_teams_disabled",
      "team_strategy": "auto-detect",
      "max_teammates": 3,
      "spawn_retry_max": 2,
      "spawn_fallback": "solo"
    },
    "OS-4": {
      "enforcement": "ENF-L1",
      "skippable": false,
      "classification_mode": "auto-detect",
      "confirm_with_user": true,
      "ongoing_epic_creation": true,
      "checkpoint_config": {
        "reopenable": true,
        "max_reopens": 3,
        "on_reopen": "delete_sentinel_and_reblock"
      }
    },
    "OS-5": {
      "enforcement": "delegated",
      "skippable": false,
      "checkpoint_interaction": "interactive",
      "max_duration": null,
      "inner_pathway_dir": ".codeflow/config/pathways/"
    },
    "OS-6": {
      "enforcement": "ENF-L1",
      "skippable": false,
      "auto_push": "prompt",
      "auto_pr": "prompt",
      "teammate_shutdown_timing": "immediate"
    },
    "OS-7": {
      "enforcement": "ENF-L2",
      "skippable": false,
      "verification_tier": "auto-detect",
      "display_output": true,
      "checkpoint_config": {
        "reopenable": true,
        "max_reopens": 3,
        "on_reopen": "re-enter-finalize-or-inner",
        "force_allow_after_max": true
      }
    },
    "OS-8": {
      "enforcement": "system",
      "skippable": false,
      "teammate_shutdown_timeout": 30,
      "cleanup_mode": "full"
    },
    "OS-9": {
      "enforcement": "system",
      "skippable": false
    }
  }
}
```

### 3.2 Schema Key Reference

#### Global Defaults

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `enforcement_policy` | string | `"standard"` | Active enforcement policy name |
| `sentinel_ttl_session` | number | `14400` | TTL in seconds for session-lifetime sentinels |
| `sentinel_ttl_short` | number | `600` | TTL in seconds for short-lived sentinels |
| `checkpoint_max_reopens` | number | `3` | Default max re-opens for all checkpoints |
| `checkpoint_force_allow` | boolean | `true` | Allow force-allowing after max re-opens |
| `dynamic_insertion_allowed` | boolean | `true` | Allow runtime node insertion |
| `dynamic_insertion_max` | number | `3` | Max nodes that can be inserted at runtime |
| `audit_all_overrides` | boolean | `true` | Log all runtime configuration changes |

#### Enforcement Policies

| Key | Type | Description |
|-----|------|-------------|
| `allow_node_skip` | boolean | Whether skippable nodes can be skipped |
| `allow_enforcement_relaxation` | boolean | Whether enforcement can be lowered at runtime |
| `allow_checkpoint_force_allow` | boolean | Whether checkpoints can be force-allowed |
| `min_verification_tier` | number | Minimum PCV tier (1, 2, or 3) |
| `require_pr` | boolean | Whether PR creation is mandatory |
| `require_team` | boolean | Whether team mode is mandatory |

#### Per-Node Configuration

| Key | Applies To | Type | Description |
|-----|-----------|------|-------------|
| `enforcement` | All nodes | string | `"system"`, `"ENF-L1"`, `"ENF-L2"`, `"ENF-L3"`, `"delegated"` |
| `skippable` | All nodes | boolean | Whether this node can be skipped |
| `skip_condition` | Skippable nodes | string | Condition expression for auto-skip |
| `resume_behavior` | OS-2 | string | `"prompt-always"`, `"auto-resume-single"`, `"auto-resume-matching"`, `"never-resume"` |
| `meta_awareness` | OS-2 | string | `"full"`, `"minimal"`, `"silent"` |
| `team_strategy` | OS-3 | string | `"auto-detect"`, `"solo"`, `"minimal"`, `"standard"`, `"full"`, `"batch-defined"` |
| `classification_mode` | OS-4 | string | `"auto-detect"`, `"user-specified"`, `"batch-defined"` |
| `checkpoint_interaction` | OS-5 | string | `"interactive"`, `"auto-evaluate"` |
| `max_duration` | OS-5 | number/null | Max inner pathway duration in seconds |
| `auto_push` | OS-6 | string | `"always"`, `"never"`, `"prompt"` |
| `auto_pr` | OS-6 | string | `"always"`, `"never"`, `"prompt"`, `"if-remote-branch"` |
| `verification_tier` | OS-7 | string | `"auto-detect"`, `"force-tier-1"`, `"force-tier-2"`, `"force-tier-3"` |

---

## 4. Configuration Inheritance Model

### 4.1 Resolution Algorithm

When the PathFlow engine needs a configuration value for a specific node, it resolves it through the layer stack:

```
function resolve(node_id, key):
    # Layer 3: Runtime overrides (highest priority)
    if runtime_overrides[node_id][key] exists:
        return runtime_overrides[node_id][key]

    # Layer 2: Session profile
    active_profile = get_active_session_profile()
    if active_profile.nodes[node_id][key] exists:
        return active_profile.nodes[node_id][key]

    # Layer 1: Pathway template defaults
    if pathflow_config.node_defaults[node_id][key] exists:
        return pathflow_config.node_defaults[node_id][key]

    # Layer 0: Hardcoded defaults
    return HARDCODED_DEFAULTS[key]
```

### 4.2 Inheritance Example

Consider resolving `verification_tier` for node OS-7:

```
Layer 0 (hardcoded):   verification_tier = "auto-detect"
Layer 1 (node_defaults): verification_tier = "auto-detect"    (same as hardcoded)
Layer 2 (structured profile): verification_tier = "force-tier-2"  (overrides!)
Layer 3 (runtime): not set

Result: "force-tier-2"
```

Now if the team lead applies a runtime override:

```
Layer 3 (runtime): verification_tier = "force-tier-3"  (overrides everything)

Result: "force-tier-3"
```

### 4.3 Enforcement Policy Inheritance

Enforcement policies add a constraint layer. Even if a session profile allows something, the enforcement policy can restrict it:

```
function resolve_with_policy(node_id, key, value):
    resolved_value = resolve(node_id, key)
    policy = get_active_enforcement_policy()

    # Policy constraints override resolved values
    if key == "verification_tier" and tier_number(resolved_value) < policy.min_verification_tier:
        resolved_value = "force-tier-" + policy.min_verification_tier

    if key == "skippable" and resolved_value == true and not policy.allow_node_skip:
        resolved_value = false

    if key == "auto_pr" and resolved_value == "never" and policy.require_pr:
        resolved_value = "always"

    return resolved_value
```

This ensures that a `strict` enforcement policy cannot be circumvented by a permissive session profile.

### 4.4 Policy Precedence

```
enforcement_policy constraints > runtime overrides > session profile > node defaults > hardcoded

Exception: The "system" enforcement level on OS-1, OS-8, OS-9 cannot be overridden by any layer.
```

---

## 5. Per-Node Enforcement Toggles

### 5.1 Enforcement Levels

Each node has an enforcement level that determines how it blocks session progression:

| Level | Behavior | Can Be Relaxed? | Can Be Strengthened? |
|-------|----------|:---------------:|:--------------------:|
| `system` | Automatic, immutable, always fires | No | No |
| `ENF-L1` | Sentinel-based blocking; must satisfy to proceed | To ENF-L3 (if policy allows) | No (already maximum non-system) |
| `ENF-L2` | Stop-hook blocking; must satisfy to end session | To ENF-L3 (if policy allows) | To ENF-L1 |
| `ENF-L3` | Advisory; logged warning if skipped | To disabled | To ENF-L1 or ENF-L2 |
| `delegated` | Inner pathway controls enforcement | No (delegated to inner) | No |
| `disabled` | No enforcement, node passes through | N/A | To any level |

### 5.2 Relaxation Rules

Enforcement can be relaxed (lowered) under these conditions:

1. The active enforcement policy has `allow_enforcement_relaxation: true`
2. The node is not a `system` enforcement node (OS-1, OS-8, OS-9 are immutable)
3. The relaxation is applied as a runtime override (Layer 3)
4. The relaxation is logged to the audit trail

**Relaxation paths:**
```
ENF-L1 -> ENF-L3    (blocking -> advisory)
ENF-L2 -> ENF-L3    (stop-blocking -> advisory)
ENF-L3 -> disabled   (advisory -> silent)
```

**Strengthening paths (always allowed, no policy check needed):**
```
disabled -> ENF-L3 -> ENF-L1
disabled -> ENF-L3 -> ENF-L2
ENF-L3 -> ENF-L1
ENF-L3 -> ENF-L2
```

### 5.3 Per-Node Enforcement Configuration

The team lead can toggle enforcement per node at runtime:

```json
{
  "runtime_overrides": {
    "OS-3": { "enforcement": "ENF-L3" },
    "OS-7": { "enforcement": "ENF-L1" }
  }
}
```

This would:
- Relax OS-3 (Team Init) from ENF-L1 to ENF-L3 (advisory -- team init is recommended but not required)
- Strengthen OS-7 (Verification) from ENF-L2 to ENF-L1 (must pass verification to proceed to next node, not just to end session)

### 5.4 Node Enforcement Matrix (All Profiles)

| Node | Default | interactive | autorun | structured | unstructured | Relaxable? |
|------|---------|:-----------:|:-------:|:----------:|:------------:|:----------:|
| OS-1 | system | system | system | system | system | No |
| OS-2 | ENF-L3 | ENF-L3 | ENF-L3 | ENF-L3 | ENF-L3 | To disabled |
| OS-3 | ENF-L1 | ENF-L1 | ENF-L1 | ENF-L1 | disabled* | To ENF-L3 |
| OS-4 | ENF-L1 | ENF-L1 | ENF-L1 | ENF-L1 | ENF-L3 | To ENF-L3 |
| OS-5 | delegated | delegated | delegated | delegated | delegated | No |
| OS-6 | ENF-L1 | ENF-L1 | ENF-L1 | ENF-L1 | ENF-L3 | To ENF-L3 |
| OS-7 | ENF-L2 | ENF-L2 | ENF-L2 | ENF-L2 | ENF-L3 | To ENF-L3 |
| OS-8 | system | system | system | system | system | No |
| OS-9 | system | system | system | system | system | No |

*Unstructured sessions disable OS-3 because `team_strategy: "solo"` triggers the skip condition.

---

## 6. Teammate Composition Configuration

### 6.1 Composition Schema

Teammate composition is configured per session profile (Layer 2) and can be overridden at runtime (Layer 3).

```json
{
  "team": {
    "strategy": "auto-detect | solo | minimal | standard | full | batch-defined",
    "max_teammates": 3,
    "persistent_roles": ["cf-developer"],
    "on_demand_roles": ["cf-planner", "cf-reviewer", "cf-qa", "cf-ops", "cf-documenter"],
    "sub_agent_roles": ["cf-support"],
    "persistence_overrides": {},
    "blueprint_dir": ".claude/agents/"
  }
}
```

### 6.2 Strategy Resolution

The `strategy` field determines the initial team composition at OS-3:

| Strategy | Teammates Spawned | When Used |
|----------|-------------------|-----------|
| `solo` | None | Quick fixes, research, unstructured work |
| `minimal` | cf-developer | Simple features, bug fixes |
| `standard` | cf-developer + on-demand as needed | Standard development |
| `full` | cf-developer + cf-planner + cf-qa | Complex features, multi-epic work |
| `auto-detect` | Determined by lead at OS-3 based on OS-2 context | Interactive sessions |
| `batch-defined` | Specified in batch config | Autorun sessions |

### 6.3 Persistence Overrides

The default persistence for each role comes from the profile's `persistent_roles` and `on_demand_roles` lists. Runtime overrides can change a role's persistence:

```json
{
  "team": {
    "persistence_overrides": {
      "cf-planner": "persistent",
      "cf-reviewer": "persistent"
    }
  }
}
```

This would promote cf-planner and cf-reviewer to persistent for the current session (useful for intensive planning or multi-PR review sessions).

**Allowed transitions:**
```
persistent -> on-demand    (demote: teammate gets shut down after current task)
on-demand -> persistent    (promote: teammate stays alive after task completion)
on-demand -> sub-agent     (demote: use Task tool instead of spawning teammate)
sub-agent -> on-demand     (promote: spawn full teammate for complex work)
```

Transitions between persistent and sub-agent are not allowed (too large a jump). Transition through on-demand first.

### 6.4 Context Thresholds

Context management thresholds are configured per profile:

```json
{
  "context": {
    "warning_threshold": 0.70,
    "recycle_threshold": 0.85,
    "enable_heartbeat": true,
    "heartbeat_interval_ops": 10,
    "heartbeat_interval_seconds": 300
  }
}
```

These can be overridden at runtime:

```json
{
  "runtime_overrides": {
    "context": {
      "warning_threshold": 0.80,
      "recycle_threshold": 0.95
    }
  }
}
```

This would delay recycling warnings, giving teammates more context runway (at the cost of potential quality degradation near the end).

---

## 7. Runtime Override Mechanism

### 7.1 How Overrides Are Applied

Runtime overrides are applied by the team lead during the session. They are stored in `/tmp/claude/managed/pathflow/overrides.json` and take effect immediately.

**Mechanisms for applying overrides:**

1. **Team lead internal decision**: The lead evaluates the session state and decides to adjust configuration. This happens naturally as part of the lead's orchestration logic at each node.

2. **User request**: The user asks the lead to adjust behavior (e.g., "skip the PR for this one" -> lead sets `OS-6.auto_pr: "never"`).

3. **Automatic escalation**: A PostToolUse hook detects a condition that triggers an automatic override (e.g., session running long -> escalate verification tier).

### 7.2 Override Schema

```json
{
  "session_id": "session-01JKABCDEF1234",
  "applied_at": "2026-02-06T19:30:00Z",
  "overrides": {
    "nodes": {
      "OS-6": { "auto_pr": "never" },
      "OS-7": { "verification_tier": "force-tier-1" }
    },
    "team": {
      "persistence_overrides": {
        "cf-planner": "persistent"
      }
    },
    "context": {
      "recycle_threshold": 0.90
    },
    "enforcement_policy": null
  },
  "audit_trail": [
    {
      "timestamp": "2026-02-06T19:30:00Z",
      "key": "nodes.OS-6.auto_pr",
      "old_value": "prompt",
      "new_value": "never",
      "reason": "User requested: skip PR for quick fix",
      "applied_by": "team-lead"
    }
  ]
}
```

### 7.3 Override Constraints

Not everything can be overridden at runtime:

| Dimension | Overridable? | Constraint |
|-----------|:------------:|------------|
| Node enforcement level | Yes | Cannot override `system` nodes; relaxation requires policy permission |
| Verification tier | Yes | Cannot go below `min_verification_tier` from enforcement policy |
| Auto-push / auto-PR | Yes | Cannot set `never` if policy has `require_pr: true` |
| Team strategy | Yes | Cannot change from `solo` to team if Agent Teams is disabled |
| Teammate persistence | Yes | Must follow allowed transitions (persistent <-> on-demand <-> sub-agent) |
| Context thresholds | Yes | No constraints (thresholds are advisory) |
| Enforcement policy | Yes | Can switch between defined policies; cannot create new policies at runtime |
| Node skippability | Partially | Can make non-skippable nodes skippable only if policy allows |
| Max teammates | Yes | No upper constraint (but practical API limits apply) |
| Checkpoint max_reopens | Yes | No constraints |
| Dynamic insertion | Yes | Can disable but not increase max beyond configured limit |

### 7.4 Override Validation

Before applying an override, the PathFlow engine validates it:

```
function validate_override(key, value):
    policy = get_active_enforcement_policy()

    # Check immutable nodes
    if key starts with "nodes.OS-1" or "nodes.OS-8" or "nodes.OS-9":
        if sub_key == "enforcement":
            return REJECT("System node enforcement is immutable")

    # Check enforcement relaxation permission
    if sub_key == "enforcement" and is_relaxation(old_value, value):
        if not policy.allow_enforcement_relaxation:
            return REJECT("Enforcement policy prohibits relaxation")

    # Check verification tier minimum
    if sub_key == "verification_tier":
        if tier_number(value) < policy.min_verification_tier:
            return REJECT("Below minimum verification tier for policy")

    # Check PR requirement
    if key == "nodes.OS-6.auto_pr" and value == "never":
        if policy.require_pr:
            return REJECT("Enforcement policy requires PR creation")

    return ACCEPT
```

### 7.5 Override Lifecycle

```
Session Start
    |
    v
Load pathflow-config.json (Layers 0-2)
    |
    v
Create empty overrides.json (Layer 3)
    |
    v
[During session: team lead applies overrides as needed]
    |-- Each override: validate -> apply -> audit log -> take effect immediately
    |
    v
Session End
    |
    v
Flush audit trail from overrides.json to persistent JSONL ledger
    |
    v
Delete /tmp/claude/managed/pathflow/ (cleanup)
```

---

## 8. Session Profile Selection

### 8.1 Selection Mechanism

The session profile is selected during OS-2 (Context Load). The selection can be:

1. **Explicit** (user-specified): User starts session with profile intent (e.g., "Let's plan the auth feature" -> `structured`)
2. **Batch-specified**: Autorun config includes `"session_profile": "autorun"`
3. **Auto-detected**: Team lead analyzes the first user prompt and context to determine profile

### 8.2 Auto-Detection Heuristic

When `auto-detect` is used, the team lead evaluates these signals:

| Signal | Weight | Profile Suggested |
|--------|:------:|-------------------|
| `/cf-plan`, `/cf-develop` command | High | `structured` |
| `/cf-review`, `/cf-test` command | High | `structured` |
| Existing epic/task referenced | High | `structured` |
| Active work detected at OS-2 | Medium | `structured` (resume) |
| Quick question or help request | High | `unstructured` |
| "Fix this bug" / "quick change" | Medium | `unstructured` |
| No commands, ad-hoc request | Medium | `interactive` (default) |
| Batch/CI environment detected | High | `autorun` |

**Default fallback**: `interactive` (the safest default with user confirmation at every checkpoint).

### 8.3 Profile Switching

The session profile can be changed mid-session as a runtime override. This is rare but valid:

- User starts with a "quick fix" (`unstructured`) but it grows complex -> lead switches to `structured`
- Structured session completes its main work, user wants to do exploratory research -> lead switches to `unstructured` for a second pass

Profile switching applies immediately. Node defaults for subsequent nodes use the new profile. Already-passed nodes are not retroactively affected.

---

## 9. Dynamic Pathway Modification

### 9.1 Node Insertion at Runtime

The team lead can insert new nodes into the outer shell during the session (as defined in the outer shell design, section 6.3).

**Configuration governing insertion:**

```json
{
  "global_defaults": {
    "dynamic_insertion_allowed": true,
    "dynamic_insertion_max": 3
  }
}
```

**Insertion request format:**

```json
{
  "action": "insert_node",
  "insert_after": "OS-5",
  "node": {
    "id": "OS-5a",
    "name": "Security Review",
    "type": "work",
    "agent_role": "cf-reviewer",
    "enforcement": "ENF-L1",
    "acceptance": "Security review passes with no critical findings"
  },
  "reason": "Protected file changes detected during inner pathway"
}
```

**Validation:**
- `dynamic_insertion_allowed` must be `true`
- Current insertion count must be below `dynamic_insertion_max`
- `insert_after` must be a valid insertion point (`after:OS-4`, `after:OS-5`, `after:OS-6`)
- The inserted node must not create cycles in the dependency graph

### 9.2 Checkpoint Re-Opening at Runtime

Checkpoints (OS-4, OS-7) can be re-opened if their acceptance criteria fail. This is configured per-checkpoint:

```json
{
  "checkpoint_config": {
    "reopenable": true,
    "max_reopens": 3,
    "on_reopen": "delete_sentinel_and_reblock",
    "force_allow_after_max": true
  }
}
```

Runtime overrides can adjust these values:

```json
{
  "runtime_overrides": {
    "nodes": {
      "OS-7": {
        "checkpoint_config": {
          "max_reopens": 5,
          "force_allow_after_max": false
        }
      }
    }
  }
}
```

This would give OS-7 more re-open attempts but never force-allow (strict mode for critical work).

### 9.3 Node Skipping at Runtime

Beyond the pre-configured skip conditions, the team lead can skip nodes at runtime:

```json
{
  "action": "skip_node",
  "node_id": "OS-6",
  "reason": "Inner pathway already committed and pushed"
}
```

**Validation:**
- Node must be marked `skippable: true` OR enforcement policy must have `allow_node_skip: true`
- System nodes (OS-1, OS-8, OS-9) can never be skipped
- Skip is logged to audit trail with reason

---

## 10. Configuration and Hooks Integration

### 10.1 How Hooks Read Configuration

PathFlow hooks need to read the resolved configuration to make enforcement decisions. The resolved configuration is written to `/tmp/claude/managed/pathflow/active-session.json` at session start and updated whenever runtime overrides are applied.

Hook access pattern:

```bash
# In a PreToolUse hook:
PATHFLOW_CONFIG="/tmp/claude/managed/pathflow/active-session.json"

if [ -f "$PATHFLOW_CONFIG" ]; then
    enforcement=$(jq -r '.resolved.nodes["OS-4"].enforcement' "$PATHFLOW_CONFIG")
    if [ "$enforcement" = "disabled" ]; then
        exit 0  # No enforcement, allow tool
    fi
    # ... check sentinel as usual
fi
```

### 10.2 New Hook: `cf-pre-tool-use-pathflow-gate.sh`

A new PreToolUse hook enforces pathway node progression:

**Purpose**: Prevent tool calls that would violate the pathway's node ordering. For example, prevent `git commit` if `pathflow:shell:work-reg` sentinel does not exist (meaning the outer shell has not reached OS-4 yet).

**Logic:**
1. Read resolved configuration from `active-session.json`
2. Determine which pathway node the current tool call relates to
3. Check if the required predecessor sentinel exists
4. If sentinel missing and enforcement is `ENF-L1`: block (exit 2)
5. If sentinel missing and enforcement is `ENF-L3`: warn (exit 0 with message)
6. If sentinel missing and enforcement is `disabled`: pass (exit 0)

**Tool-to-Node mapping:**

| Tool Pattern | Requires Pathway Sentinel | Node |
|-------------|--------------------------|------|
| `Edit`, `Write` (non-config) | `pathflow:shell:work-reg` | OS-4 must be passed |
| `Bash: git commit` | `pathflow:shell:inner-complete` | OS-5 must be passed |
| `Bash: git push` | `pathflow:shell:finalize` | OS-6 must be passed |
| `Bash: gh pr create` | `pathflow:shell:finalize` | OS-6 must be passed |
| `Teammate.spawnTeam` | `pathflow:shell:context` | OS-2 must be passed |

### 10.3 Hook Configuration Awareness

Existing hooks should be updated to check the resolved PathFlow configuration before enforcing. This allows enforcement to be relaxed per-node without modifying individual hooks:

```bash
# Generic pattern for PathFlow-aware hooks:
PATHFLOW_CONFIG="/tmp/claude/managed/pathflow/active-session.json"

# Determine current enforcement level for the relevant node
get_node_enforcement() {
    local node_id="$1"
    if [ -f "$PATHFLOW_CONFIG" ]; then
        jq -r ".resolved.nodes[\"$node_id\"].enforcement // \"ENF-L1\"" "$PATHFLOW_CONFIG"
    else
        echo "ENF-L1"  # Default to blocking if no config
    fi
}

# Use in hook:
enforcement=$(get_node_enforcement "OS-4")
case "$enforcement" in
    "disabled") exit 0 ;;
    "ENF-L3")
        # Warn but don't block
        echo '{"result":"warn","message":"Advisory: work not registered"}'
        exit 0
        ;;
    "ENF-L1"|"ENF-L2")
        # Block
        echo '{"result":"deny","message":"Work must be registered before proceeding"}'
        exit 2
        ;;
esac
```

---

## 11. Audit Trail

### 11.1 What Is Audited

Every configuration-relevant event is logged to the audit trail:

| Event | When | Logged Data |
|-------|------|-------------|
| Session profile selected | OS-2 | Profile name, selection method (auto/explicit/batch) |
| Enforcement policy activated | OS-2 | Policy name |
| Node enforcement overridden | Runtime | Node ID, old level, new level, reason |
| Node skipped | Runtime | Node ID, skip reason, sentinel metadata |
| Checkpoint re-opened | Runtime | Checkpoint ID, reopen count, failure reason |
| Node inserted | Runtime | Node spec, insertion point, reason |
| Teammate persistence changed | Runtime | Role, old persistence, new persistence, reason |
| Profile switched | Runtime | Old profile, new profile, reason |
| Force-allow triggered | Runtime | Checkpoint ID, reopen count at force |

### 11.2 Audit Log Format

```json
{
  "timestamp": "2026-02-06T19:35:00Z",
  "session_id": "session-01JKABCDEF1234",
  "event": "enforcement_override",
  "node_id": "OS-7",
  "details": {
    "old_value": "ENF-L2",
    "new_value": "ENF-L3",
    "reason": "Quick fix, user confirmed low-risk change",
    "applied_by": "team-lead",
    "policy_check": "passed (standard policy allows relaxation)"
  }
}
```

### 11.3 Audit Persistence

During the session, audit entries are appended to `/tmp/claude/managed/pathflow/audit.jsonl`. At session end (OS-8/OS-9), the audit trail is flushed to the persistent JSONL ledger at `.codeflow/audit/pathflow-audit.jsonl` (append-only, never overwritten).

---

## 12. Configuration Presets

### 12.1 Preset Concept

For common scenarios, PathFlow provides named presets that combine session profile + enforcement policy + teammate composition into a single selection:

| Preset | Profile | Policy | Team Strategy | Use Case |
|--------|---------|--------|:-------------:|----------|
| `quick-fix` | unstructured | permissive | solo | One-line fixes, typos |
| `feature` | interactive | standard | auto-detect | Standard feature work |
| `epic` | structured | standard | standard | Multi-task epic development |
| `strict-epic` | structured | strict | full | Critical features, security-sensitive work |
| `batch` | autorun | standard | batch-defined | CI/CD automated sessions |
| `research` | unstructured | permissive | solo | Exploration, no code changes |
| `review` | interactive | standard | minimal | Code review sessions |

### 12.2 Using Presets

Presets are syntactic sugar. The team lead can select a preset at OS-2, which expands to the corresponding profile, policy, and team configuration:

```
User: "Quick fix: typo in README"
Lead internal: apply_preset("quick-fix")
  -> session_profile = "unstructured"
  -> enforcement_policy = "permissive"
  -> team.strategy = "solo"
```

Presets do not introduce new configuration -- they are convenience mappings to existing configuration values.

---

## 13. Integration with Existing Settings

### 13.1 Relationship to `.claude/settings.json`

PathFlow configuration (`pathflow-config.json`) is separate from and complementary to Claude Code's native settings:

| Concern | Configured In | Owned By |
|---------|--------------|----------|
| Hook definitions (which hooks run, in what order) | `.claude/settings.json` | Claude Code |
| Permission lists (allow/ask/deny) | `.claude/settings.json` | Claude Code |
| StatusLine command | `.claude/settings.json` | Claude Code |
| Agent Teams env var | `.claude/settings.json` | Claude Code |
| **Pathway node behavior** | `pathflow-config.json` | **PathFlow** |
| **Enforcement levels per node** | `pathflow-config.json` | **PathFlow** |
| **Session profiles** | `pathflow-config.json` | **PathFlow** |
| **Teammate composition** | `pathflow-config.json` | **PathFlow** |
| **Teammate blueprints** | `.claude/agents/cf-{role}.md` | **PathFlow** |

### 13.2 Settings Templates Synchronization

CodeFlow's existing settings templates (`.claude/settings-templates/strict.json`, `standard.json`, `autonomous.json`, `permissive.json`) control Claude Code's native settings. They do NOT control PathFlow configuration.

However, there is a conceptual alignment:

| Settings Template | Suggested PathFlow Policy |
|-------------------|---------------------------|
| `strict.json` | `strict` enforcement policy |
| `standard.json` | `standard` enforcement policy |
| `autonomous.json` | `standard` enforcement policy |
| `permissive.json` | `permissive` enforcement policy |

The PathFlow config can reference the active settings template to auto-select a matching enforcement policy, but this is a convenience -- they are independent configuration domains.

---

## Appendix A: Hardcoded Defaults (Layer 0)

These values are built into the PathFlow engine and used when no configuration file exists:

```json
{
  "enforcement_policy": "standard",
  "sentinel_ttl_session": 14400,
  "sentinel_ttl_short": 600,
  "checkpoint_max_reopens": 3,
  "checkpoint_force_allow": true,
  "dynamic_insertion_allowed": true,
  "dynamic_insertion_max": 3,
  "audit_all_overrides": true,
  "session_profile": "interactive",
  "team_strategy": "auto-detect",
  "max_teammates": 3,
  "context_warning_threshold": 0.70,
  "context_recycle_threshold": 0.85,
  "verification_tier": "auto-detect",
  "auto_push": "prompt",
  "auto_pr": "prompt",
  "meta_awareness": "full",
  "resume_behavior": "prompt-always"
}
```

**Design principle**: Hardcoded defaults produce a safe, interactive session with standard enforcement. A session with zero configuration behaves like a well-supervised interactive development session.

---

## Appendix B: Open Questions

### B1: Configuration File Protection

`pathflow-config.json` is a sensitive file -- modifying it changes enforcement behavior. Should it be protected by the security management hooks (like other `.codeflow/config/` files)?

**Recommendation**: Yes. Add `pathflow-config.json` to the protected paths list. Modifications require the `cf-security-management:modify-protected-resource` workflow (stage -> review -> apply).

### B2: Per-Teammate Configuration

Currently, teammate behavior is configured via blueprints (`.claude/agents/cf-{role}.md`) and composition (in `pathflow-config.json`). Should there be per-teammate runtime configuration beyond persistence?

**Recommendation**: No for initial implementation. Blueprints define behavior; the team lead manages teammates via messages and task assignments. Adding per-teammate config knobs would add complexity without clear benefit.

### B3: Configuration Versioning

Should `pathflow-config.json` be versioned? If the schema changes between PathFlow versions, old configs could break.

**Recommendation**: Yes. The `"$schema": "pathflow-config-v1"` field enables version detection. PathFlow engine should validate the schema version at session start and warn (not block) on version mismatch.

### B4: Multi-Pathway Session Configuration

The outer shell design raises the question of multi-traversal sessions (OS-7 looping back to OS-4). If implemented, the configuration would need to support per-traversal overrides (e.g., first pass uses `structured`, second pass uses `unstructured`).

**Recommendation**: Defer. Single-traversal is the initial implementation. Multi-traversal configuration can be added as a Layer 3 runtime override pattern when the feature is implemented.
