# agy: retired delegate tier and experimental guard binding

Loaded on demand from `cf-delegate` when the user names `agy` or runs CodeFlow
guards under it.

## Why agy is not a delegate tier

Google's Antigravity `agy` was a degraded, opt-in read-only tier. Its only
documented drive shape is headless one-shot CLI invocation (non-TTY stdout
drops the final response, so automation had to read a transcript file), a
shape CodeFlow ADR-0023 prohibits outright. There is no verified interactive lane to
it, so `agy` is **not** a delegate tier; if the user names it, say the
transport rule rules it out. Like any tool that touches the repo, `agy`
remains bound by the harness-agnostic git-hook plane and CI.

## Experimental guard binding

**Experimental: binding the CodeFlow guards to `agy`** (for when `agy` is
someone's *harness*, not a delegate): its hook dialect differs (an
`allow_tool` JSON contract, hooks that always exit 0), so the exit-2 block
that stops a bad command under Claude/Codex is at best *advisory* there. To
opt in for feedback anyway, create `~/.gemini/config/hooks.json` with
`{"hooks":{"PreToolUse":[{"matcher":"^Bash$","hooks":[{"type":"command","command":"codeflow hook git-guard"},{"type":"command","command":"codeflow hook exec-guard"}]}]}}`.
This is experimental and unverified on macOS; the authoritative protection stays the
git-hook plane, CI, and an independent review before anything lands.
