# Set up the GitHub Apps

This guide is for a repository administrator who wants agent sessions to stop
acting under their own GitHub login, and for each developer on that repository.
One command creates two GitHub Apps, and each App takes two browser clicks. The
agent App takes one more click to turn on its device flow.

This page is the one place where the setup is described. Other CodeFlow pages
link here.

## How agent identity works

This holds for CodeFlow's own repository and for every project that uses
CodeFlow. Agent pull requests show the developer as author, never the App's
bot.

| What GitHub records | Who it is | Set by |
|---|---|---|
| Commit author | The developer, always | git, unchanged |
| Commit committer | `<slug>[bot]` in an agent session that has the two committer values; otherwise the developer | The harness settings ([Author and committer](#author-and-committer)) |
| Pull request author | The developer, with the App's badge | The developer's user access token for the agent App ([Pull requests as you](#pull-requests-as-you)) |
| Pusher | The agent App | The App's installation token |

Which token does what:

- **User access token** (`codeflow remote agent-login` once, then
  `codeflow remote agent-token --user`): opens and edits pull requests. It can
  do only what both the developer and the agent App may do.
- **Installation token** (`codeflow remote agent-token`): pushes, by default.
  It has the agent App's permissions only and lasts one hour.
- **Neither:** the session uses the developer's own login.

What is optional: all of it. The agent App, the committer values and the
developer's authorization are each optional, per project and per developer.
Without them, agent work runs under the developer's own login.

Which checks enforce what:

- **Commit message rules never relax for agent work**, whoever is author or
  committer: conventional commits, the commit-msg hook and `git.ai_attribution`
  at block. The bot committer is not a route around them, and the agent App is
  never added to `git.automation_profiles`.
- **Identity checks are project policy: off by default for a project that uses
  CodeFlow, on in CodeFlow's own repository.** No CodeFlow check reads a commit
  author, a committer or a pull request author. The identity checks are who
  may produce a required check, which `codeflow remote protect` pins to the
  App that must create it, and which automation actors
  `git.automation_profiles` exempts from the branch naming and message shape
  rules, empty by default. A project turns them on itself. CodeFlow's own `main` pins its required checks to GitHub Actions.
- **GitHub enforces one more rule:** "Pull request authors cannot approve their
  own pull requests"
  ([approving a pull request with required reviews](https://docs.github.com/en/pull-requests/collaborating-with-pull-requests/reviewing-changes-in-pull-requests/approving-a-pull-request-with-required-reviews)).
  With the developer as author, someone else approves where a ruleset needs an
  approval.

## The two Apps

| App | Permissions | What it is for |
|---|---|---|
| Authority App | Checks write | Creates the `authority:` check runs from the default branch |
| Agent App | Contents and Pull requests write | Gives agent sessions a limited token on GitHub, and opens their pull requests as the developer |

GitHub adds Metadata read to every App. The agent App has no Administration,
Workflows, Environments, Deployments, Commit statuses or Checks permission.

## The agent App is optional

Nothing requires the agent App, for a project or for a developer. Each computer
sits at one of four levels, and developers on one repository can differ. The
last level is the default this guide sets up.

| Level | What it needs | Commits record | Pushes run as | Pull requests show as author | What it gives up |
|---|---|---|---|---|---|
| No App | Nothing | You as author and committer | Your login | You | Agent work looks like yours, and the session has everything your login has |
| Committer values only | The two values in the harness settings; no key on this computer | You as author, `<slug>[bot]` as committer | Your login | You | The session still has everything your login has; the committer is a record only |
| Key on the computer | The values, plus this computer's key for the App | You as author, `<slug>[bot]` as committer | The App's installation token | `<slug>[bot]` | One key to hold and rotate per computer, and a credential store |
| Key and your authorization (default) | The above, plus your one `agent-login` | You as author, `<slug>[bot]` as committer | The App's installation token | You, with the App's badge | One authorization to renew every six months |

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

Then turn on the agent App's device flow. A GitHub App manifest has no setting
for it, so the command cannot do this for you, and it prints the click with
the page:

1. Open the agent App's settings page,
   `https://github.com/settings/apps/<slug>`, or
   `https://github.com/organizations/<org>/settings/apps/<slug>` for an
   organization.
2. Under "Identifying and authorizing users", select "Enable Device Flow" and
   save.

Leave expiring user access tokens on. GitHub turns them on for a new App unless
you opt out, and without them a user access token cannot be renewed.

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

## Pull requests as you

This is the default for a project that uses CodeFlow. Each developer does it
once per computer, after the agent App exists, has its device flow on, and has
this computer's key (from `setup-apps` or `import-agent-key`).

```sh
codeflow remote agent-login
```

The command prints an address and a code. Open the address, enter the code and
approve the agent App. It uses GitHub's device flow, so it needs no personal
access token, no `gh` login and no App client secret. The grant is kept in this
computer's credential store under the service `codeflow-agent-user`, account
`<owner>/<repo>`, apart from the App's key. The command prints the login it
authorized and the command that removes the grant.

In a session, `codeflow remote agent-token --user` prints your user access
token for the agent App, for `GH_TOKEN`. See
[Use the agent App in a session](#use-the-agent-app-in-a-session).

- **Renewal is silent.** GitHub ends a user access token after 8 hours and its
  refresh token after six months. Near the token's end the command renews it
  with the refresh token. GitHub stops accepting a refresh token once it is
  used, so the new grant is stored before the new token is printed, and a lock
  under `~/.codeflow/locks` lets only one renewal run at a time on a computer.
- **One message asks you to authorize again**, and only when GitHub refuses the
  saved authorization or it has expired: `authorize again with codeflow remote
  agent-login --repo <owner>/<repo>`. A network failure or a GitHub error names
  its own cause and keeps the saved authorization.
- **A revoked authorization is noticed.** Before it prints a saved token, the
  command asks GitHub who the token belongs to (`GET /user`). A token GitHub no
  longer takes leads to a renewal, which a revoked authorization refuses.
- **The token is limited twice.** It has only the permissions both you and the
  agent App have, on resources both of you can reach.

When the device flow is off, `agent-login` says so and names the App's settings
page.

## Use the agent App in a session

| Action | Token | Command |
|---|---|---|
| Open, edit or comment on a pull request (`gh`) | Your user access token for the App | `codeflow remote agent-token --user` |
| `git push` | The App's installation token (default) | `codeflow remote agent-token --git-credential` |
| `git push`, if you choose | Your user access token for the App | `codeflow remote agent-token --user --git-credential` |

`codeflow remote agent-token` prints an installation token for this repository,
valid for one hour. It reads the key from the credential store and never prints
it.

- `gh`, as you (the default for pull requests):

  ```sh
  TOKEN="$(codeflow remote agent-token --user)" && GH_TOKEN="$TOKEN" gh pr create ...
  ```

  Keep the `&&`. Without it, a failed token leaves `GH_TOKEN` empty, and `gh`
  may fall back to your own login, so the action would run with everything
  your login has.
- `gh`, as the App's bot: use `codeflow remote agent-token` without `--user`.
  A pull request opened this way shows `<slug>[bot]` as its author.
- `git push`, with the installation token (the default):

  ```sh
  git -c credential.helper= -c 'credential.helper=!codeflow remote agent-token --git-credential' push
  ```

- `git push`, with your user access token, if you prefer pushes to carry your
  authorization too:

  ```sh
  git -c credential.helper= -c 'credential.helper=!codeflow remote agent-token --user --git-credential' push
  ```

  Limit: GitHub's pages on user access tokens do not mention git over HTTPS.
  The helper answers with the user name `x-access-token` and the token as the
  password, as for the installation token. CodeFlow has not tested this push
  against GitHub.

Either way the commit identity does not change: you are the author and the App's
bot is the committer.

The custody of the key and the grant rests on the session's sandbox. A
sandboxed session must not reach the credential store, and only `codeflow
remote agent-token` runs outside the sandbox. Check this on your host: from a
sandboxed session, run the following. Each prints only a byte count, and each
count must be 0.

```sh
security find-generic-password -s codeflow-agent-app -a <owner>/<repo> -w | wc -c
security find-generic-password -s codeflow-agent-user -a <owner>/<repo> -w | wc -c
```

When a count is not 0, stop: the session can read the key or the grant on this
host.

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
| A pull request opened with your user access token for the App (`--user`, the default) | author: you, with the App's badge |
| A pull request opened with the agent App's installation token | author `<slug>[bot]` |
| A merge made with the agent App's installation token | merged by `<slug>[bot]` |
| A merge commit GitHub makes for that merge | author `<slug>[bot]` |

The first two rows state what git records. How GitHub displays a pushed commit
with a bot committer has not been observed yet.

The row for your user access token rests on GitHub's statement that "API
requests made by an app on behalf of a user will be attributed to that user",
and that the GitHub UI shows "the user's avatar photo along with the app's
identicon badge as the author". GitHub's page gives an issue as its example and
does not name pull requests or merge commits. CodeFlow has not yet observed a
pull request opened this way.

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

- GitHub does not accept the key for that Client ID, or cannot be reached or
  fails, in which case the message says which and the key is not judged;
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

Each developer then runs `codeflow remote agent-login` on their own computer, as
in [Pull requests as you](#pull-requests-as-you). An authorization is one
person's, so it is never copied between developers.

## A computer with the settings but no key

A clone brings the committer settings with it, so this state is normal until the
developer imports a key.

| What the session does | What happens |
|---|---|
| `git commit` | Works, and names the agent App as committer. No key is involved |
| `codeflow remote agent-token` | Fails with a non-zero exit and prints no token: "no agent App entry for <owner>/<repo>" |
| `codeflow remote agent-token --user` with no `agent-login` yet | Fails with a non-zero exit and prints no token, naming `codeflow remote agent-login --repo <owner>/<repo>` |
| `git push` through the credential helper | Gets no token, so it fails in a session and asks for a login in a terminal |
| `TOKEN=... && GH_TOKEN=... gh ...` | Stops at the `&&` |

Such a commit still names the App as committer, which is why the field is a
record and not proof. If a developer then pushes it under their own login, GitHub
shows them as the pusher.

## Undo it

Every write is printed with what undoes it. To remove an App, delete it on its
settings page under "Advanced", which is a web step. Then run the printed
commands for the variable, the secret and the stored key.

To withdraw your own authorization, revoke it on GitHub: Settings,
"Applications", the "Authorized GitHub Apps" tab, "Revoke". Only the person who
authorized an App can revoke it. Then remove the grant from this computer with
the command `agent-login` printed, such as
`security delete-generic-password -s codeflow-agent-user -a <owner>/<repo>` on
macOS or `secret-tool clear service codeflow-agent-user account <owner>/<repo>`
on Linux.

## Sources

GitHub's pages, read 2026-11-07. Where a page is silent, this guide says so
above.

- [Generating a user access token for a GitHub App](https://docs.github.com/en/apps/creating-github-apps/authenticating-with-a-github-app/generating-a-user-access-token-for-a-github-app):
  the 8 hour token, the device flow, and "only has permissions that both the
  user and the app have".
- [Refreshing user access tokens](https://docs.github.com/en/apps/creating-github-apps/authenticating-with-a-github-app/refreshing-user-access-tokens):
  the six month refresh token, and that a used refresh token stops working.
- [Authorizing OAuth apps](https://docs.github.com/en/apps/oauth-apps/building-oauth-apps/authorizing-oauth-apps):
  the device flow's requests and answers.
- [Modifying a GitHub App registration](https://docs.github.com/en/apps/maintaining-github-apps/modifying-a-github-app-registration):
  "Enable Device Flow".
- [Registering a GitHub App from a manifest](https://docs.github.com/en/apps/sharing-github-apps/registering-a-github-app-from-a-manifest):
  the manifest's parameters, none of which is the device flow.
- [Authenticating with a GitHub App on behalf of a user](https://docs.github.com/en/apps/creating-github-apps/authenticating-with-a-github-app/authenticating-with-a-github-app-on-behalf-of-a-user):
  attribution to the user.
- [Reviewing and revoking authorization of GitHub Apps](https://docs.github.com/en/apps/using-github-apps/reviewing-and-revoking-authorization-of-github-apps):
  revoking.

## More detail

- [Policy reference, "Mode A setup"](policy-reference.md#mode-a-setup-operator):
  how the authority App fits the remote protection.
- [CLI reference](cli.md): every `codeflow remote` command.
