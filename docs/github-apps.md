# Set up the GitHub Apps

This guide is for a repository administrator who wants agent sessions to stop
acting under their own GitHub login. One command creates two GitHub Apps, and
each App takes two browser clicks.

| App | Permissions | What it is for |
|---|---|---|
| Authority App | Checks write | Creates the `authority:` check runs from the default branch |
| Agent App | Contents and Pull requests write | Gives agent sessions their own identity on GitHub |

GitHub adds Metadata read to every App. The agent App has no Administration,
Workflows, Environments, Deployments, Commit statuses or Checks permission.

## The agent App is optional

Nothing requires the agent App, for a project or for a developer. Each computer
sits at one of three levels, and developers on one repository can differ.

| Level | What it needs | Commits record | Pushes and pull requests run as | What it gives up |
|---|---|---|---|---|
| No App | Nothing | You as author and committer | Your login | Agent work looks like yours, and the session has everything your login has |
| Committer values only | The two values in the harness settings; no key on this computer | You as author, `<slug>[bot]` as committer | Your login | The session still has everything your login has; the committer is a record only |
| Key on the computer | The values, plus this computer's key for the App | You as author, `<slug>[bot]` as committer | The App's limited token | One key to hold and rotate per computer, and a credential store |

The committer field needs no key and no signature. It is text in the commit, so
the second level works on any computer that has the settings.

## Before you start

- You are an administrator of the repository, signed in with your own
  `gh auth login`. The command refuses a GitHub App token, and an account that
  is not the repository's administrator, before it writes anything.
- You run it in your own terminal, in a clone of the repository, never in an
  agent session.
- The agent App's key needs a credential store: the login keychain on macOS,
  or the Secret Service through `secret-tool` on Linux. Windows has no
  supported store yet, so use `--no-agent` there to set up the authority App
  alone.
- GitHub limits an App name to 34 characters, and names are unique across
  GitHub. The default names are `codeflow-authority-<repo>` and
  `codeflow-agent-<repo>`. When one is too long, choose another with
  `--authority-name` or `--agent-name`.

## Create the Apps

```sh
codeflow remote setup-apps
```

For each App:

1. Open the address the command prints. The page sends the App's manifest to
   GitHub.
2. Check the name and the permissions, then click "Create GitHub App".
3. Open the printed `https://github.com/apps/<slug>/installations/new`, choose
   "Only select repositories", pick this repository and click "Install".

When the browser cannot return to `http://127.0.0.1`, add `--paste`. The
command writes the page to a file, and you paste back the address GitHub opens.

The command also creates the environment `codeflow-authority`, restricted to
the default branch, with the secret `CODEFLOW_AUTHORITY_APP_KEY` and the
variable `CODEFLOW_AUTHORITY_APP_ID`. The agent App's key goes to the
credential store. Every write is printed with what undoes it.

## Run it again

The command is safe to run twice. It creates nothing for an App it finds, and
it reports each App differently, because it can check them differently.

**The agent App** is checked with its own key. The command signs a token with
the stored key and asks GitHub `GET /app`, so the answer comes from the App
itself. When the id, slug, owner, permissions and events match, it prints:

```text
agent App: already set up (...), nothing created
```

When they do not match, or GitHub refuses the key, the run stops with a
non-zero exit and names what differs.

**The authority App** is usually not checked this way. Its key is stored only
as a GitHub secret, and no command can read it back. An App created with "Only
on this account" is private, so `gh api apps/<slug>` answers 404 under a normal
login. GitHub's API answers 404 for a deleted App too, so the command cannot
tell the two apart and does not guess. It reports what it did check and goes
on to the agent App:

```text
authority App: present but not verified, nothing created
  checked: the variable CODEFLOW_AUTHORITY_APP_ID (<id>) and the secret CODEFLOW_AUTHORITY_APP_KEY (its name only; the value cannot be read)
  not checked: the App <slug> itself. GitHub's API answered 404, and it answers 404 both for a private App this login cannot read and for a deleted App, so this run cannot tell them apart
  check it yourself: https://github.com/settings/apps/<slug> should show App ID <id>, and https://github.com/settings/apps/<slug>/permissions should show only checks write and metadata read, with no events
  to set it up again, delete that App, then `gh api -X DELETE <variable path>` and `gh secret delete CODEFLOW_AUTHORITY_APP_KEY --env codeflow-authority --repo <owner>/<repo>`, and run again
```

This is a prompt to look, and the run is not a failure. Open the two pages and
compare them. In an organization the pages are under
`https://github.com/organizations/<org>/settings/apps/<slug>`.

When your login can read the App, for example because the App is public, the
command checks it through the API and prints `authority App: already set up`.

The run stops, whatever the App's visibility, when:

- the secret is missing, or the variable does not hold an App id;
- GitHub shows an App with another id, other permissions or any events;
- GitHub answers anything other than success or 404, such as a rate limit or a
  server error. Run it again later;
- this computer's credential store cannot say whether it holds the agent App's
  key, such as when the Secret Service is not reachable on Linux. The run
  creates no App and stores nothing.

## An authority App you made by hand

There is no flag that skips the authority App. Give the command the slug of the
App you already have, so that it looks for that App and creates only the agent
App. Without `--authority-name` it looks for the default name, and its message
would then point at a page for an App that does not exist.

First confirm that the id variable is in the environment, because the command
creates a new authority App when the variable is missing:

```sh
gh api repos/<owner>/<repo>/environments/codeflow-authority/variables/CODEFLOW_AUTHORITY_APP_ID --jq .value
```

Then run the command with your App's slug. For CodeFlow's own repository, whose
authority App was made by hand, the command is:

```sh
codeflow remote setup-apps --authority-name codeflow-authority-app --agent-name codeflow-agent-app
```

It writes nothing to an environment that is already restricted to the default
branch.

## Use the agent App in a session

`codeflow remote agent-token` prints an installation token for this repository,
valid for one hour. It reads the key from the credential store and never prints
it.

- `gh`:

  ```sh
  TOKEN="$(codeflow remote agent-token)" && GH_TOKEN="$TOKEN" gh pr create ...
  ```

  Keep the `&&`. Without it, a failed token leaves `GH_TOKEN` empty, and `gh`
  may fall back to your own login, so the action would run as you.
- `git push`:

  ```sh
  git -c credential.helper= -c 'credential.helper=!codeflow remote agent-token --git-credential' push
  ```

The key's custody rests on the session's sandbox. A sandboxed session must not
reach the credential store, and only `codeflow remote agent-token` runs outside
the sandbox. Check this on your host: from a sandboxed session, run the
following. It prints only a byte count, and that count must be 0.

```sh
security find-generic-password -s codeflow-agent-app -a <owner>/<repo> -w | wc -c
```

When the count is not 0, stop: the session can read the key on this host.

## Author and committer

The human stays the author of every commit. The agent App is the committer.

The agent App as committer is an identity that names the App, not a model vendor, and is not AI attribution under git.ai_attribution. Operator decision 2026-11-06.

Nothing about the commit rules relaxes. Agent commits still pass every commit
standard: conventional commits, the commit-msg hook, and `git.ai_attribution`
at block, so no AI co-author, no "Generated with" line and no emoji. The bot
committer changes only the committer identity field and is never a route around
a message check. No policy, hook or check is loosened or special-cased for
bot-committed commits.

The committer field is a provenance record, not proof: git lets anyone set it.
Proof of who pushed is the token. CodeFlow does not enforce the author or
committer fields.

`codeflow remote agent-token --committer name` and `--committer email` are the
source of truth for the two values. They look like `<slug>[bot]` and
`<bot user id>+<slug>[bot]@users.noreply.github.com`. The id is the App's bot
user id, which is a different number from the App id. The command prints nothing
on stdout when it cannot confirm the identity: GitHub does not answer as the App,
the bot user does not exist, or its login, type or id is not what it should be.

Your harness's project settings carry the values to each session. They name the
App and are not secrets, so check them in, and a new developer gets them on
clone. `codeflow remote setup-apps` prints the exact lines for each harness and
edits no settings file. Add them yourself:

Claude Code, inside the `"env"` object of `.claude/settings.json`:

```json
"GIT_COMMITTER_NAME": "<slug>[bot]",
"GIT_COMMITTER_EMAIL": "<bot user id>+<slug>[bot]@users.noreply.github.com"
```

Codex, at the end of `.codex/config.toml`:

```toml
[shell_environment_policy.set]
GIT_COMMITTER_NAME = "<slug>[bot]"
GIT_COMMITTER_EMAIL = "<bot user id>+<slug>[bot]@users.noreply.github.com"
```

Codex reads a project's `.codex/config.toml` only when that project is trusted,
and trust is set per path, so each clone and worktree needs it. Check every new
checkout with `printenv GIT_COMMITTER_NAME` in a session.

Limit: Codex documents `set`, but CodeFlow has not confirmed that Codex reads it
from a project's file. That `printenv` check is the proof. If it prints nothing,
set the variables in the shell that launches Codex, as for Grok Build below.

Grok Build has no project environment setting that CodeFlow knows of. Set the two
variables in the shell that launches it, with the quotes, because `[bot]` is a
pattern to the shell:

```sh
export GIT_COMMITTER_NAME='<slug>[bot]'
export GIT_COMMITTER_EMAIL='<bot user id>+<slug>[bot]@users.noreply.github.com'
```

Both settings files are enforcement files. A human makes this edit, because
agent sessions cannot, and approves it with `codeflow baseline approve` before
the commit. `codeflow update` keeps your two keys in `.claude/settings.json`, and
merges your lines in `.codex/config.toml`. If that merge conflicts, it writes
`.codex/config.toml.new` and reports it.

- Set both variables. With only one set, the commit mixes two identities.
- Never put these values in `git config` or a commit template. Never set
  `GIT_AUTHOR_NAME` or `GIT_AUTHOR_EMAIL`.
- A commit you make in your own terminal is outside the harness settings, so it
  keeps you as committer.
- Name the App for the project, never for a model vendor, so that the statement
  above stays true.
- Do not add the agent App to `git.automation_profiles`. That setting skips
  branch naming and the commit message shape rules for the actors it names,
  which would take the agent's commits out of the standard.
- A `<slug>[bot]` co-author trailer stays refused. `git.ai_attribution` reads
  the commit message, and the committer field is not part of the message.

What GitHub shows:

| Action | GitHub shows |
|---|---|
| A commit made in a session with the two variables | author: the human; committer: `<slug>[bot]` |
| A commit made without them | the human author and committer |
| A pull request opened with the agent App's token | author `<slug>[bot]` |
| A merge made with the agent App's token | merged by `<slug>[bot]` |
| A merge commit GitHub makes for that merge | author `<slug>[bot]` |

The first two rows state what git records. How GitHub displays a pushed commit
with a bot committer has not been observed yet.

## Signed commits

This setup needs no signing key and signs nothing. A commit made on your computer
with the bot as committer has no signature from the App. If your repository
requires signed commits, plan for these commits before you add the two values:
they can be refused, or shown as unverified, because no key for the bot exists.

Limit: how GitHub shows a commit signed with the developer's key under a bot
committer has not been tested.

## A second developer or computer

A repository has one agent App. Each developer's computer holds its own key for
that App, so one key can be removed without touching the others. Do not run
`setup-apps` again for this, and do not copy a key from another computer.

1. Open the App's settings page, `https://github.com/settings/apps/<slug>`, or
   `https://github.com/organizations/<org>/settings/apps/<slug>` for an
   organization. Note the Client ID.
2. Under "Private keys", click "Generate a private key". The browser downloads a
   `.pem` file. An App can hold up to 25 keys. GitHub places this page in the
   settings of the account that owns the App, and documents App managers only
   for organizations. So for an App owned by a personal account, plan on the
   owner generating the key and passing you the file by a private route. For an
   organization's App, an organization owner or App manager can do it.

   Limit: this is planning advice. GitHub's pages do not state that a personal
   App's key page is closed to everyone but the owner, and do not say that App
   managers can generate keys. CodeFlow has not tested either.

3. Make the file readable only by you, then store it:

   ```sh
   chmod 600 <path to the .pem file>
   codeflow remote import-agent-key --key-file <path to the .pem file> --client-id <Client ID>
   ```

The command needs no administrator role and no `gh` login. Run it in your own
terminal, in a clone of the repository. Before it stores anything, it signs a
token with the key and asks GitHub, as the App, which App this is. The key is
refused, and nothing is stored, when:

- GitHub does not accept the key for that Client ID;
- the App's owner differs from the repository's owner, or its permissions or
  events differ from the agent App's manifest;
- the App is not installed on this repository;
- the file is readable by anyone else, belongs to another user, is a link, or
  does not hold a private key;
- this computer already has an entry for the repository. Remove it first with
  the command the message names.

**Delete the file yourself afterwards.** The command leaves it in place, and its
last line tells you that it did:

```sh
rm <path to the .pem file>
```

Deleting does not erase the key from the disk. Copy-on-write disks, snapshots and
backups can keep it. If the file ever reached a synced folder, a shared disk or a
backup, treat the key as exposed: delete it under "Private keys" on the App's
page, generate a new one and import that.

When a computer is lost, or a developer leaves, delete that computer's key on the
same page. The other keys keep working.

## A computer with the settings but no key

A clone brings the committer settings with it, so this state is normal until the
developer imports a key.

| What the session does | What happens |
|---|---|
| `git commit` | Works, and names the agent App as committer. No key is involved |
| `codeflow remote agent-token` | Fails with a non-zero exit and prints no token: "no agent App entry for <owner>/<repo>" |
| `git push` through the credential helper | Gets no token, so it fails in a session and asks for a login in a terminal |
| `TOKEN=... && GH_TOKEN=... gh ...` | Stops at the `&&` |

Such a commit still names the App as committer, which is why the field is a
record and not proof. If a developer then pushes it under their own login, GitHub
shows them as the pusher.

## Undo it

Every write is printed with what undoes it. To remove an App, delete it on its
settings page under "Advanced", which is a web step. Then run the printed
commands for the variable, the secret and the stored key.

## More detail

- [Policy reference, "Mode A setup"](policy-reference.md#mode-a-setup-operator):
  how the authority App fits the remote protection.
- [CLI reference](cli.md): every `codeflow remote` command.
