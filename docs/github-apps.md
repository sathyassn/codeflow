# Set up the GitHub Apps

This guide is for a repository administrator who wants agent sessions to stop
acting under their own GitHub login, and for each developer on that repository.
One command creates up to three GitHub Apps, and each App takes two browser
clicks. The PR App takes one more click to turn on its device flow.

This page is the one place where the setup is described. Other CodeFlow pages
link here.

## How agent identity works

This holds for CodeFlow's own repository and for every project that uses
CodeFlow.

There are three Apps. Only the authority App is needed for the remote
protection; the agent App and the PR App are each optional.

| App | Permissions | What it may do | Needed |
|---|---|---|---|
| Authority App | Checks write | Creates the `authority:` check runs from the default branch | For Mode A protection |
| Agent App | Contents and Pull requests write | Gives agent sessions a short-lived installation token for pushes, reads, `gh` and git's credential helper, and names `<slug>[bot]` as committer | Optional |
| PR App | Pull requests write and Contents read | Opens and edits agent pull requests with each developer's user access token, so the developer is the author. It cannot merge or push | Optional |

GitHub adds Metadata read to every App.

Who GitHub records:

| What GitHub records | Who it is |
|---|---|
| Commit author | The developer, always |
| Commit committer | `<slug>[bot]` in an agent session that has the two committer values ([Author and committer](#author-and-committer)); otherwise the developer |
| Pull request author | Whoever owns the token that opens the pull request: the developer with the PR App's user token or the developer's own login, `<slug>[bot]` with the agent App's installation token |
| Pusher | The agent App with its installation token; otherwise the developer |

Pull request authorship follows the token that opens the pull request, never
whether a key exists.

Which token does what:

| Act | Token, in this order |
|---|---|
| Open and edit pull requests | The PR App's user access token (`codeflow remote pr-token`) where the project has the PR App; else the agent App's installation token (`codeflow remote agent-token`) where it has the agent App; else the developer's own login |
| `git push`, reads and git's credential helper | The agent App's installation token where the project has the agent App; else the developer's own git credentials |

`codeflow remote setup-apps` prints these lines for the set-up it made.

### The four set-ups

| Set-up | Pull request author | Commit author, and committer | Key, and where it is kept | What the guard enforces |
|---|---|---|---|---|
| No App | The developer, through the developer's own login | The developer, and the developer | None | The review rule (git.pr_review_approval) and the git floor, which refuses `gh pr merge` into a protected base. The developer's own login can merge wherever GitHub allows the developer to |
| Agent App only | `<slug>[bot]`, through the installation token | The developer, and `<slug>[bot]` where the session sets the committer values | The agent App's key, in the operating system's credential store on each developer's computer | The review rule and the git floor |
| PR App only | The developer, through the PR App's user token | The developer, and the developer (pushes use the developer's own git credentials) | No key CodeFlow keeps; the developer's grant in the operating system's credential store | The review rule and the git floor |
| Agent App and PR App | The developer, through the PR App's user token | The developer, and `<slug>[bot]` where the session sets the committer values | The agent App's key and the developer's grant, each in the operating system's credential store | The review rule and the git floor |

- **No App:** agents act with the developer's own login, which can merge.
  The review rule still applies.
- **The PR App holds no key CodeFlow keeps.** GitHub returns a private key, a
  client secret and a webhook secret when it creates any App from a manifest.
  `setup-apps` drops all three, and the PR App is used only through the device
  flow, which needs none of them. GitHub keeps a key on the App, because "If
  your GitHub App has only one key, you will need to generate a new key before
  deleting the old key"
  ([managing private keys](https://docs.github.com/en/apps/creating-github-apps/authenticating-with-a-github-app/managing-private-keys-for-github-apps)).
  That key can be replaced on the App's page, but not removed.
- **The agent App's key is needed only to mint installation tokens.** A pull
  request opened with one shows the bot.

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
- **Agent sessions never approve or dismiss reviews.** The git guard refuses
  `gh pr review --approve`, and a `gh api` or `curl` request that posts an
  `APPROVE` review or event or puts a review dismissal, whatever token the
  command uses. A REST review request whose body the guard cannot read, such
  as one from standard input, refuses too. The rule is the policy key
  `git.pr_review_approval`, on when the key is absent; a project may turn it
  off. The guard covers only the commands it can parse in a guarded session.
  GitHub still accepts these calls from outside the guard, from any token with
  Pull requests write: the PR App's user token, the agent App's installation
  token and the developer's own login included.

### Limits where identity checks are off and the agent is not sandboxed

For a project that uses CodeFlow with its identity checks off, and an agent
session that runs without a sandbox:

- Where the project has the PR App, its token can approve another developer's
  pull request from outside the guard, and that approval is the developer's.
- Any process of the same user that can reach the credential store can read
  the PR App grant and refresh it without `pr-token`, so it skips the grant
  check below.
- Where the project has the agent App, its installation token is still in the
  session. It can merge and push as the bot, and approve wherever the guard
  does not see the call.
- The grant check runs at the next `pr-token` print only. A token already
  printed stays outside it until it expires. An installation of the PR App
  with `repository_selection: all` gives the token every repository of that
  installation that the user can access.

## The agent App's levels

Nothing requires the agent App, for a project or for a developer. Each computer
sits at one of three levels, and developers on one repository can differ. The
PR App is separate: any level can add it, and then pull requests show the
developer as author.

| Level | What it needs | Commits record | Pushes run as | Pull requests opened without the PR App show as author | What it gives up |
|---|---|---|---|---|---|
| No App | Nothing | You as author and committer | Your login | You | Agent work looks like yours, and the session has everything your login has |
| Committer values only | The two values in the harness settings; no key on this computer | You as author, `<slug>[bot]` as committer | Your login | You | The session still has everything your login has; the committer is a record only |
| Key on the computer | The values, plus this computer's key for the App | You as author, `<slug>[bot]` as committer | The App's installation token | `<slug>[bot]` | One key to hold and rotate per computer, and a credential store |

The agent App has no Administration, Workflows, Environments, Deployments,
Commit statuses or Checks permission. The committer field needs no key and no
signature. It is text in the commit, so the second level works on any computer
that has the settings.

## Before you start

- You are an administrator of the repository, signed in with your own
  `gh auth login`. The command refuses a GitHub App token, and an account that
  is not the repository's administrator, before it writes anything.
- You run it in your own terminal, in a clone of the repository, never in an
  agent session.
- The agent App's key, and each developer's PR App grant, need a credential
  store: the login keychain on macOS, or the Secret Service through
  `secret-tool` on Linux. Windows has no supported store yet, so use
  `--no-agent` there.
- `--no-agent` skips the agent App and `--no-pr-app` skips the PR App. With
  both, the command sets up the authority App alone.
- GitHub limits an App name to 34 characters, and names are unique across
  GitHub. The default names are `codeflow-authority-<repo>`,
  `codeflow-agent-<repo>` and `codeflow-pr-<repo>`. When one is too long,
  choose another with `--authority-name`, `--agent-name` or `--pr-name`.

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
credential store. The PR App's slug, App id and client id, none of them
secret, go to the repository variable `CODEFLOW_PR_APP`, so a second run finds
the App. Every write is printed with what undoes it.

Then turn on the PR App's device flow. A GitHub App manifest has no setting
for it, so the command cannot do this for you, and it prints the click with
the page:

1. Open the PR App's settings page,
   `https://github.com/settings/apps/<slug>`, or
   `https://github.com/organizations/<org>/settings/apps/<slug>` for an
   organization.
2. Under "Identifying and authorizing users", select "Enable Device Flow" and
   save.

Leave expiring user access tokens on. GitHub turns them on for a new App unless
you opt out, and without them a user access token cannot be renewed.

The command then prints the `pr-login` line each developer runs, with the
App's id, its client id and the repository's id as GitHub returned them:

```text
then each developer runs `codeflow remote pr-login --repo <owner>/<repo> --app <slug> --app-id <id> --client-id <client id> --repository-id <id>` once
```

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

**The PR App** is found through the variable `CODEFLOW_PR_APP`. Its key was
dropped, so it cannot be read as itself, and a private App answers 404 as the
authority App does. The command then prints `PR App: present but not
verified, nothing created`, what it checked, the two pages to compare and the
`pr-login` line again. It never creates a second PR App. When your login can
read the App, the command checks its id, client id, permissions and events.

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
and PR Apps. Without `--authority-name` it looks for the default name, and its message
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

This applies where the project has the PR App. Each developer does it once
per computer, after the PR App is installed and has its device flow on. It
needs no agent App and no agent App key on the computer. Run the line
`setup-apps` printed:

```sh
codeflow remote pr-login --repo <owner>/<repo> --app <slug> --app-id <id> --client-id <client id> --repository-id <id>
```

The command prints an address and a code. Open the address, enter the code and
approve the PR App. It uses GitHub's device flow, so it needs no personal
access token, no `gh` login and no client secret: "The `client_secret` is not
needed for the device flow"
([authorizing OAuth apps](https://docs.github.com/en/apps/oauth-apps/building-oauth-apps/authorizing-oauth-apps)).
Before it stores anything, the command checks the grant (below). The grant is
kept in this computer's credential store under the service `codeflow-pr-user`,
account `<owner>/<repo>`, apart from the agent App's key. It is never written
to a file, an environment variable or a process argument list. The command
prints the login it authorized and the command that removes the grant.

In an organization that uses SAML single sign-on, start an active SAML session
for the organization before you authorize. Otherwise the token cannot see the
organization's resources. GitHub's page says to "instruct the user to start an
active SAML session for their organization before reauthorizing"
([generating a user access token](https://docs.github.com/en/apps/creating-github-apps/authenticating-with-a-github-app/generating-a-user-access-token-for-a-github-app)).

In a session, `codeflow remote pr-token` prints your user access token for the
PR App, for `GH_TOKEN`. A pull request opened or edited with it shows you as
author. See [Tokens in a session](#tokens-in-a-session).

- **Renewal is silent.** GitHub ends a user access token after 8 hours and its
  refresh token after six months. Near the token's end the command renews it
  with the refresh token. "Once you use a refresh token, that refresh token and
  the old user access token will no longer work"
  ([refreshing user access tokens](https://docs.github.com/en/apps/creating-github-apps/authenticating-with-a-github-app/refreshing-user-access-tokens)),
  so the new grant is stored before the new token is printed, and an empty
  lock file under `~/.codeflow/locks` lets only one renewal run at a time on a
  computer.
- **One message asks you to authorize again**, and only when GitHub refuses the
  saved authorization or it has expired: it names the whole `pr-login` line. A
  network failure or a GitHub error names its own cause and keeps the saved
  authorization.
- **No token appears in an error.** Messages redact both the access token and
  the refresh token. The refresh token is never printed; the access token
  appears only as the printed token.
- **A revoked authorization is noticed.** Before it prints a saved token, the
  command asks GitHub who the token belongs to (`GET /user`). A token GitHub no
  longer takes leads to a renewal, which a revoked authorization refuses.

When the device flow is off, `pr-login` says so and names the App's settings
page.

### The grant check

A user access token cannot be narrowed when it is issued or renewed. "A
GitHub App user access token only has permissions that both the user and the
app have"
([generating a user access token](https://docs.github.com/en/apps/creating-github-apps/authenticating-with-a-github-app/generating-a-user-access-token-for-a-github-app)),
and the page names no parameter that restricts permissions. One token reaches
every installation of the App that the user can access. So before `pr-login`
stores a grant, and before `pr-token` prints any token, the command reads
every page of `GET /user/installations` with that token and refuses, storing
and printing nothing, unless:

- every installation listed has exactly `pull_requests: write`,
  `contents: read` and `metadata: read`, and no other permission;
- each installation's App id, and its client id where GitHub returns one,
  equal the ids on the `pr-login` line;
- the repository id on the `pr-login` line is among an installation's
  repositories (`GET /user/installations/{installation_id}/repositories`,
  every page). The repository is matched by that id, never by the owner and
  name of the working copy.

A page that is missing, cut short or cannot be read refuses. A refusal names
the installation, what differs, and the page that fixes it: the App's
permissions page, and the installation's page, where its owner approves
permissions. Owners approve permissions per installation: "If the account
owner does not approve the new permissions, their installation will continue
to use the old permissions"
([choosing permissions](https://docs.github.com/en/apps/creating-github-apps/registering-a-github-app/choosing-permissions-for-a-github-app)).

Limits: the check runs only inside `pr-token` and `pr-login`. A widened App
stops the next print, and a token already printed stays outside the check
until it expires. Any process of the same user that can reach the credential
store can read the grant and refresh it without `pr-token`, and so skips the
check. An installation with `repository_selection: all` passes, because the
check finds the repository among its repositories; the token then reaches
every repository of that installation that the user can access.

## Tokens in a session

| Action | Token | Command |
|---|---|---|
| Open, edit or comment on a pull request (`gh`), where the project has the PR App | Your user access token for the PR App | `codeflow remote pr-token` |
| The same, without the PR App, where the project has the agent App | The agent App's installation token | `codeflow remote agent-token` |
| `git push`, reads and git's credential helper, where the project has the agent App | The agent App's installation token | `codeflow remote agent-token --git-credential` |
| Anything, with neither App | Your own login and git credentials | none |

- `gh`, as you, with the PR App:

  ```sh
  TOKEN="$(codeflow remote pr-token)" && GH_TOKEN="$TOKEN" gh pr create ...
  ```

  Keep the `&&`. Without it, a failed token leaves `GH_TOKEN` empty, and `gh`
  may fall back to your own login, so the action would run with everything
  your login has. The PR App's token cannot merge or push: merging needs
  Contents write, which the App does not hold, and the token "only has
  permissions that both the user and the app have". It never acts as git's
  credential helper.
- `gh`, as the agent App's bot: use `codeflow remote agent-token`. A pull
  request opened this way shows `<slug>[bot]` as its author.
- `git push`, with the agent App's installation token:

  ```sh
  git -c credential.helper= -c 'credential.helper=!codeflow remote agent-token --git-credential' push
  ```

`codeflow remote agent-token` prints an installation token for this repository,
valid for one hour. It reads the key from the credential store and never prints
it.

The commit identity does not depend on the token: you are the author, and the
agent App's bot is the committer where the session sets the committer values.

The custody of the key and the grant rests on the session's sandbox. A
sandboxed session must not reach the credential store, and only `codeflow
remote agent-token` and `codeflow remote pr-token` run outside the sandbox.
Check this on your host: from a sandboxed session, run the following. Each
prints only a byte count, and each count must be 0.

```sh
security find-generic-password -s codeflow-agent-app -a <owner>/<repo> -w | wc -c
security find-generic-password -s codeflow-pr-user -a <owner>/<repo> -w | wc -c
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
| A pull request opened with your user access token for the PR App (`pr-token`) | author: you, with the App's badge |
| A pull request opened with the agent App's installation token | author `<slug>[bot]` |
| A pull request opened with your own login | author: you |
| A merge made with the agent App's installation token | merged by `<slug>[bot]` |
| A merge commit GitHub makes for that merge | author `<slug>[bot]` |

The first two rows state what git records. How GitHub displays a pushed commit
with a bot committer has not been observed yet.

The row for your user access token rests on GitHub's statement that "API
requests made by an app on behalf of a user will be attributed to that user",
and that the GitHub UI shows "the user's avatar photo along with the app's
identicon badge as the author"
([authenticating on behalf of a user](https://docs.github.com/en/apps/creating-github-apps/authenticating-with-a-github-app/authenticating-with-a-github-app-on-behalf-of-a-user)).
GitHub's page gives an issue as its example and does not name pull requests or
merge commits. CodeFlow has not yet observed a pull request opened this way,
and no page read says whether GitHub shows the App on a review made with a
user access token.

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

Where the project has the PR App, each developer also runs the `pr-login`
line on their own computer, as in [Pull requests as you](#pull-requests-as-you).
It needs no agent App key. An authorization is one person's, so it is never
copied between developers.

## A computer with the settings but no key

A clone brings the committer settings with it, so this state is normal until the
developer imports a key.

| What the session does | What happens |
|---|---|
| `git commit` | Works, and names the agent App as committer. No key is involved |
| `codeflow remote agent-token` | Fails with a non-zero exit and prints no token: "no agent App entry for <owner>/<repo>" |
| `codeflow remote pr-token` with no `pr-login` yet | Fails with a non-zero exit and prints no token, naming `codeflow remote pr-login` and the line `setup-apps` printed |
| `git push` through the credential helper | Gets no token, so it fails in a session and asks for a login in a terminal |
| `TOKEN=... && GH_TOKEN=... gh ...` | Stops at the `&&` |

Such a commit still names the App as committer, which is why the field is a
record and not proof. If a developer then pushes it under their own login, GitHub
shows them as the pusher.

## Undo it

Every write is printed with what undoes it. To remove an App, delete it on its
settings page under "Advanced", which is a web step. Then run the printed
commands for the variables, the secret and the stored key.

To withdraw your own authorization of the PR App, revoke it on GitHub:
Settings, "Applications", the "Authorized GitHub Apps" tab, "Revoke". "The
authorization can only be revoked by the person who authorized the GitHub App"
([reviewing and revoking authorization](https://docs.github.com/en/apps/using-github-apps/reviewing-and-revoking-authorization-of-github-apps)).
Then remove the grant from this computer with the command `pr-login` printed,
such as `security delete-generic-password -s codeflow-pr-user -a <owner>/<repo>`
on macOS or `secret-tool clear service codeflow-pr-user account <owner>/<repo>`
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
- [REST API endpoints for GitHub App installations](https://docs.github.com/en/rest/apps/installations#list-app-installations-accessible-to-the-user-access-token):
  the installations and repositories the grant check reads.
- [Choosing permissions for a GitHub App](https://docs.github.com/en/apps/creating-github-apps/registering-a-github-app/choosing-permissions-for-a-github-app):
  permissions approved per installation.
- [Managing private keys for GitHub Apps](https://docs.github.com/en/apps/creating-github-apps/authenticating-with-a-github-app/managing-private-keys-for-github-apps):
  an App keeps at least one key.
- [Permissions required for GitHub Apps](https://docs.github.com/en/rest/authentication/permissions-required-for-github-apps):
  merging needs Contents write; creating, submitting and dismissing a review
  need Pull requests write.

## More detail

- [Policy reference, "Mode A setup"](policy-reference.md#mode-a-setup-operator):
  how the authority App fits the remote protection.
- [CLI reference](cli.md): every `codeflow remote` command.
