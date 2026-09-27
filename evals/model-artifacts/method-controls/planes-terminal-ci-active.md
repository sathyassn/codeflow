Four planes enforce the git rules here, and all four are active.

```text
            edit      commit    push      pull request  merge on main
hooks                 [#]       [#]
guard       [#]=================[#]
CI                                        [#]
remote                                                  [#]

[#] active here   === runs throughout
```
Each plane sits at the moments it acts.

CI checks every pull request before remote protection lets it merge.
