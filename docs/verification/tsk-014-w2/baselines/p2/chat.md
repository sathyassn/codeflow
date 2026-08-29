# P2 ordinary-chat baseline

What a competent assistant types when asked about the TSK-007 qualification
state. Authored before any candidate.

---

At the exact implementation candidate `82671551`, macOS arm64 passed all nine
`codeflow test --mode full --strict` targets, including the presentation target
at 158.336 s, the locked renderer build with digest `d9d37a03…`, and both the
real headless Chromium journey and the injected-failure cleanup journey. The
isolated release measurement reported a 16,518,448-byte binary with SHA-256
`6cc9d167…`.

Native Linux evidence exists but it was produced at source revision `74feaf04`,
which is older than the exact candidate, so it is retained as bounded diagnostic
evidence rather than promoted to exact-candidate approval. Within that guest,
Playwright's arm64 `headless_shell` ran, but the full desktop Chromium binary
exited with `SIGTRAP` and is not claimed as passed. It is also arm64 only, not
x86_64.

There is no native Windows or WSL2 run at all — the installed Parallels licence
had expired. The Windows confinement guard does run, but it proves start-time
confinement only, not account, profile, or VM teardown.

Two independent GPT-5.6 Sol/high reviews approved the exact candidate with no
material findings. The Claude judgment-primary verdict is still missing: the
session bound correctly but the accepted turn ended immediately with
`oauth_org_not_allowed`, so that is an authorization block rather than a test
failure.

Note also that the browser journey used `--no-launch` with a directly owned
Playwright browser, so it is not native product-launcher evidence.
