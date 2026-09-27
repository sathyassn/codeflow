# Merge policy

## Concept

The merge policy decides whether a pull request lands in a service repository, and how. It is for
engineers who open and review pull requests. A pull request meets the branch rules first, then the
required checks, then the merge method. The policy is not the CI setup: it names the checks that must
pass, and CI runs them.

## Architecture

The merge bot reads one file, `.merge-policy.toml`, and applies its three parts in order.

- Branch rules name the protected branches: `main` and `release/*`.
- Required checks name what must pass: build, test and lint.
- Merge methods say how a pull request lands: squash on `main`, a merge commit on `release/*`.

A pull request that fails a branch rule never reaches the checks, and one that fails a check never
reaches the merge method.

## Technical

Every service repository adopts the policy with its first production deploy, by committing
`.merge-policy.toml` at the repository root. Prototypes under `labs/` are exempt until they deploy.

| Command | Effect |
|---|---|
| `mp show` | prints the effective policy for the current repository |
| `mp check <pr>` | evaluates a pull request against all three parts |
| `mp explain <rule>` | prints why a rule applies |
