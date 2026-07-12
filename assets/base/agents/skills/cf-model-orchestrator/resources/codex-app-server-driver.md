# Driving codex directly — the app-server protocol (advanced fallback) and tmux

> **Prefer the official `codex-plugin-cc`** (see the skill's "Driving codex"
> section). It wraps this same app-server, is maintained by OpenAI, and is the
> recommended path — it spares you keeping a hand-rolled driver in step with the
> experimental API. This resource is the **advanced fallback**: the raw protocol
> plus a reference driver, for fully-programmatic driving without slash commands,
> or environments where the plugin cannot be installed.

The concrete, code-based protocol for `cf-model-orchestrator` to drive codex
directly as the duo's reviewer/executor. **Within this direct path, app-server is
PRIMARY; tmux is the fallback.** Verified against **codex-cli 0.144.1** (the v2
`thread/*` + `turn/*` API, which is `[experimental]` — see "Version pinning").

## Why app-server (not `codex exec`, not tmux screen-scraping)

- It is the **same core engine as the TUI**, so it exposes the full tool set
  including the **Playwright MCP** for UI e2e. Headless `codex exec` cannot reach
  that tool set — which is precisely why exec is not the executor here.
- **Deterministic turn-completion** (`turn/completed` notification), a
  **deterministic MCP-readiness precheck** (`mcpServerStatus/list`), and
  **code-answered approvals** that never wedge the session.
- **Proper resumable sessions**: `thread/resume` loads a thread's full context
  from disk (`~/.codex/sessions/**/rollout-*.jsonl`), so it survives app-server
  restarts and is CLI-equivalent — a superset of `codex exec resume`. This is
  what makes the multi-round plan-align back-and-forth durable.
- **The codex desktop app need not be running.** `codex` is a standalone binary;
  auth is the local `auth.json` ChatGPT OAuth tokens, which codex refreshes
  itself. app-server is not an app-owned singleton.

## The sequence

1. **Launch** `codex app-server` over stdio. Framing is **JSONL — one JSON
   object per line** (the `"jsonrpc":"2.0"` header is not required on the wire).
2. **`initialize`** with `capabilities:{experimentalApi:true,
   requestAttestation:false}` — **`experimentalApi:true` is REQUIRED** to reach
   the v2 `thread/*`+`turn/*` methods — then send the `initialized` notification.
3. **MCP precheck** — `mcpServerStatus/list` `{detail:"full"}` → `result.data[]`,
   each `{name, authStatus, tools, …}`. Before any turn, assert each required
   server (e.g. `playwright`) is present **and exposes tools** (`tools`
   non-empty). A server that failed to start reports **0 tools** — on this
   machine `playwright` reads 24 tools and works, while `computer-use` reads 0
   and is not usable (and is not needed; Playwright covers web e2e). `authStatus:
   "unsupported"` is normal for stdio servers and is not a failure. Also honor
   `mcpServer/startupStatus/updated` (`starting|ready|failed|cancelled`) if you
   wait for a slow starter. **Never proceed toolless silently** — wait/retry,
   then report.
4. **`thread/start`** `{cwd, approvalPolicy:"never", sandbox:"workspace-write"}` →
   persist **`result.thread.id`** (the threadId).
   - To **continue** across rounds or after a restart: **`thread/resume`**
     `{threadId}` → loads full context from disk (optional `initialTurnsPage`).
     **Prefer resume by threadId** — the path/history resume modes are
     `[UNSTABLE]`.
5. **`turn/start`** `{threadId, input:[{type:"text", text, text_elements:[]}]}`
   once per round.
6. **Read notifications until `turn/completed`** `{threadId, turn:{status}}` —
   that **is** the idle signal (`status ∈ completed|interrupted|failed`).
   Accumulate `item/agentMessage/delta` `{delta, itemId, threadId}` for streamed
   text and `item/completed` for final items.

## Approvals never wedge

Keep `approvalPolicy:"never"` for exec/edits, but codex still emits residual
server→client **requests** you must answer in code, or the session wedges:

- `item/commandExecution/requestApproval`
- `item/fileChange/requestApproval`
- `item/permissions/requestApproval`
- `mcpServer/elicitation/request`
- `item/tool/requestUserInput`

Reply `{id, result:{decision:"approved"}}` (ReviewDecision ∈ `approved |
approved_for_session | denied | abort | timed_out | …`). Approving is correct
here because the **sandbox** (`workspace-write`, inside a feature-branch
worktree) is the real access control — not a human prompt.

## Robustness

- **Per-turn timeout** → `turn/interrupt` `{threadId}`; if still wedged, kill and
  respawn the subprocess.
- **Single writer per thread.** One orchestrator owns a threadId at a time.
  `thread/resume` rejoins a live thread — never drive turns on one threadId from
  two processes.
- **Keep the model stable across resumes.** Resuming with a different model than
  the rollout recorded injects a one-time model-switch instruction on the next
  turn. Since we pin Fable / `gpt-5.6-sol`, do not flip model mid-thread.
- **Keep `ephemeral` OFF** (the default) so threads persist to disk;
  `--ephemeral` / `ephemeral:true` = non-resumable.
- Watch for the `error` notification and a non-zero subprocess exit.

## Version pinning (harness-parity canary)

The v2 `thread/*`+`turn/*` API is `[experimental]`, gated behind the `initialize`
capability `experimentalApi:true` and emitted only under the `--experimental`
contract generators. **Pin the codex version** and, on every upgrade, regenerate
and diff the contract:

```sh
codex app-server generate-ts          --out DIR
codex app-server generate-json-schema --out DIR --experimental
```

Tie this to codeflow's harness-parity canary / release-checklist re-verify step
so the driver cannot silently rot against a new codex. (Contract above verified
against codex-cli 0.144.1.)

## Reference driver (Python 3, standard library only)

**REFERENCE IMPLEMENTATION — read it, adapt it, and live-test it before any
production use.** It has not been run end-to-end here. Field names and methods
are taken from the 0.144.1 generated contract; regenerate and diff on every codex
upgrade (see "Version pinning").

```python
#!/usr/bin/env python3
"""Reference driver for `codex app-server` (codex-cli 0.144.1, v2 thread/turn API).

Reference only — not run end-to-end here. Live-test before production use.
stdlib only: subprocess + json + threads.
"""
import itertools
import json
import queue
import subprocess
import threading
import time

# Residual server->client approval requests to auto-answer so a turn never wedges.
APPROVAL_METHODS = {
    "item/commandExecution/requestApproval",
    "item/fileChange/requestApproval",
    "item/permissions/requestApproval",
    "mcpServer/elicitation/request",
    "item/tool/requestUserInput",
}


class AppServerError(RuntimeError):
    pass


class CodexAppServer:
    def __init__(self, cwd, codex="codex"):
        self.cwd = cwd
        self._ids = itertools.count(1)
        self._proc = subprocess.Popen(
            [codex, "app-server"],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL, text=True, bufsize=1,
        )
        self._resp = {}                 # request id -> response message
        self._resp_cv = threading.Condition()
        self._notes = queue.Queue()     # server->client notifications
        self._alive = True
        self._reader = threading.Thread(target=self._read_loop, daemon=True)
        self._reader.start()

    # --- framing: one JSON object per line ---
    def _send(self, msg):
        self._proc.stdin.write(json.dumps(msg) + "\n")
        self._proc.stdin.flush()

    def _read_loop(self):
        for line in self._proc.stdout:
            line = line.strip()
            if not line:
                continue
            try:
                msg = json.loads(line)
            except json.JSONDecodeError:
                continue
            if "id" in msg and ("result" in msg or "error" in msg):
                with self._resp_cv:                 # response to our request
                    self._resp[msg["id"]] = msg
                    self._resp_cv.notify_all()
            elif "id" in msg and "method" in msg:
                self._answer_request(msg)           # server->client request
            elif "method" in msg:
                self._notes.put(msg)                # notification
        self._alive = False
        with self._resp_cv:
            self._resp_cv.notify_all()

    def _answer_request(self, msg):
        if msg.get("method") in APPROVAL_METHODS:
            self._send({"id": msg["id"], "result": {"decision": "approved"}})
        else:                                       # reject unknown, never wedge
            self._send({"id": msg["id"],
                        "error": {"code": -32601,
                                  "message": "unhandled server request: %s" % msg.get("method")}})

    def request(self, method, params, timeout=600):
        rid = next(self._ids)
        self._send({"id": rid, "method": method, "params": params})
        deadline = time.monotonic() + timeout
        with self._resp_cv:
            while rid not in self._resp:
                if not self._alive:
                    raise AppServerError("app-server exited")
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise AppServerError("timeout waiting for %s" % method)
                self._resp_cv.wait(remaining)
            msg = self._resp.pop(rid)
        if "error" in msg:
            raise AppServerError("%s: %s" % (method, msg["error"]))
        return msg.get("result", {})

    def notify(self, method, params=None):
        self._send({"method": method, "params": params or {}})

    # --- handshake ---
    def initialize(self):
        res = self.request("initialize", {
            "clientInfo": {"name": "cf-driver", "title": None, "version": "1"},
            "capabilities": {"experimentalApi": True, "requestAttestation": False},
        })
        self.notify("initialized")
        return res

    # --- deterministic MCP precheck ---
    def require_mcp(self, required):
        res = self.request("mcpServerStatus/list", {"detail": "full"})
        by_name = {s.get("name"): s for s in res.get("data", [])}
        for name in required:
            s = by_name.get(name)
            tools = (s or {}).get("tools") or []
            if not s or not tools:
                raise AppServerError(
                    "MCP server %r not ready (present=%s, tools=%d)"
                    % (name, bool(s), len(tools)))
        return by_name

    # --- threads ---
    def start_thread(self, approval_policy="never", sandbox="workspace-write"):
        res = self.request("thread/start", {
            "cwd": self.cwd,
            "approvalPolicy": approval_policy,
            "sandbox": sandbox,
        })
        return res["thread"]["id"]

    def resume_thread(self, thread_id):
        res = self.request("thread/resume", {"threadId": thread_id})
        return res.get("thread", {}).get("id", thread_id)

    # --- one turn; blocks on the turn/completed NOTIFICATION (not the response) ---
    def turn(self, thread_id, text, timeout=1800):
        while not self._notes.empty():              # drop stale notifications
            try:
                self._notes.get_nowait()
            except queue.Empty:
                break
        rid = next(self._ids)
        self._send({"id": rid, "method": "turn/start", "params": {
            "threadId": thread_id,
            "input": [{"type": "text", "text": text, "text_elements": []}],
        }})
        chunks = []
        deadline = time.monotonic() + timeout
        while True:
            with self._resp_cv:                     # surface an early turn/start error
                early = self._resp.pop(rid, None)
            if early is not None and "error" in early:
                raise AppServerError("turn/start: %s" % early["error"])
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                self.request("turn/interrupt", {"threadId": thread_id}, timeout=30)
                raise AppServerError("turn timed out; interrupted")
            try:
                note = self._notes.get(timeout=min(remaining, 30))
            except queue.Empty:
                if not self._alive:
                    raise AppServerError("app-server exited mid-turn")
                continue
            m, p = note.get("method"), note.get("params", {})
            if p.get("threadId") not in (None, thread_id):
                continue
            if m == "item/agentMessage/delta":
                chunks.append(p.get("delta", ""))
            elif m == "turn/completed":
                status = (p.get("turn") or {}).get("status")
                if status != "completed":
                    raise AppServerError("turn ended status=%s" % status)
                return "".join(chunks)
            elif m == "error":
                raise AppServerError("server error: %s" % p)

    def close(self):
        try:
            self._proc.stdin.close()
        except Exception:
            pass
        try:
            self._proc.wait(timeout=10)
        except Exception:
            self._proc.kill()


if __name__ == "__main__":
    drv = CodexAppServer(cwd="/abs/project")        # an already-trusted worktree
    try:
        drv.initialize()
        drv.require_mcp(["playwright"])             # abort if UI e2e tools absent
        tid = drv.start_thread()
        print("threadId:", tid)
        print(drv.turn(tid,
                       "Review the plan in PLAN.md against its acceptance criteria. "
                       "End with VERDICT: approved|changes_requested."))
        # Later round / after a restart:
        #   drv.resume_thread(tid)
        #   drv.turn(tid, "Address finding #1 and re-run the e2e suite.")
    finally:
        drv.close()
```

## Fallback: tmux-driven TUI

Use only where app-server is genuinely unavailable. **Strictly less robust**: no
structured approvals (a stray prompt the policy does not cover wedges the pane),
and MCP status must be prechecked out-of-band with `codex mcp list`.

- **Start detached:** `tmux new-session -d -s codex -x 220 -y 50`.
- **Launch codex** with `-a never -s workspace-write` and a per-session notify
  hook: `-c 'notify=["/abs/notify.sh"]'`. Pre-seed folder trust
  (`[projects."/abs/proj"]` + `trust_level = "trusted"`) and hook trust (approve
  via `/hooks`, or `--dangerously-bypass-hook-trust` for a vetted committed
  config).
- **Turn detection (native signal, not scraping):** `notify.sh` writes `$1` (the
  `agent-turn-complete` JSON, which carries `last-assistant-message`) to a file
  and calls `tmux wait-for -S codex-turn`; the driver blocks on
  `tmux wait-for codex-turn`. Note `notify` fires only on turn-complete, **not**
  on an approval prompt — which is why `-a never` is mandatory here.
- **Send input:** literal via `send-keys -l "…"` then a **separate**
  `send-keys Enter`; multiline via `set-buffer -b cx -- "…"` +
  `paste-buffer -p -b cx -t codex` (the `-p` requests bracketed paste) +
  `send-keys Enter`.
- **Read output:** `capture-pane -p -J -t codex -S -200` — **never `-e`** (the
  default capture is already ANSI-free; `-J` de-wraps).
- **Crash / timeout:** `set-option -t codex remain-on-exit on`, then read
  `#{pane_dead}` via `list-panes -F`; wrap `wait-for` in an OS `timeout`; on
  expiry send `Escape` (interrupt) or `C-c`, `capture-pane` for diagnosis, then
  `kill-session`.
- **Gotcha:** tmux's control socket lives under `/tmp`, which the codeflow
  command sandbox denies — the tmux path needs the sandbox bypass for its socket.
