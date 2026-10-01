Here is what happens to a change on its way in.

```text
edit --> commit --> push --> pull request --> merge
          hooks     hooks       CI           review
```
A change passes each check in order.

Hooks, the session guard, CI and remote protection all enforce the rules.
