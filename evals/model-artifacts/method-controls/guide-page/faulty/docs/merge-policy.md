# Merge policy

## Concept

Every service repository adopts this policy when it gets its first production
deploy. Prototypes under `labs/` are exempt until they deploy. Adoption is one file,
`.merge-policy.toml`, committed at the repository root.

```cf-stage
Branch rules|main and release/*
->
Required checks|build, test and lint
->
Merge methods|squash or merge commit
caption: The three parts of the policy.
```

## Architecture

The policy has three parts, each read by the merge bot in order:

- Branch rules name the protected branches: `main` and `release/*`.
- Required checks name what must pass: build, test and lint.
- Merge methods say how a pull request lands: squash on `main`, a merge commit on `release/*`.

A pull request that fails a branch rule never reaches the checks, and one that fails a check never
reaches the merge method.

## Technical

| Command | Effect |
|---|---|
| `mp show` | prints the effective policy for the current repository |
| `mp check <pr>` | evaluates a pull request against all three parts |
| `mp explain <rule>` | prints why a rule applies |
