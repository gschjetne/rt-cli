# rt

The command line of [review-tools](https://rt.schjetne.dev): your coding agent opens a
review with the decisions it needs, you answer in the browser (on your phone, if you are
away), and the agent carries on.

```sh
curl -fsSL https://github.com/gschjetne/rt-cli/releases/latest/download/install.sh | sh
```

On Windows, in PowerShell:

```powershell
irm https://github.com/gschjetne/rt-cli/releases/latest/download/install.ps1 | iex
```

Both check the download against the release's `SHA256SUMS`. Each build also carries
build provenance: `gh attestation verify rt-x86_64-linux --repo gschjetne/rt-cli`.

Then, in a clone:

```sh
rt init          # sign in, install the app on the repository, get an agent token
rt agent-instructions >> CLAUDE.md
```

A clone of a repository on GitHub connects to the repository's project, and the people
with access to the repository can read its reviews. Any other clone, with no remote or one
on another host such as Gerrit, connects to a project of your own, named after the
directory (`rt init --name` chooses another). Only you can see its reviews, and nobody can
be invited to them. `rt init --name` with the same name in another clone connects to the
same project.

`rt init` connects to https://rt.schjetne.dev unless `RT_ENDPOINT` names another
instance. Agents in containers use `RT_ENDPOINT` and `RT_TOKEN` (or a network secret);
`rt whoami` shows where `rt` goes and as whom.

- `crates/rt`: the command line.
- `crates/rt-client`: the HTTP client of the API, with endpoint and token discovery.
- `docs/agent`: what agents read: the instructions `rt agent-instructions` prints, and
  how to write a review document.

The API's types and the review document format are in
[rt-api](https://github.com/gschjetne/rt-api).

## Releasing

Set the version in `Cargo.toml`, then push a tag `v<version>`: CI builds rt for every
system and publishes the release.

MIT licensed.
