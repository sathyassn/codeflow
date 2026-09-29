# Delegated Claude turn rules: duty map

Status: evidence for TSK-163 AC-1. TSK-138 found that the cf-delegate
lifecycle lane restated three rules that the Claude turn adapter owns: the
launch sequence, turn detection and the sibling Stop-hook preflight. The
copies had drifted: the lane ran the preflight "Before delivery", the
adapter "Before launching". Both files have one reader, an agent on a
non-Claude host at successive moments of one task, so one home serves it.
This task states each rule once, in the adapter, with the preflight as a
step after `init` and before launch. The lane keeps its host preflight, its
pointer to the adapter, the Herdr variant and its evidence section.

Paths below are under `assets/base/claude/skills/cf-delegate/`. `lane`
means `resources/lane-lifecycle.md`; `adapter` means
`resources/claude-turn-completion.md`.

## Where the reader reaches the rules

```text
Codex host, about to delegate to Claude
  SKILL.md  "From codex (Codex host)" row
    -> lane   Preflight (claude, TTY host, canary), then Lifecycle:
              "before launching Claude, read and follow the shipped
               turn lifecycle adapter"
      -> adapter
           Per-run setup                 run id, state dir, init, immutable settings
           Sibling Stop-hook preflight   after init, before launch
           Launch and drive one turn     launch, arm, deliver, retry, waits
           Stable exit states            exit codes, poison and recovery
           Sequential turns              one armed turn, the next turn
Codex, Grok or other non-Claude host, launching a Claude worker
  cf-model-orchestrator SKILL.md and routing/effort.md -> adapter
Claude host
  SKILL.md -> resources/lane-plugin.md   (reads neither file)
```

The lane pointer comes before any launch the lane describes, so a Codex host
reads the adapter before it starts Claude. The Claude host path is
unchanged: `no_claude_host_path_makes_the_turn_adapter_mandatory` still
passes with the lane's new pointer sentence as the reviewed edge.

## Lifecycle lane (`lane`)

Reader: an agent on a Codex host that delegates to the interactive Claude
CLI. Job after this task: the host preflight, the pointer to the adapter
before launch, the Herdr variant, and the lane's own policies (pane access,
effective autonomy, read-only consults, edit handoff, test-running review,
interactive prompts, cleanup, legacy mode) and its evidence section.

### Launch sequence

| Duty the lane stated | Home after TSK-163 | Pointer |
|---|---|---|
| Read the managed defaults, then a doctor-validated override | adapter, Launch and drive one turn (first comment) and the `model-bindings` paragraph | lane Lifecycle pointer |
| `codeflow delegate init` with run id, state dir, model and effort; prints the settings | adapter, Per-run setup step 2 and the launch block | lane Lifecycle pointer |
| Launch in tmux with `CLAUDE_CODE_DISABLE_BACKGROUND_TASKS=1`, model, effort, production `bypassPermissions` and `--settings` | adapter, launch block | lane Lifecycle pointer |
| Consult and no-edit use the same launch with `--permission-mode auto` | adapter, launch block comment | lane Lifecycle pointer |
| `wait --until ready` | adapter, launch block | lane Lifecycle pointer |
| `arm` the turn with the prompt file | adapter, launch block | lane Lifecycle pointer |
| Buffer paste, 300 ms settle, the fixed sentence only for a paste attachment, one Enter | adapter, launch block and delivery paragraph | lane Lifecycle pointer |
| `wait --until accepted`, then `wait --until terminal` | adapter, launch block | lane Lifecycle pointer |
| If acceptance times out and the pane shows the input still waiting, Enter once more and re-wait once; never blind or repeated Enter | adapter, delivery paragraph | lane Lifecycle pointer; lane Pane access keeps the one post-paste look |
| Herdr variant: named tab per `cf-herdr`, launch-local environment, `herdr pane send-text` | lane, kept; now says it replaces the adapter's tmux session | stays in the lane |

### Turn detection

| Duty the lane stated | Home after TSK-163 | Pointer |
|---|---|---|
| Turn detection is the lifecycle, not the pane | adapter, opening paragraph and Stable exit states ("never on pane appearance"); the lane pointer keeps "never use pane stability as completion" | lane Lifecycle pointer |
| `init` makes owner-only state outside every Git worktree | adapter, Per-run setup steps 1 and 2 | lane Lifecycle pointer |
| Hooks wired: `SessionStart`, `UserPromptSubmit`, `Stop`, `StopFailure` to `codeflow hook delegate-turn --state-dir` | adapter, Per-run setup step 2 | lane Lifecycle pointer |
| The settings file is immutable and bound to run id and state-dir spelling | adapter, Per-run setup step 3 | lane Lifecycle pointer |
| `arm` records the SHA-256 of canonical UTF-8, internal LF, no terminal line break, no other control characters; rejects empty or noncanonical input first | adapter, delivery paragraph | lane Lifecycle pointer |
| Normalize once, deliver that same file exactly | adapter, delivery paragraph | lane Lifecycle pointer |
| Acceptance and terminal records bind session and `prompt_id` | adapter, delivery paragraph and Stable exit states | lane Lifecycle pointer |
| Waits are bounded with stable exit states | adapter, Stable exit states | lane Lifecycle pointer |
| Restarts, mis-correlated events and interrupted waits poison the run; recover with a new run id and state dir | adapter, Stable exit states | lane Lifecycle pointer |
| One outstanding armed turn per run; arm a new id in the same session after a terminal result | adapter, Sequential turns | lane Lifecycle pointer |
| Follow-ups keep the session; arm the next turn and deliver to the same pane | adapter, Sequential turns ("arm the next turn under a new turn id in the same session") | lane Lifecycle pointer |
| Use the adapter for exact mechanics; never improvise a parser or scrape transcripts | lane, kept in the pointer sentence | stays in the lane |

### Sibling Stop-hook preflight

| Duty the lane stated | Home after TSK-163 | Pointer |
|---|---|---|
| Enumerate the effective Stop-hook set from every source the session loads | adapter, Sibling Stop-hook preflight | lane Lifecycle pointer |
| Reject a sibling Stop hook not known to be nonblocking | adapter, same section | lane Lifecycle pointer |
| The one known-safe sibling: the Codex plugin's `stop-review-gate-hook.mjs`, only with `stopReviewGate` confirmed off through the plugin's own surface | adapter, same section | lane Lifecycle pointer |
| CodeFlow never reads or infers plugin-private state | adapter, same section | lane Lifecycle pointer |
| An unknown or unverified sibling fails the preflight | adapter, same section | lane Lifecycle pointer |
| When it runs | adapter, same section: after `init` writes the task settings file and before launch, because a session loads its hooks when it starts; the launch block names the step between `init` and `tmux new-session` | lane Lifecycle pointer |

No duty disappears. Two drifts resolve toward the adapter, the lifecycle's
owner:

- The preflight runs before launch, not before delivery. A check at
  delivery comes after the session has loaded its hooks.
- The Enter retry: the lane allowed it when "the pane shows the prompt
  still waiting"; the adapter allows it when the pane "explicitly shows the
  paste attachment still waiting in the editor". The adapter's wording now
  governs alone.

## Turn adapter (`adapter`)

Reader: an agent on a Codex, Grok or other non-Claude host, before it
launches Claude through the delegated lifecycle. Two additions, no removals:

- The preflight section says why it comes before launch and that it runs
  after `init`, since the task settings file is one of the sources it
  enumerates.
- The launch block names the preflight as the step between `init` and the
  launch.

## Core (`SKILL.md`)

The Codex host row listed `codeflow delegate init`, exact-byte delivery,
turn detection and the sibling Stop-hook preflight as held by the lane. It
now names what the lane holds and says the lane sends the reader to the
turn adapter for those four before launch.

## Overlaps left in place

The lane's cleanup and legacy bullets and parts of its effective autonomy
bullet also appear in the adapter, in matching words. They are outside the
three rules TSK-138 mapped to this task and have not drifted, so they stay.

## Guard

`each_delegated_turn_rule_is_stated_once_in_the_adapter` in
`crates/codeflow-core/tests/delegate_doctrine_contract.rs` replaces the two
separate lane and adapter preflight pins. It finds each rule in the adapter
in reading order and fails, naming the rule, when the lane states it again,
when the adapter loses it, or when the adapter places the preflight after
launch. `a_restated_lost_or_late_turn_rule_fails_naming_it` holds its
negative controls: one restatement per rule added to the lane, a renamed
`arm` step in the adapter, and the preflight section moved after the launch
section.
