### Fixed

<!-- codeflow:release-impact patch -->
- **Model evaluation trials launch Codex and Grok 1.0.46 again.** A
  signed-in Codex 0.159.1 synced the account's installed remote plugins
  into the dedicated evaluator home at every start, and one of them holds a
  folder named `hooks`, so every Codex launch with the hook-trust bypass
  refused, Codex peers in Claude trials included. `cf-evaluate-model`'s
  `eval_kit.py prepare-eval-homes` now writes `plugins = false` and
  `remote_plugin = false` under `[features]` in the dedicated Codex
  config, adding only what is missing, and moves a stale remote plugin
  cache out of that home into `~/.codeflow-eval/removed-remote-plugins/`,
  printing each move and why; every Codex start the kit builds passes the
  same two settings. Run it again once after updating. The repository's
  qualification runner refuses any launch whose dedicated Codex home lacks
  the settings or holds that cache, naming `prepare-eval-homes`, and checks
  again after readiness and for each Codex peer. The plugin check still
  refuses any folder named `hooks`. The runner also recognises Grok Build
  1.0.46's trust dialog beside 1.0.44, records the version, and refuses any
  other version or screen with a message naming both, keeping "evaluator
  home not signed in" for a visible sign-in screen.
