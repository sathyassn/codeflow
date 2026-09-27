Four planes can enforce the git rules here. Three of them are active; CI is installed but never runs.

```text
            edit      commit    push      pull request  merge on main
hooks                 [#]       [#]
guard       [#]=================[#]
CI                                        ( )
remote                                                  [#]

[#] active here   ( ) present but not running   === runs throughout
```
Each plane sits at the moments it acts, and CI is drawn open because GitHub Actions are disabled for the organization.

The hooks come with `codeflow init`, the guard is wired in `.claude/settings.json`, and remote protection on `main` asks for one approving review. Run `codeflow test` yourself before opening a pull request, since no CI result will arrive.
