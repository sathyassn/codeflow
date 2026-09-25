# Reviewing with present

<!-- Guide layer. Reader: someone an agent has asked to review a present
     document, and the operator who starts one. Sources: docs/architecture/present.md,
     docs/capabilities.md (interactive-presentation-review), docs/cli.md,
     cf-present skill, crates/codeflow-cli/src/cmd/present.rs,
     crates/codeflow-present/web/src/chrome.tsx. -->

## Concept

A present session is a short-lived local review document that an agent writes
for you to read and comment on.

The agent writes the content, and CodeFlow draws the page, its themes and the
Comment tools. It is for the person an agent has asked to review a plan, a diff
or a decision, and for the operator who starts the session. It is not durable
documentation, a product UI or a standing server. A session stays private to
your user account on your machine and closes itself after four idle hours.

## Architecture

Four parts take part in a review, and the record stays with CodeFlow on your
machine.

- **The agent session** writes one JSON document and opens it. Later it reads
  your submitted review and marks it addressed or dismissed.
- **The local loopback service** is one CodeFlow process per session, listening
  only on `127.0.0.1`. It validates the document, stores every revision and
  accepts your review. Host, Origin and session cookie checks protect it.
- **Your browser** is an isolated browser profile that CodeFlow launches for
  this one session. It never uses your everyday browser or its logins, and if
  it cannot launch one it stops instead of falling back to yours. Nothing the
  browser caches becomes part of the record.
- **The feedback ledger** is an append-only history kept outside the
  repository in owner-only state. Each submitted review moves from received to
  delivered, then to addressed or dismissed.
- Sessions belong to a Git repository, so linked worktrees of one repository
  share them.
- Nothing from a session is committed. An outcome worth keeping moves to its
  real task, spec, decision record or project doc.

The [present runtime architecture](architecture/present.md) covers the service,
state and platform rules in depth, and the
[command reference](cli.md) lists every flag.

## Technical

Every present command runs inside a Git working tree and acts on that
repository's sessions.

| Command | What it does |
|---|---|
| `codeflow present open <DOCUMENT>` | Validates the JSON document, starts the session's service, opens the isolated browser window and prints `opened <session-id>`. `--no-launch` starts the service and prints the bootstrap file to open instead |
| `codeflow present list` | Prints this repository's sessions as JSON |
| `codeflow present show <SESSION_ID>` | Reopens an active session, restarting its service if it stopped. For a closed session it prints the session record. `--no-launch` prints where to open it |
| `codeflow present update <SESSION_ID> <DOCUMENT>` | Adds a validated, immutable revision to an active session |
| `codeflow present feedback <SESSION_ID>` | Delivers pending reviews as JSON lines to whoever runs it, usually the agent. `--follow` keeps delivering until the session closes |
| `codeflow present resolve <SESSION_ID> <EVENT_ID>` | Marks one delivered review as handled. Needs `--event-version <N>` and `--status addressed` or `--status dismissed` |
| `codeflow present history <SESSION_ID>` | Prints the session's full feedback history as JSON |
| `codeflow present export <SESSION_ID> --out <FILE>` | Writes a self-contained, read-only HTML copy of the document. `--theme` takes `slate`, `graphite` or `sage`, or the aliases `editorial` (the default, shown as slate), `instrument`, `technical` and `ink`. `--mode` is `system` (default), `light` or `dark` |
| `codeflow present close <SESSION_ID>` | Ends the session and closes its browser window. Repeating it is safe |
| `codeflow present clear [SESSION_ID]` | Removes closed sessions older than `--older-than` (default `30d`). `--dry-run` lists what it would remove |

### Review a document

You review in the window CodeFlow opens, and your notes reach the agent only
when you submit them.

1. Wait for the agent to run `codeflow present open <document.json>`, or run it
   yourself. If you closed the window, reopen it with
   `codeflow present show <session-id>`.
2. Read the page from the top. On a wide screen, the left rail lists the
   sections.
3. Press `C` or click **Comment** to turn on Comment mode. The notes rail
   opens beside the document.
4. Pick what the note is about. Select words for a text note, click a figure
   part or another element for an element note, or drag a box for an area note.
   **Tools** in the rail offers **Add selected text**, **Pick element**,
   **Select area** and **Whole document** for the same choices.
5. Click the small floating marker that appears, write the note and click
   **Save note**, or press Cmd+Enter or Ctrl+Enter. Repeat for each point.
6. Under **Verdict**, choose Approve, Approve with notes or Request changes.
   Request changes also needs the required change written in the box below.
7. Click **Submit review**. It stays disabled until you have saved at least one
   note.
8. The agent reads your review with `codeflow present feedback <session-id>`
   and, after acting on it, runs
   `codeflow present resolve <session-id> <event-id> --event-version <n> --status addressed`.

The rail reads "Review received" with an event id when you submit and
"Review state changed." as the agent handles it, and after a reload the
**Earlier feedback** list shows that review as addressed or dismissed.

### Export or close a session

Export when someone needs a read-only copy, and close the session when the
review is over.

1. Find the session id with `codeflow present list`.
2. Export a copy with
   `codeflow present export <session-id> --out review.html`. Add
   `--mode dark` or `--theme graphite` if you want a different look. The copy
   holds the document only, without notes or review controls.
3. Close the session with `codeflow present close <session-id>`.
4. To reclaim space later, preview with `codeflow present clear --dry-run`,
   then run `codeflow present clear`.

Export prints `exported <file>`, close prints `closed <session-id>` and the
review window goes away, and clear prints `would remove` or `removed` for each
session it selects.
