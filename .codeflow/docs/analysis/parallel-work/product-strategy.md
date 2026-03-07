---
title: "Product Strategy and Competitive Analysis"
type: analysis
status: draft
author: cf-documentation
created_at: "2026-03-05"
updated_at: "2026-03-07"
parent: "parallel-work/README.md"
---

# Product Strategy and Competitive Analysis

[← Back to Overview](README.md)

## Table of Contents

- [1. Overview](#1-overview)
- [2. Competitive Landscape](#2-competitive-landscape)
- [3. Differentiation](#3-differentiation)
- [4. Multi-Model Strategy](#4-multi-model-strategy)
- [5. Target Users](#5-target-users)
- [6. Pricing Model](#6-pricing-model)
- [7. Open-Core Strategy](#7-open-core-strategy)
- [8. Risk Assessment](#8-risk-assessment)
- [9. Strategic Recommendations](#9-strategic-recommendations)
- [10. Epic D: Model Orchestrator](#10-epic-d-model-orchestrator)
- [11. Epic E: CodeFlow App](#11-epic-e-codeflow-app)

---

## 1. Overview

CodeFlow is a structured AI development workflow framework built on top of Claude Code. It adds PathFlow phase enforcement, multi-agent team coordination, a three-tier memory system, and (upcoming) cross-project intelligence to the existing Claude Code foundation.

**Positioning:** CodeFlow occupies the space between raw AI coding tools (Copilot, Claude Code) and fully autonomous agent platforms (Factory.ai, Devin). It is human-directed with structure — the developer remains in control, and CodeFlow enforces workflow discipline that makes AI-assisted development predictable and auditable.

**Market context (March 2026):**
- 85% of developers use AI tools in some capacity
- GitHub Copilot holds 42% market share in AI coding tools
- Claude Code authoring approximately 4% of GitHub commits, heading toward 20% by end-2026 (Anthropic Agentic Coding Trends Report, 2026)
- Engineers use AI in approximately 60% of their work, but fully delegate only 0-20% of tasks
- Workflow automation market valued at $26B in 2026 (Mordor Intelligence)
- VS Code is building multi-agent development features natively

The central tension: AI tools are powerful but undisciplined. CodeFlow's thesis is that structured workflow + AI = predictable outcomes.

---

## 2. Competitive Landscape

### Autonomy vs Structure Map

```text
High Autonomy
      |
      |   Factory.ai    Devin 2.0
      |   (Droids)      (autonomous agent)
      |
      |                              Ruflo / ccswarm
      |                              (open-source orchestration)
      |
      |                                          CodeFlow
      |                                          (structured, human-directed)
      |
      |   Cursor / Copilot / Windsurf
      |   (AI-assisted editing)
      |
Low Autonomy ---------------------------------> High Structure
```

### Factory.ai

- **Model:** Enterprise autonomous agent platform, Droids execute tasks independently
- **Pricing:** $20/month starting, enterprise tiers (custom pricing)
- **Funding:** $100M+
- **Target:** Enterprise engineering teams seeking to automate repetitive tasks
- **Strength:** Polished product, enterprise sales motion, proven at scale
- **Weakness:** High cost at enterprise tier, black-box automation, requires trust in autonomous execution

### Devin 2.0

- **Model:** Autonomous software engineer agent
- **Pricing:** $20/month (down from $500), $2.25/ACU (agent compute unit)
- **Valuation:** $2B
- **Target:** Teams wanting to fully delegate coding tasks
- **Strength:** End-to-end task completion, strong brand recognition
- **Weakness:** Still requires significant oversight, expensive for complex tasks, limited team workflow integration

### Cursor / GitHub Copilot / Windsurf

- **Model:** IDE-level AI assistance
- **Pricing:** ~$20/month per developer
- **Target:** Individual developers wanting AI acceleration in their editor
- **Strength:** Seamless editor integration, broad adoption
- **Weakness:** No workflow enforcement, no team coordination, no cross-project intelligence

### Ruflo

- **Model:** Open-source Claude Code orchestration
- **Features:** Multi-agent swarms, MCP integrations, plugin marketplace
- **Pricing:** Open-source (free)
- **Strength:** Community-driven, extensible, no cost
- **Weakness:** No commercial support, no persistence/memory, loose coordination

### ccswarm

- **Model:** Open-source, git worktree isolation, specialized agents
- **Features:** Worktree-per-agent isolation, role-based agent routing
- **Pricing:** Open-source (free)
- **Strength:** Clean architecture, git-native approach
- **Weakness:** Early stage, no hosted offering, limited documentation

### Claude Code Agentrooms

- **Model:** Open-source, specialized routing, web UI
- **Features:** Multi-agent coordination with web interface
- **Pricing:** Open-source (free)
- **Strength:** Visual interface, accessible to non-CLI developers
- **Weakness:** Web UI adds complexity, no persistent memory

### CodeFlow's Unique Position

CodeFlow occupies a currently uncontested position: structured, human-directed AI development with persistent cross-project intelligence.

| Capability | Factory.ai | Devin | Copilot | Ruflo/ccswarm | CodeFlow |
|------------|-----------|-------|---------|---------------|----------|
| Workflow enforcement | Partial | No | No | No | Yes (PathFlow) |
| Multi-agent coordination | Yes | No | No | Yes | Yes |
| Persistent memory | Yes | Limited | No | No | Yes (3-tier) |
| Cross-project intelligence | Unknown | No | No | No | Yes (Epic C) |
| Human-directed | No | No | Yes | Partial | Yes |
| Open framework | No | No | No | Yes | Yes |
| Subscription leverage | No | No | No | No | Yes |

---

## 3. Differentiation

### Process Design (Defensibility: HIGH — hard to replicate)

PathFlow phases, quality pipeline (WS-DEV → WS-REV → WS-QA), and enforcement hooks are not features — they are a methodology. Replicating the method requires understanding why each stage exists and how enforcement prevents violations. This is institutional knowledge encoded in 21 hook scripts, 14 commands, and agent definitions.

**Why hard to replicate:** Process design is invisible to competitors until they see the output. By the time a competitor copies the structure, CodeFlow has iterated to the next version.

### Multi-Model Orchestration (Defensibility: MEDIUM)

Routing different work stages to different AI models (Claude Code for planning, Codex for code generation, Gemini for review) based on cost/quality trade-offs is a significant advantage for teams. No current competitor offers config-driven model routing within a structured workflow.

**Why medium:** Multi-model routing is architecturally straightforward. Once proven, competitors can replicate the approach. The defensibility comes from the combination with PathFlow enforcement, not the routing alone.

### Cross-Project Intelligence (Defensibility: HIGH — no competitor has this)

The global intelligence layer (Epic C) aggregates context across all repositories a team works on, enables semantic search across past decisions, and supports graph queries across project relationships. This becomes more valuable the longer a team uses it — the intelligence grows with usage.

**Why high:** Building a semantic knowledge graph of a team's development history is a multi-year investment. Early movers accumulate data advantage. No current competitor offers this.

### Three-Tier Data Model (Defensibility: MEDIUM)

JSONL as rebuild authority, SurrealDB as query interface, and Markdown as human-readable views is a proven architectural pattern. The combination provides auditability, queryability, and human readability simultaneously.

### Config-Driven Flexibility (Defensibility: HIGH)

The enforcement-policy.json, pathflow-config.json, and agent definition files make CodeFlow's behavior fully configurable. Teams can tune phase limits, rework iterations, parallel batch sizes, and model routing without changing code.

**Why high:** Config-driven systems accumulate community configurations and patterns that become a moat. Teams share their CodeFlow configs, creating a community knowledge base.

---

## 4. Multi-Model Strategy

### Claude Code as Control Plane

Claude Code (with CodeFlow) remains the control plane: PathFlow orchestration, hook enforcement, task management, and team coordination. Claude Sonnet/Opus models handle planning, review, and orchestration decisions.

External models are execution workers: they receive specific, bounded tasks from the control plane and return results.

### External Models via tmux T1/T3 Execution

External models (Codex, Gemini, Ollama, local models) run in tmux panes managed by the `codeflow` binary. Two execution tiers:

| Tier | Mode | Use Case | Latency |
|------|------|----------|---------|
| T1 | Interactive | Visible to developer, can intervene | Higher |
| T3 | Background | Automated worker, no interaction | Lower |

The cf-model-orchestrator skill (to be revived in Rust, Epic D) handles model selection, prompt formatting, response parsing, and result routing back to PathFlow.

### Work Stage to Model Routing

Config-driven routing in `.codeflow/config/orchestration.toml` (proposed):

```toml
[stage_routing]
WS-DEV = "claude-sonnet-4-6"          # Default: Claude for complex implementation
WS-DEV.alternative = "codex-1"        # Override: Codex for routine code generation
WS-REV = "claude-opus-4-6"            # Review always uses Opus
WS-PLAN = "claude-opus-4-6"           # Planning always uses Opus
WS-QA = "claude-sonnet-4-6"           # QA can use Sonnet
```

The routing is per-stage and per-work-type, allowing fine-grained cost optimization.

### Subscription Leverage: The Core Pitch

Teams using Claude Code ($20/dev/month subscription) pay per seat, not per API token. External models like Codex also offer subscription plans. CodeFlow enables routing expensive tasks (planning, review) to subscription-plan models and cheap tasks (routine code generation) to lower-cost alternatives.

**Cost comparison for a 5-person team per month:**

| Scenario | Setup | Monthly Cost |
|----------|-------|-------------|
| API tokens only | Claude API + Codex API + custom integration | $150-400 depending on usage |
| Subscriptions + CodeFlow | Claude Code ($20 × 5) + Codex subscription + CodeFlow ($10 × 5) | $200 fixed |
| Subscriptions only, no orchestration | Claude Code × 5 + Codex × 5 | $200 but no routing optimization |

**The pitch:** "Stop burning API tokens. Use your existing subscriptions more effectively with CodeFlow routing."

The subscription leverage story is strongest for teams already paying for Claude Code and one or more other AI tools. CodeFlow orchestrates what they already have.

---

## 5. Target Users

### User Segments

| Segment | Need | Willingness to Pay | Price Sensitivity |
|---------|------|-------------------|-------------------|
| Solo dev (Claude Code user) | Better workflow discipline, memory across sessions | Low-Medium | High ($10/mo is viable) |
| Small team (2-5 devs) | Coordination, shared context, multi-model cost optimization | Medium | Medium ($10-15/dev/mo) |
| Mid team (5-15 devs) | Cross-project intelligence, analytics, team-wide patterns | Medium-High | Low-Medium ($15-25/dev/mo) |
| Enterprise (15+) | Dashboard, audit trail, compliance, SLA support | High | Low (budget-driven) |

### Primary Target: Small Teams (2-5 Developers)

The highest-value early adopters are small teams already using Claude Code who are experiencing coordination pain:

- Two developers starting Claude Code sessions in the same repo and overwriting each other's state
- Context lost between sessions on the same task
- Review quality inconsistent without a structured pipeline
- Multiple AI tools (Claude Code, Copilot, Codex) with no coordination layer

These teams can justify $10/dev/month ($50/month for a 5-person team) without procurement processes. They can self-serve from documentation. They become advocates.

### Secondary Target: Solo Developers Using Claude Code

Solo developers using Claude Code are the adoption funnel. They discover CodeFlow when searching for Claude Code workflow improvements. The free/OSS tier (project-local features) captures them; the paid tier (global DB, multi-model) converts power users.

---

## 6. Pricing Model

### Base Price

$10/developer/month or $100/developer/year (17% annual discount).

### Competitive Context

| Product | Price | Model |
|---------|-------|-------|
| Factory.ai | $20/month+ | Autonomous agent platform |
| Devin 2.0 | $20/month + $2.25/ACU | Autonomous agent |
| GitHub Copilot | $19/month | IDE AI assistance |
| Cursor Pro | $20/month | IDE AI assistance |
| Ruflo | Free (open-source) | Open-source orchestration |
| CodeFlow | $10/month | Structured workflow + intelligence |

CodeFlow at $10/month positions as:
- Half the cost of Copilot/Cursor (addresses a different need)
- An add-on to Claude Code ($20), not a replacement
- Priced below the impulse purchase threshold for individual developers

**Combined cost pitch:** Claude Code ($20) + CodeFlow ($10) = $30/dev/month for structured, multi-agent, cross-project AI development.

### Revenue Scenarios

| Monthly Active Users | MRR | ARR |
|---------------------|-----|-----|
| 100 | $1,000 | $12,000 |
| 500 | $5,000 | $60,000 |
| 1,000 | $10,000 | $120,000 |
| 5,000 | $50,000 | $600,000 |
| 10,000 | $100,000 | $1,200,000 |

5,000 paying users ($600K ARR) is a viable indie product. 10,000 users ($1.2M ARR) is a small company.

### Suggested Tier Structure

| Tier | Price | Includes |
|------|-------|----------|
| Community | Free / OSS | CLI binary, project-local SurrealDB, PathFlow, multi-agent teams, full framework |
| Pro | $10/dev/month | Global intelligence layer (daemon + cross-project queries + embeddings), multi-model orchestration, priority support |
| Team | $25/dev/month (future) | Dashboard (SvelteKit + Tauri), team analytics, admin controls, shared configurations |

The tier boundary is clean: **project-local features are free**. **Infrastructure features (daemon, dashboard) are paid**.

---

## 7. Open-Core Strategy

### Free Layer Drives Adoption

The Community tier is fully functional for solo developers and single-project teams. It includes:

- Complete `codeflow` CLI binary
- PathFlow phase enforcement (all 7 phases)
- Multi-agent team coordination (all 8 teammates)
- Project-local SurrealDB embedded
- Three-tier memory model
- 21 hook enforcement scripts
- Full test suite (1,555+ tests)

This is not a crippled version. It is a complete, production-quality tool that solves real problems. Developers who adopt Community become advocates before ever seeing a paywall.

### Pro Tier Captures Teams

Teams experience coordination pain when they have multiple repositories or multiple developers. The Pro tier addresses this directly:

- Global intelligence layer: daemon + cross-project visibility
- Multi-model orchestration: subscription leverage
- Support: response SLA

Teams converting from Community to Pro have already validated that the product solves their problem. The conversion friction is low.

### Tier Boundary: Project-Local vs Infrastructure

The cleanest tier boundary is:

- **Free:** Everything that runs within a single project repository
- **Paid:** Infrastructure that spans projects or requires server components

This boundary is intuitive to developers. "Local tools are free; hosted tools are paid" is a widely understood model (ngrok, Tailscale, etc.).

### Open-Source vs Paid Decision: Deferred

The final decision on whether the CLI source is open-source (Apache 2.0 / MIT) or source-available is explicitly deferred. The priority is:

1. Build an excellent tool that the team uses
2. Find early adopters organically
3. Validate that the paid tier creates enough value to justify commercialization
4. Then decide on open-source licensing vs commercial licensing

Building in public (even without a formal open-source license) is compatible with this timeline. A formal open-source release accelerates adoption but reduces revenue options.

---

## 8. Risk Assessment

### Platform Dependency on Claude Code

**Risk:** CodeFlow is deeply coupled to Claude Code's agent framework (teammate spawning, SendMessage, TaskCreate, hook events). If Anthropic changes these APIs or deprecates the agent framework, CodeFlow's core architecture breaks.

**Likelihood:** Medium (Anthropic is actively expanding Claude Code capabilities, not contracting them)

**Impact:** High (requires significant rearchitecting)

**Mitigation:** Multi-model orchestration reduces dependency — external model workers run via tmux, not Claude Code subagents. The PathFlow logic and data model are independent of Claude Code. The hook framework is implemented in the `codeflow` Rust binary, not in Claude Code internals. A future migration from Claude Code to a different control plane is possible (high effort, but not impossible).

### Competition from Free Alternatives

**Risk:** Ruflo, ccswarm, and Agentrooms are open-source alternatives with growing communities. If one matures significantly, it may capture the market that CodeFlow targets.

**Likelihood:** Medium (open-source projects often lack sustained maintenance)

**Impact:** Medium (reduces total addressable market, but differentiated features still valuable)

**Mitigation:** Cross-project intelligence (Epic C) is a significant technical moat. Open-source alternatives are unlikely to invest in a global daemon + vector search + graph queries architecture without commercial backing. The process design (PathFlow) is also differentiated — it embodies institutional knowledge that takes time to replicate.

### Anthropic Building Features Natively

**Risk:** Anthropic adds team coordination, persistent memory, and workflow enforcement to Claude Code itself, reducing CodeFlow's differentiation.

**Likelihood:** High in 2-3 year timeframe (VS Code is already building multi-agent development features natively; Anthropic tracks these trends)

**Impact:** High if they replicate core PathFlow features

**Mitigation:** CodeFlow's memory architecture (JSONL + SurrealDB + Markdown), cross-project intelligence layer, and config-driven enforcement are specific design choices that Anthropic is unlikely to replicate exactly. The open-core model (free tier) means CodeFlow can survive as a community tool even if commercialization becomes unviable.

### Market Timing

**Risk:** The market for structured AI development workflow tools is nascent. Developers may not yet feel enough pain to pay for workflow discipline.

**Likelihood:** Medium (85% adoption with 0-20% delegation suggests the pain of under-delegation is not yet acute)

**Impact:** Medium (adoption slower than expected, but eventual market exists)

**Mitigation:** Building for own use is the hedge. If the market does not develop, CodeFlow remains a valuable personal tool. If it does develop, CodeFlow is positioned as the early mover with the most production experience.

### Engineering Burden of Paid Product

**Risk:** Adding payment infrastructure, user accounts, license key validation, and support SLA requires significant effort that diverts from feature development.

**Likelihood:** High if commercialization proceeds

**Impact:** Medium (detracts from core product velocity)

**Mitigation:** Use a managed billing platform (Stripe, Paddle) with minimal custom code. Launch with annual/monthly subscriptions and honor-system enforcement initially (trust users, add technical enforcement later). Defer dashboard and analytics until revenue justifies the investment.

---

## 9. Strategic Recommendations

Ordered by confidence:

### 1. Keep Building for Own Use (Confidence: HIGH)

CodeFlow solves a real problem — the authors use it daily. Building a tool that the team relies on is the best way to ensure it remains high quality and relevant. Every engineering decision made for own-use is validated by immediate feedback.

**Action:** Continue the current epic roadmap (Epic 0 → A/B/C → D → E) prioritized by own-use value.

### 2. Open-Source the Framework Layer (Confidence: HIGH)

The PathFlow framework, agent team architecture, and hook enforcement system are valuable to the Claude Code community beyond CodeFlow. Open-sourcing (or publishing under a permissive license) the core framework drives adoption and community contributions.

**Action:** After Epic 0 (Rust CLI) is complete, evaluate a public repository release. Even without formal open-source licensing, publishing the code creates community.

### 3. If Productizing, Product is Intelligence Platform, Not CLI (Confidence: MEDIUM)

The cross-project intelligence layer (Epic C) is the highest-defensibility feature. If CodeFlow becomes a commercial product, the pitch should be:

"CodeFlow is the intelligence platform that makes your team's AI development history queryable and actionable — across all your repositories, past and present."

The CLI is the delivery mechanism. The intelligence is the product.

**Action:** After Epic C ships, evaluate whether cross-project intelligence creates enough standalone value for a paid tier.

### 4. Consider "Standard Not Product" Path (Confidence: MEDIUM)

An alternative to commercial product is establishing PathFlow as an industry standard for structured AI development. Published specification, community implementations, conference talks.

This path trades revenue potential for influence and adoption. Standards authors often benefit indirectly through consulting, training, and enterprise services.

**Action:** Monitor community interest. If other teams start adopting PathFlow independently, accelerate the standard path.

### 5. Do Not Compete with Factory.ai or Devin (Confidence: HIGH)

Factory.ai and Devin are well-funded autonomous agent platforms targeting enterprise customers willing to pay for full automation. CodeFlow's human-directed, structured workflow is fundamentally different — it is not a step on the path to full autonomy, it is a different thesis about how AI development should work.

Competing on automation capabilities means building a product that makes CodeFlow's core thesis (human direction + structure) irrelevant.

**Action:** Explicitly position against autonomous agents: "CodeFlow is for developers who want AI assistance, not AI replacement."

---

## 10. Epic D: Model Orchestrator

Epic D: Model Orchestrator (12 tasks, 3 phases)

**Purpose:** Build a config-driven model orchestration layer in Rust that routes work stages to different AI models. Claude Code remains the control plane; external models (Codex CLI, Gemini CLI, Ollama, etc.) are execution workers invoked via CLI subprocesses. The team lead calls `codeflow orchestrate exec` -- the orchestrator is CLI tooling, not a teammate or daemon service.

**Dependency:** Epic 0 (Rust CLI) must be complete. Epic C (global DB) provides session tracking for multi-model sessions.

### Phase D1: Orchestration Core (5 tasks)

| # | Task | Description | Effort |
|---|------|-------------|--------|
| D1 | Orchestration config schema | `.codeflow/config/orchestration.toml` -- stage-to-model routing tables, model profiles, precedence rules | M |
| D2 | Model profile registry | Built-in profiles (Claude subagent, Codex CLI, Gemini CLI, Ollama local) + custom extensibility via TOML profile definitions | M |
| D3 | Stage routing engine | Map `(work_stage, work_type)` -> `model_profile` with precedence rules: task override > work_type override > stage default > global default | M |
| D4 | T1 one-shot execution | `std::process::Command` for quick tasks -- no tmux, direct stdout/stderr capture, timeout management, exit code handling | L |
| D5 | T3 persistent session | tmux session management with naming convention `model-{ulid}`, multi-turn exchanges, file modification support, session cleanup | L |

### Phase D2: Execution Engine (4 tasks)

| # | Task | Description | Effort |
|---|------|-------------|--------|
| D6 | PTY-based subprocess stdout capture | `portable-pty` crate for cross-platform PTY allocation, raw output streaming, encoding handling | L |
| D7 | Response normalization and completion detection | Parse model outputs into `ModelResponse` struct, detect completion markers per model profile, handle timeout/error states | L |
| D8 | Process lifecycle management | Spawn, monitor, restart, kill model CLI subprocesses. Health checks. Graceful shutdown. Resource cleanup on session end | M |
| D9 | CLI commands | `codeflow orchestrate exec` (run a model for a stage), `codeflow orchestrate status` (active model sessions), `codeflow orchestrate kill` (terminate a model session) | M |

### Phase D3: Tracking (3 tasks)

| # | Task | Description | Effort |
|---|------|-------------|--------|
| D10 | Model session tracking in SurrealDB | Extend `active_work` table with `model_profile` and `cost` fields, record which model handled each stage | S |
| D11 | Multi-model audit log | JSONL ledger events for `model_selected`, `stage_handoff`, `completion` -- full audit trail of model routing decisions | S |
| D12 | Cost tracking and subscription configuration | Budget management per model profile, subscription plan config (which models are subscription vs API-token), usage aggregation and reporting | M |

### Key Architectural Points

- **Claude Code remains the control plane.** PathFlow orchestration, hook enforcement, task management, and team coordination are unchanged. External models are execution workers only.
- **T1 uses `std::process::Command`, NOT tmux.** One-shot execution is more reliable without tmux overhead. Direct process spawning, stdout capture, and exit code handling.
- **T3 uses tmux** with naming convention `model-{ulid}` for persistent multi-turn sessions that require file modification support.
- **Config-driven routing** in `.codeflow/config/orchestration.toml` -- teams configure which model executes which stage without code changes.
- **Subscription leverage pitch:** "Stop burning API tokens. Use your existing model subscriptions more effectively with CodeFlow routing." Teams already paying for Claude Code ($20/dev/month) and Codex can route planning/review to Claude (reasoning quality) and routine code generation to Codex (throughput). Same subscriptions, better cost/quality ratio.

---

## 11. Epic E: CodeFlow App

Epic E: CodeFlow App (~18-20 tasks, 5 phases)

**Purpose:** A desktop application that serves as the unified interface for all AI-assisted development -- streaming terminal output from multiple model CLIs, project management views, configuration management, cross-project intelligence, and standalone model playground. The app complements the terminal, not replaces it. The CLI works independently; the app provides a unified view when used.

**Technology:** Tauri v2 + SvelteKit. ~50MB binary (vs Electron ~300MB). System webview (WebKit on macOS, WebKitGTK on Linux). Rust backend imports `codeflow-core` directly as a crate dependency. SvelteKit has minimal frontend boilerplate.

**Dependency:** Epic 0 + Epic C (daemon + SurrealDB data). Epic D (execution engine + PTY capture) required for streaming terminal features. Phases E1, E3, E4 can start when Epic C Phase C1-C2 is stable (daemon + project registry). Phase E2 requires Epic D Phase D1-D2 (execution engine + PTY capture). Phase E5 requires Epic C Phase C3-C4 (embeddings, knowledge graph).

### Phase E1: App Foundation (4 tasks)

| # | Task | Description | Effort |
|---|------|-------------|--------|
| E1 | Tauri v2 + SvelteKit project setup | Workspace integration with `codeflow-core` as crate dependency, build pipeline, dev tooling | M |
| E2 | Daemon Unix socket client | Connect to `~/.codeflow/db.sock`, SurrealQL query interface, connection health monitoring | M |
| E3 | Multi-project selector | Sidebar with project accordion, registered projects from daemon, per-project session views with PathFlow phase/branch/work_type display, cross-project search | M |
| E4 | App shell | Window management, menu bar, navigation, tab management, theme support | S |

### Phase E2: Streaming Terminal (5 tasks)

| # | Task | Description | Effort |
|---|------|-------------|--------|
| E5 | xterm.js terminal emulator integration | Same technology as VS Code, framework-agnostic, works in any webview | L |
| E6 | PTY bridge | Tauri backend captures PTY output from model CLIs, streams to xterm.js via Tauri IPC events | L |
| E7 | Multi-tab stream management | Each model CLI gets its own tab, tab lifecycle tied to model process lifecycle | M |
| E8 | Sub-stream inline embedding | Detect delegation markers in Claude Code's output, create inline collapsible panel showing delegated model's stream | L |
| E9 | Stream controls | Pause/resume streaming, scrollback buffer, search within stream, copy selection | M |

### Phase E3: Project Management (4 tasks)

| # | Task | Description | Effort |
|---|------|-------------|--------|
| E10 | Kanban board | Task cards by status columns (OPEN, IN_PROGRESS, REVIEW, DONE), drag-and-drop reordering | M |
| E11 | Epic timeline/roadmap view | Phase progress bars, task completion tracking, dependency visualization | M |
| E12 | Task detail view | Criteria status matrix, stage reports, PR link, acceptance criteria checklist | M |
| E13 | Task dependency graph | Force-directed graph layout (D3.js or vis-network), interactive zoom/pan/filter | L |

### Phase E4: Configuration (3 tasks)

| # | Task | Description | Effort |
|---|------|-------------|--------|
| E14 | Config file reader/writer | Tauri backend reads/writes config via filesystem + serde, watches for external changes | M |
| E15 | Form-based config viewers | Structured viewers for `enforcement-policy.json`, `pathflow-config.json`, `orchestration.toml`, `project.toml`, `codeflow-knowledge.toml` | L |
| E16 | READ-ONLY config with change request | All config views are read-only by default. "Request Change" button pre-fills a prompt for the active Claude Code session to handle the edit through PathFlow. Direct editing is the LLM's job. | M |

### Phase E5: Intelligence (4 tasks)

| # | Task | Description | Effort |
|---|------|-------------|--------|
| E17 | Semantic search view | Query input -> local ONNX embedding -> daemon HNSW vector query -> ranked results | M |
| E18 | Cross-project task board | Aggregate tasks across projects with `can_access` visibility scoping, filterable by project/status/type | M |
| E19 | Knowledge graph explorer | Interactive entity/relationship browser, expandable nodes, relationship type filtering | L |
| E20 | Team analytics dashboard | Session frequency, stage pass rates, rework rates, model usage breakdown, API cost tracking | L |

### Additional Features

| Feature | Description |
|---------|-------------|
| File viewer | highlight.js/shiki for read-only syntax-highlighted code view |
| Diff viewer | diff2html for git diff rendering (GitHub PR-style side-by-side and unified views) |
| "Open in Editor" button | Auto-detect installed editors (code, zed, idea) via `which`, dropdown selection |
| Playground mode | Sidebar section for standalone model CLIs without PathFlow -- just spawn the CLI in a PTY, no session wrapping. Reinforces the app as the single place for all AI interactions. |

### Streaming Modes

Two complementary streaming architectures:

| Mode | Description | Use Case |
|------|-------------|----------|
| Separate tabs | One xterm.js terminal per PTY, user switches between tabs | Independent model sessions, playground mode |
| Sub-stream embedding | App detects delegation markers in Claude Code's output, creates inline collapsible panel showing delegated model's stream in real-time | Orchestrated multi-model sessions where Claude Code delegates to external models |

### Tech Stack Rationale

| Choice | Rationale |
|--------|-----------|
| Tauri v2 | ~50MB binary (vs Electron ~300MB), system webview (WebKit macOS, WebKitGTK Linux), Rust backend imports `codeflow-core` directly |
| SvelteKit | Minimal boilerplate, reactive UI, SSR not needed (local app), smaller bundle than React/Vue |
| xterm.js + WebGL renderer | Battle-tested terminal emulator (VS Code uses it), framework-agnostic, GPU-accelerated rendering in any webview |
| portable-pty | Cross-platform PTY abstraction (shared with Epic D), `openpty` on macOS, `/dev/ptmx` on Linux |
| vte (wezterm) | VT100/ANSI escape sequence parsing in Rust, pre-processes raw PTY output before sending to frontend |

### Hybrid PTY Architecture

The terminal rendering uses a hybrid Rust+JS approach for optimal performance, mirroring the architecture VS Code uses (native PTY handling + xterm.js rendering):

**Backend (Tauri Rust):**
- `portable-pty` crate -- cross-platform PTY abstraction (`openpty` on macOS, `/dev/ptmx` on Linux)
- `vte` crate (from wezterm project) -- VT100/ANSI escape sequence parsing in Rust, pre-processes raw PTY output before sending to frontend
- Sends pre-parsed structured content to frontend via Tauri IPC

**Frontend (webview):**
- xterm.js with WebGL renderer addon -- GPU-accelerated terminal rendering in the webview
- Receives pre-parsed content from Rust backend, reducing JS-side processing overhead

Rust handles the performance-critical I/O and parsing; xterm.js handles the proven rendering in the webview.

### Terminal Rendering Strategy

- **Start with raw mode** (xterm.js, zero extra work) -- displays output exactly as the terminal shows it (ASCII tables, ANSI colors, monospace layout). This is the baseline for Phase E2.
- **Add rich rendering as a Phase E2 enhancement** -- detect markdown blocks in the PTY stream, render as HTML components (styled tables, syntax-highlighted code blocks, clickable links).
- **Toggle between "raw terminal" and "rich view"** per user preference. Raw mode is always available as fallback.
- **Rich rendering requires a markdown parser** (e.g., `marked` or `markdown-it`) to intercept and transform markdown patterns in the stream before display.

### UI/UX Design Quality

The CodeFlow App targets the visual polish and interaction quality of best-in-class desktop development tools (Linear, Obsidian, VS Code). The Tauri + SvelteKit stack imposes no visual ceiling -- the app renders in a system webview (WebKit on macOS, WebKitGTK on Linux) with full CSS/HTML spec support.

**Design principles:**
- **Native-feeling interactions:** Window vibrancy/translucency via Tauri's macOS vibrancy API, native menu bar and system tray, drag-and-drop with native feel, keyboard shortcuts matching OS conventions (Cmd on macOS, Ctrl on Linux)
- **Dark/light mode** following OS preference automatically (`prefers-color-scheme`)
- **System font rendering** (no web font flash), component-level scoped CSS
- **Smooth 60fps animations** using Svelte's compiled `transition`/`animate` directives
- **Micro-interactions:** Card entrance animations, tab transitions, hover feedback, focus glow on inputs
- **Visual hierarchy:** Box shadows for depth/elevation, color-coded work type badges (FEAT=blue, FIX=red, PLAN=purple, DOCS=teal), left-side color stripes on kanban cards
- **Status communication:** Animated thinking indicators, connection status in status bar, phase progress badges
- **Chat-style input** with send button, "Enter to send / Shift+Enter for newline" convention
- **Stream header** with model badge, session info (branch + PathFlow phase), and action controls (Stop, Clear, Restart)

**Reference implementations for design quality benchmarks:**

| Reference | What to Study |
|-----------|---------------|
| Linear | Task management polish, keyboard-first UX |
| Obsidian | Themeable, plugin-friendly, local-first |
| VS Code | Terminal integration, multi-panel, extension ecosystem |
| Warp | AI-native terminal, rich rendering of command output |

### Impact on Previous Epics

- **Epic 0:** No changes.
- **Epic A:** No changes. Loro CRDT is independent; future app enhancement for real-time collaboration post-E.
- **Epic B:** No changes.
- **Epic C:** Minor -- daemon IPC server (Task 003) could extend with WebSocket upgrade path for persistent streaming. Process supervision API could extend daemon process manager (Task 002). These extensions are backward-compatible and can live in Epic D Phase D2 instead to avoid scope creep on Epic C's existing tasks.
- **Epic D:** Shared infrastructure -- `portable-pty` crate and PTY capture code from Phase D2 is reused directly by Epic E Phase E2.

### Updated Dependency Graph

```text
Epic 0 (prerequisite)
    |
    +---> Epic A (parallel execution)
    +---> Epic B (data standardization)
    +---> Epic C (global intelligence)
              |
              +---> Epic D (model orchestration -- depends on 0 + C)
                        |
                        +---> Epic E (CodeFlow App -- depends on 0 + C + D)
```
