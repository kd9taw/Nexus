# Remote staging administration

The pilot serves `https://remote-staging.hamradiotools.io`. It displays radio and
SPE/KPA amplifier readings after account sign-in, trial admission, station pairing
and local browser approval. The first acceptance session uses a desktop browser
on another network, followed by phone browsers on cellular.

## Create the Auth0 tenant

The account owner creates the tenant in the [Auth0 dashboard](https://manage.auth0.com/).
Use a dedicated development tenant, for example `nexus-remote-staging` if available,
and retain its generated domain, including the region segment. Use the tenant's
`auth0.com` domain for this pilot. The deployment check accepts that domain family;
a custom login domain would need a separately reviewed configuration change.
See [Create tenants](https://auth0.com/docs/get-started/auth0-overview/create-tenants).

Create one **Single Page Application** named **Nexus Remote staging** and one API:

| Setting | Value |
|---|---|
| Application type | Single Page Application |
| Allowed Callback URLs | `https://remote-staging.hamradiotools.io` |
| Allowed Logout URLs | `https://remote-staging.hamradiotools.io` |
| Allowed Web Origins | `https://remote-staging.hamradiotools.io` |
| API name | Nexus Remote staging API |
| API identifier / audience | `https://remote-staging.hamradiotools.io/api` |
| API signing algorithm | RS256 |
| Browser login | Universal Login, authorization code with PKCE |
| Browser token storage | SDK memory; refresh tokens are not requested |

Enable the tenant's database connection for this application so the pilot operator
can create an account. Keep the first trial limited to the maintainer. The app is
a public client; its **Client ID** and tenant domain are the two identifiers needed
to finish configuration. No client secret is used by the browser or Worker.
The setup follows Auth0's [SPA registration](https://auth0.com/docs/get-started/auth0-overview/create-applications/single-page-web-apps)
and [API registration](https://auth0.com/docs/get-started/apis) documentation.

In `kd9taw/Nexus` → Settings → Environments → **production**, add these **variables**:

| Variable | Value |
|---|---|
| `REMOTE_AUTH0_ISSUER` | `https://YOUR-TENANT.REGION.auth0.com/`, copied from the tenant, with a trailing slash |
| `REMOTE_AUTH0_CLIENT_ID` | The SPA application's public Client ID |

The staging API audience is fixed in the tooling. The preflight checks public OIDC
discovery and signing keys; it cannot read the app's callback list or prove a real
sign-in. Complete those acceptance checks in the actual browser.

## Cloudflare and deployment

The workflow `.github/workflows/remote-staging.yml` reuses the existing **production**
environment's `CLOUDFLARE_ACCOUNT_ID` and `CLOUDFLARE_API_TOKEN`. If the Pages token
lacks the required grants, place a separately scoped token in the same environment
as `REMOTE_CLOUDFLARE_API_TOKEN`; Remote prefers it without changing the site token.
Token values stay in GitHub secrets and are passed only to administrator steps.

Cloudflare documents its [CI token setup](https://developers.cloudflare.com/workers/ci-cd/external-cicd/github-actions/).
Scope the token to the existing account and the `hamradiotools.io` zone. Remote
needs Workers script deployment/custom-domain access, D1 creation/migration access,
and zone lookup for the custom domain. The [D1 inventory](https://developers.cloudflare.com/api/resources/d1/subresources/database/methods/list/)
accepts D1 Read or Write, while [database creation](https://developers.cloudflare.com/api/resources/d1/subresources/database/methods/create/)
requires D1 Write. Successful read-only inspection does **not** attest write grants.
Review the actual token permissions before a write; the tooling does not add grants
or upgrade a Cloudflare plan.

The workflow is dispatched explicitly from the reviewed branch. Select an operation:

1. **inspect** reads D1 and Worker metadata. It reports only the named staging resources,
   rejects a hostname owned by another service or an existing Worker without the Remote
   staging tag, and changes nothing.
2. **provision** creates only the dedicated `nexus-remote-staging` D1 database if absent.
   A rerun reuses an exact-name match. Ambiguous results or denied reads fail before a write.
   Worker/Static Assets and the SQLite `StationRoom` namespace are installed by deployment.
3. **deploy** checks the Auth0 public configuration and anonymously accessible source SHA,
   runs the `remote-web` gates from `ci.yml`, resolves the database and packages the tested
   Worker/browser bytes. It retains a source/hash receipt, applies staging migrations,
   and uploads without rebuilding. The final check requires the live Worker revision,
   identity configuration and every browser asset hash to match, and proves that anonymous
   and foreign-Origin requests are refused.
4. **recover** handles a partial upload that created the dedicated Worker before its
   ownership tag was recorded. Supply the source revision, Worker SHA-256 and D1 UUID
   from the reviewed failed deployment's retained artifact. It requires the uploaded
   module bytes, identity variables, database, namespace and runtime settings to match
   before adding the ownership tag. It neither replaces Worker code nor attaches a domain.
   For an older upload that discovered adjacent text files, also supply the receipt's
   SHA-256 values for `assets/index.html`, `assets/remote-licenses.txt` and
   `migrations/0001_observation.sql` as the `recovery_additional_modules` JSON object.
   All three must match; no unknown or unchecked additional module is accepted.

Worker tags are applied through Cloudflare's script-settings API and read back;
the pinned Wrangler does not support a top-level `tags` configuration field.
The Worker is uploaded with route reconciliation disabled. After its bytes and
bindings are confirmed, the administrator tool attaches only the staging custom
domain through the domain API. It does not use Wrangler's noninteractive bulk DNS
overwrite behavior. Provider failures report numeric error codes without copying
provider response bodies, account identifiers or credentials into public logs.
The upload tool copies the verified Worker into an isolated temporary module directory:
Wrangler's `--no-bundle` still discovers adjacent text/SQL files. Its actual dry-run
output must contain exactly that Worker module with the receipt's hash. The deployed
module inventory and non-versioned script settings are read back before domain attachment.

Example administrator commands, after source publication and the required review:

```sh
gh workflow run remote-staging.yml --repo kd9taw/Nexus --ref REVIEWED_BRANCH -f operation=inspect
gh workflow run remote-staging.yml --repo kd9taw/Nexus --ref REVIEWED_BRANCH -f operation=provision
gh workflow run remote-staging.yml --repo kd9taw/Nexus --ref REVIEWED_BRANCH -f operation=deploy
```

[GitHub requires a dispatched workflow to exist on the default branch first](https://docs.github.com/en/actions/how-tos/manage-workflow-runs/manually-run-a-workflow).
Publishing a feature branch alone does not install a newly named workflow. Introduce the reviewed
workflow on main before dispatch, and read the run's actual job/step results. Ordinary
pushes and pull requests never deploy this service. The workflow's scoped web checks
do not replace the application CI or desktop installer/hardware acceptance gates.

The workflow can be registered on main ahead of the Remote implementation. In that
case, dispatch with `--ref` set to the reviewed Remote branch. A ref without the
administration scripts fails before any provider access.

The artifact contains only the compiled Worker, browser assets/licenses, migrations,
public configuration and `manifest.json`. Its source URL points to the exact commit.
Wrangler diagnostics use a discarded temporary directory and are not uploaded as
artifacts or printed to the public job log. A failed migration/deploy is reported as a
failure; inspect the Cloudflare resource state before retrying an ambiguous operation.

## First browser-to-shack acceptance

After signing in, use the account UUID displayed by Remote to enable a short manual
trial as described in [README.md](README.md#staging-setup). Then pair the desktop,
approve the browser locally and enable observation. Verify actual readings against
the rig and amplifier; never count test fixtures as a hardware result.

Exercise local disable, device/station revocation, trial expiry, lost network,
desktop restart, locked/unavailable OS credential storage and stale measurements.
Remote must start disabled after a desktop restart. Repeat on mobile web with
cellular, backgrounding and screen locking. Record the actual platform, revision,
measured interruption/reconnect behavior and Cloudflare usage before widening the pilot.

## Rollback

Disable observation locally and revoke pilot stations/trials first. Confirm browser
retirement. Keep the authority database and its revocation generations. If a previous
Worker version was accepted and is compatible with the applied D1/DO migrations,
restore that version through Cloudflare's deployment history and verify its matching
receipt. Otherwise withdraw only the staging custom domain while investigating.
Do not roll back by deleting the database or creating an empty replacement.

Cloudflare [Worker rollbacks](https://developers.cloudflare.com/workers/versions-and-deployments/rollbacks/)
do not undo external database migrations and cannot cross a Durable Object lifecycle
change. Review compatibility before restoring code;
do not automatically roll back after an ambiguous upload or revive revoked trust.
The normal Nexus release, marketing Pages site and in-app update feed have their own
workflows and are outside this staging operation.
