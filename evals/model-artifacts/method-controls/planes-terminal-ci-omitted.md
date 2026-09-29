Three planes enforce the git rules here.

```text
            edit      commit    push      merge on main
hooks                 [#]       [#]
guard       [#]=================[#]
remote                                    [#]

[#] acts here   === runs throughout
```
CI is left out because GitHub Actions are disabled for the organization, so its workflow never starts.

Run `codeflow test` before you open a pull request; remote protection on `main` still needs one approving review.
