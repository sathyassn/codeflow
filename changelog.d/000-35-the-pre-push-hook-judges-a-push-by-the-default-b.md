### Fixed

<!-- codeflow:release-impact patch -->
- **The pre-push hook judges a push by the default branch's policy.**
  `codeflow ci` and the pre-push hook judged commit, branch and PR-body
  standards with the branch's own `.codeflow/policy.json`, while the hosted
  policy job reads the policy of the target tip it checks out. A branch
  that loosened its own rules, such as more commit-body bullets, passed
  locally and failed after the push. The pre-push hook now judges every
  pushed branch with the policy at the destination default branch's
  advertised tip, fetched at most once per push when this clone lacks it.
  That policy is a candidate destination authority, not the known pull
  request target, and hosted CI stays the enforcement: no other branch is
  an authority, even a protected one, so a pull request into an
  integration line is judged locally by the default branch's policy and
  the host may judge it differently. The policy is strictly validated,
  sets the rules, and decides whether and at what level `codeflow ci`
  gates the push (`git.test_gate_on_push`), so neither the head nor the
  working copy can lower or turn off that check. For a head that shares
  history with that tip, the commit checks run over everything the head
  adds to the tip, as the hosted job's range from the target tip does, so
  a commit the destination already holds under a tag or another branch,
  or one an earlier push carried before the policy tightened, is still
  checked. A head whose recorded history shares nothing with the tip,
  such as a `gh-pages` deployment branch, is never diffed against it: a
  fast-forward of a branch the destination already has keeps its own new
  commits as its commit range, and a new branch keeps the commits the
  destination does not hold yet, under that policy, and the hook says
  this is not default-target parity. A shallow clone, a graft or a
  replace ref cannot show the histories are unrelated, so it keeps the
  tip. A target the task record
  declares bounds only the other checks: nothing local proves the pull
  request goes there, so a branch built on an integration line has the
  line's inherited commits judged by the default branch's current policy
  as well. That is stricter than the hosted job for a pull request into
  the line, for inherited commits only; those commits must pass that
  policy when the line's pull request reaches the default branch anyway.
  `codeflow ci` names both ranges when its commit checks run from another
  commit than its base, and then does not call the run the hosted verdict.
  A policy there that this codeflow cannot read or validate refuses the
  push, whether or not its range resolves; when a newer codeflow wrote it, upgrade the
  local one. With no candidate authority (a destination that does not
  answer, a failed fetch, or a default branch with no policy yet), the
  hook says so and, where a range resolves, still runs `codeflow ci` at
  block level with the policy at the range's base, as a best-effort
  check. When an `upstream` remote points elsewhere than the
  push, the hook notes that a pull request may target the upstream, whose
  policy can differ. The tree checks in the pre-push hook
  (`codeflow validate --docs` and the quick targets) and the release
  preflight still follow the working copy's gate, and the commit-msg and
  other local hook stages still read the working copy, so a commit relying
  on a loosened rule is made and then refused at push, before it leaves
  the clone. Run directly, `codeflow ci` judges with the policy at the base
  it is given, or at the commit `--policy-from` names, as do its
  automation profiles; its banner says the result is the hosted verdict
  only when that commit is the pull request's target tip, which the hosted
  jobs pass. A base with no policy yet, as in the change that adopts
  CodeFlow, still uses the working copy's. A branch that changes the
  policy lands that change before commits that rely on it.
  When a local target branch is behind its upstream, `codeflow work start`
  and `codeflow ci` no longer print a note asking you to fast-forward it:
  they already anchor on the upstream, `work start` names it, and the
  guards refuse the fast-forward steps the note printed. When git itself
  refuses the base, for example a `GIT_REPLACE_REF_BASE` without a trailing
  slash on git 2.55 or later, `codeflow ci` now prints git's message in
  place of the advice to fetch.
