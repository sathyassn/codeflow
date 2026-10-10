### Fixed

<!-- codeflow:release-impact patch -->
- **The full gate runs inside CodeFlow's own Claude sandbox.** The full
  gate takes a machine-wide lock under `~/.codeflow/locks` and keeps its
  evidence under `~/.codeflow/gate-runs`, which the shipped Claude settings
  presets did not let a sandboxed command write, so every full gate in a
  sandboxed session refused with `gate lock unavailable`. The presets now
  allow writes to those two directories and nothing else in the CodeFlow
  home, and `codeflow update` adds them to existing settings. Two full
  gates still never run at once on one machine. The refusal now points at
  `codeflow doctor --check permissions`, which names each gate directory
  this process cannot write. When `SANDBOX_RUNTIME` is set, doctor notes
  it, probes the network over HTTPS instead of a DNS lookup the sandbox
  cannot make and passes only on an HTTP 2xx or 3xx answer, and reports a
  failed Codex sign-in probe as unconfirmed, quoting its error, unless the
  probe says you are signed out.
