### Fixed

<!-- codeflow:release-impact patch -->
- **A human's override covers protected commits and pushes.** The README
  says a human can override the git-hook plane with
  `CODEFLOW_HUMAN_OVERRIDE=1`, but the pre-commit and pre-push hooks
  ignored it, so the first push of `main` to an empty remote after
  `codeflow init` needed `--no-verify`. Both hooks now honour the override
  for the protected-branch rules, as the merge hooks already did:
  `CODEFLOW_HUMAN_OVERRIDE=1 git push -u origin main` works. A force push
  or deletion of a protected branch and the secret checks stay refused, and
  the git-guard still refuses an agent that sets the override. A push to a
  protected branch whose remote tip this clone has not fetched cannot be
  proven a fast-forward, so it is refused as a force push until you fetch.
  The git-guard also sees git behind launcher options (`command -p`,
  `exec -a NAME`, `nohup`, `/usr/bin/time -o FILE`), including an override
  hidden in a git alias declared that way. It skips a launcher option it
  does not know rather than trust it, so a command that runs nothing, such
  as `nohup --help git push`, may be refused.
