# Publishing setup — what `publish.yml` needs before it can run

`.github/workflows/publish.yml` performs the steps that used to follow a release by hand:
the SourceForge source mirror, the SourceForge FRS installer upload, the
hamradiotools.io update-endpoint bump, the site build and deploy, and a check that the
deploy actually reached the live URL.

It has **never been executed**. Everything below is what it needs before its first run, and
it will not half-work: a missing secret fails the run inside twenty seconds and names every
secret that is absent, and no step degrades to a silent no-op.

---

## 1. Two prerequisites that are not secrets

### The site repo's `main` must be the tree the live site was built from

As of 2026-08-07 it is not. `kd9taw/hamradiotools@main` commits `version.json` with
`"latest": "1.0.0"`, while hamradiotools.io serves `1.0.2` — the 1.0.1 and 1.0.2 bumps
were built and deployed from a local branch that was never pushed.

This matters because `wrangler pages deploy --branch=main` only *labels* a deployment as
production; it does not read git. It uploads whatever directory it is pointed at. So `main`
can sit arbitrarily far behind what is live and nothing anywhere says so. A workflow that
clones `main`, builds it and deploys it would publish the older site over the current one,
silently, as a side effect of a release.

`publish.yml` refuses to do that: before it writes anything to the site clone it compares
the `latest` that `main` commits against the `latest` the live site serves, and stops with
both values printed if they disagree. **Push the site work that produced the live site to
`main`**, and the two agree from then on.

### The workflow file must be on `main` in this repo

A `release:` trigger only fires from the default branch's copy of a workflow.

---

## 2. The four repository secrets

Add each at **Settings -> Secrets and variables -> Actions -> New repository secret**, on
`kd9taw/nexus`. Names are exact.

| Secret | What it is |
|---|---|
| `SF_SSH_KEY` | SourceForge SSH private key — used for both the git mirror and the FRS upload |
| `CLOUDFLARE_API_TOKEN` | Cloudflare Pages deploy token |
| `CLOUDFLARE_ACCOUNT_ID` | Cloudflare account identifier |
| `SITE_REPO_TOKEN` | A token that can push to `kd9taw/hamradiotools` |

### `SF_SSH_KEY`

The private half of the keypair the SourceForge account already accepts — on the
workstation, `~/.ssh/hamradiotools_ed25519`. Paste the **whole file**, its opening and
closing marker lines included. A key pasted without them does not parse, and the mirror
fails at authentication rather than at the paste.

One constraint: **the key must have no passphrase.** CI cannot answer a prompt, and the
upload runs with `BatchMode=yes` so it fails rather than hanging. If the working key has a
passphrase, generate a second one for CI and add its public half at SourceForge under
Account -> SSH Settings:

```
ssh-keygen -t ed25519 -N '' -f ~/.ssh/nexus_ci_ed25519 -C 'nexus-ci'
```

The same key serves `git.code.sf.net` (mirror) and `frs.sourceforge.net` (upload); they are
one account.

### `CLOUDFLARE_API_TOKEN`

Cloudflare dashboard -> My Profile -> API Tokens -> Create Token. Either the **Edit
Cloudflare Workers** template, or a custom token whose only permission is
**Account -> Cloudflare Pages -> Edit**, scoped to the one account. Not the Global API Key —
that grants everything and cannot be scoped.

### `CLOUDFLARE_ACCOUNT_ID`

Cloudflare dashboard -> Workers & Pages -> Account details, or the hex string in the
dashboard URL after `/accounts/`.

### `SITE_REPO_TOKEN`

A fine-grained personal access token:

- Repository access: **Only select repositories** -> `kd9taw/hamradiotools`
- Permissions: **Contents: Read and write**. Nothing else is needed.

Fine-grained tokens expire. When it does, the publish fails at the site step with a push
rejection — loud, but on release night. Set the longest lifetime the account allows and note
the expiry date somewhere it will be seen.

Reading the GitHub Release itself needs no secret; that uses the built-in token.

---

## 3. The trigger, and why it will not fire on its own yet

`publish.yml` triggers on `release: published`. **That event does not fire for a release
created by another workflow using the built-in `GITHUB_TOKEN`** — GitHub suppresses those to
stop workflows triggering each other — and `release.yml`'s publish job creates the release
with exactly that (`GH_TOKEN: ${{ github.token }}`). `workflow_dispatch` and
`repository_dispatch` are the documented exceptions.

So today:

- A release published **by a human** (web UI, or `gh release create` under a personal token)
  triggers `publish.yml` automatically.
- A release published **by `release.yml`** does not. Run `publish.yml` from
  Actions -> Publish downstream -> Run workflow, with the tag (`v1.0.3`). It publishes the
  mechanical remainder for a release that already exists; it cannot create or modify one.

To close that gap, `release.yml` needs one line at the end of its publish job:

```yaml
      - name: Hand off to the downstream publish
        env:
          GH_TOKEN: ${{ secrets.RELEASE_PAT }}   # a PAT with actions:write — NOT github.token
        run: gh workflow run publish.yml -f tag="${{ steps.ver.outputs.tag }}"
```

That is a change to `release.yml` and a fifth secret, and it is deliberately not made here —
the author of `publish.yml` was scoped to `publish.yml`. Until it is made, the dispatch above
is the path.

---

## 4. What is still manual after every release

1. **SourceForge "Default Download."** Browser only; there is no API, a scripted POST
   returns 401. Files -> `v<ver>/Nexus_<ver>_x64-setup.exe` -> (i) ->
   *Default Download For: Windows* -> Save. Until it is flipped, SF's `best_release.json` —
   the app's fallback update feed, used when the site is unreachable — keeps serving the
   previous version, and the daily *Release endpoints* check reports it stale.
2. **Release prose.** The site endpoint's `notes` field is operator prose. `publish.yml`
   takes it from `docs/RELEASE_NOTES-<ver>.md` when that file exists at the tag (the same
   file `release.yml` uses for the GitHub release body), otherwise it falls back to the
   release body and says in the run summary that it degraded. Write the prose before
   tagging to control that field.
3. **The site's copy of the operator manual.** `scripts/sync-manual.mjs` syncs it from a
   local Nexus checkout, which does not exist on a runner, so the site build there runs in
   verify-only mode. A manual change still ships with a hand-run site build.

---

## 5. First run

The workflow has never executed. Run it the first time deliberately, not on release night:

1. Push the site repo's live tree to `main` (section 1).
2. Add the four secrets (section 2).
3. Actions -> Publish downstream -> Run workflow -> tag `v1.0.2`.

Re-publishing the current release is a safe smoke test. The FRS upload creates its directory
with `-mkdir`, whose failure is ignored when the directory exists, and re-uploads the same
bytes; the site step finds `version.json` already at `1.0.2` and commits nothing; the deploy
and the live verification run regardless, because committed is not served. If the site base
gate from section 1 has not been satisfied it stops there, before changing anything on the
site, with both versions printed.
