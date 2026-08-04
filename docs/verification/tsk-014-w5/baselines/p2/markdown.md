# TSK-007 — qualification evidence at 82671551

Ordinary repository Markdown. Retained as the second plain baseline; it is not a design.

## Lanes

`macOS arm64`, `Linux arm64`, `native Windows`, `WSL2`, `independent review`.

## Per claim

1. **Nine strict test targets** — executed on macOS arm64; Linux arm64 evidence exists at
   an older revision and is not promoted; native Windows and WSL2 were not run.
2. **Isolated release-binary measurement** — executed on macOS arm64 only.
3. **Native Linux support** — Linux arm64 only, at revision `74feaf04`, explicitly not
   promoted to exact-candidate approval.
4. **Full desktop Chromium binary** — executed on macOS arm64; the Linux run exited
   `SIGTRAP` and is not claimed as passed.
5. **Windows profile and teardown safety** — a start-time confinement guard only.
6. **Native product-launcher behaviour** — no lane carries launcher evidence; the journey
   ran `--no-launch` with a directly owned browser.
7. **Cross-lineage review of the exact candidate** — two reviews returned approval with no
   material findings.
8. **Bound Claude judgment verdict** — the accepted turn ended immediately with
   `oauth_org_not_allowed`.

## Why lanes are missing

> No native Windows or WSL2 run was available because the installed Parallels license had
> expired.

> That guard proves start-time confinement, not account, profile, or VM teardown.

> is retained as bounded diagnostic evidence and is not promoted to exact-candidate
> platform approval
