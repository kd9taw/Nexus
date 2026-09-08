# Nexus Remote observation pilot

This service connects an approved browser to an outbound Nexus desktop connection.
It displays radio and SPE/KPA amplifier observations. It has no command router,
receive audio, QSO writes, payment collection or native mobile application.

The browser uses Auth0 Authorization Code + PKCE. The Worker validates RS256 tokens
against a pinned issuer, API audience and SPA client, then maps `(issuer, subject)`
to an internal account ID. An account needs a manually enabled trial. Pairing and
browser approval are separate local actions in Nexus. Clearing browser cookies
requires approval again. A login or account recovery cannot approve a browser.

The desktop stores its Remote pairing and credential in one atomic OS credential
entry under `org.hamradiotools.nexus.remote`. No existing connector credential or
settings file is accessed. Enrollment proofs remain in memory. Pairing persists,
but observation starts disabled after every desktop restart. No inbound shack
port or router forwarding is required. The desktop is pinned to
`https://remote-staging.hamradiotools.io` for this pilot.

## Local verification

Use Node 24, the repository's pinned Rust toolchain and the Linux dependencies in
`.github/workflows/ci.yml`. The workflow is the command source; run `scripts/gates`.
The focused service commands, from the repository root, are:

```sh
npm --prefix ui ci
npm --prefix remote ci
npm --prefix ui run build:remote
WRANGLER_SEND_METRICS=false npm --prefix remote run types
npm --prefix remote run check
WRANGLER_SEND_METRICS=false npm --prefix remote run build
npm --prefix remote test
npm --prefix remote run test:native
npm --prefix remote run test:browser
```

`build` is a Wrangler **dry run**, not a deployment. The committed configuration
contains an unconfigured identity provider and an all-zero D1 ID. The service tests
use actual workerd, D1 SQL, hibernation and TCP WebSockets with signing keys created
in memory. The native integration builds and invokes an ignored Rust test probe;
its in-memory vault and synthetic radio readings exist only under `cfg(test)`.
It does not test an OS keychain, a physical radio, an Auth0 tenant or another network.

The compiled-browser check uses installed Google Chrome (`CHROME_BIN` can override
the executable) and a discarded temporary profile. It exercises the actual Auth0
SDK against a simulated issuer that verifies PKCE, then checks browser approval,
live observations, disconnect/reconnect, revocation and responsive layouts. Only
the test provider is intercepted; the browser reaches real local Worker APIs,
static assets and WebSockets. `NEXUS_REMOTE_BROWSER_ARTIFACTS` can name a directory
for screenshots and geometry results containing synthetic station data.

Wrangler 4.129.1 uses Miniflare 5.20260907.0-alpha. The harness pins that same version;
it is a development tool, not application runtime code. The service dependency
graph is checked with `npm audit` in CI.

A local workerd limitation is covered explicitly: when HTTP revokes both sockets
before either client has sent an application message, both close frames arrive
promptly but TCP teardown can wait for the `ws` client's 30-second close timeout.
A minimal service without Nexus reproduces it. Tests require immediate authority
retirement and entry into CLOSING, then require eventual close as well. The browser
source hides readings whenever its socket is no longer OPEN. Verify the actual
browser close/reconnect latency on the deployed staging endpoint before acceptance.

## Staging setup

Provision a separate Cloudflare Worker, SQLite Durable Object namespace and D1
database. Use the `remote-staging.hamradiotools.io` custom domain; keep `workers.dev`
and preview URLs disabled. No queue, object bucket, VM, TURN server or media relay
is needed for this observation stage. Account credentials are supplied through
normal provider tooling, never this repository or the browser configuration.

Create an Auth0 SPA application and an API using RS256. Set the SPA's allowed
callback URL, logout URL and web origin to `https://remote-staging.hamradiotools.io`.
The SPA is a public client: do not create or distribute a client secret. Set the
API audience to the exact identifier used by the Worker. No refresh-token storage
is enabled; browser tokens live in SDK memory. The API requires the SPA's `azp`.

After provisioning, create the ignored staging configuration using public IDs:

```sh
node remote/scripts/staging-config.mjs \
  --issuer https://YOUR-TENANT.auth0.com/ \
  --client-id YOUR-SPA-CLIENT-ID \
  --audience https://remote-staging.hamradiotools.io/api \
  --database-id YOUR-D1-UUID
```

Before deploying, review the generated configuration, publish the corresponding
source for the browser bundle, run the gates against that committed source, and
retain its commit ID and artifact hashes. Apply `migrations/0001_observation.sql`
to the staging D1 database, then deploy using `remote/wrangler.staging.jsonc`.
These are explicit administrator operations; no test or build command performs them.
The HTML response pins the identity provider in its CSP; static assets bypass the
Worker. Auth responses, cookies, tickets, provider claims and observations must not
be captured by application logs or added to tracing. Observability is disabled in
the committed configuration.

A user signs in first and gives the administrator their displayed account UUID.
Enable a short trial in D1, using that UUID and a reviewed UTC expiration in epoch
milliseconds:

```sql
INSERT INTO trials(account_id, enabled, expires_at)
SELECT id, 1, YOUR_EXPIRATION_MS FROM accounts WHERE id = 'YOUR_ACCOUNT_UUID'
ON CONFLICT(account_id) DO UPDATE SET enabled=1, expires_at=excluded.expires_at;
```

The user then starts pairing in desktop Settings, enters the displayed code in the
browser, compares account IDs and approves pairing at the shack. The browser next
requests its own approval; its short comparison code must match the desktop list.
Finally the user enables observation locally and selects **Observe station** in
the browser. The initial acceptance run uses a desktop browser, then mobile web.

## Bounds and recovery

- Two enabled stations per account, eight nonexpired browser enrollments per
  station, four simultaneous observers per station.
- Pairing and pending browser approvals expire after ten minutes. Approved browser
  cookies expire after thirty days. One-use WebSocket tickets expire after fifteen
  seconds. Credentials and tickets are stored as SHA-256 digests in D1.
- Browser sessions revalidate account, device and trial authority over HTTPS every
  thirty seconds, with a maximum sixty-second lease. Expiry alarms do not depend
  on a client sending another message. ACKs and pings cannot renew authority.
- A station emits at most one requested sample per 500 ms tick while watched.
  Request IDs bind publication to the current socket and bound its transport age.
  The cloud allows three seconds for a sample and five seconds for an observer ACK.
  It coalesces publications and retains only the latest frame in memory. Hibernation
  discards that frame and restores bounded authority/order/ACK state from attachments.
  An already requested sample finishes when the last observer leaves; no further
  sample is requested while idle. This keeps a delayed response from needlessly
  disconnecting the station during a browser disconnect/reconnect.
- No observation goes to D1 or Durable Object storage. Policy changes persist;
  per-frame order and ACK updates use socket attachments. Each rate-limit authority
  has one counter row, not a new row per minute of continuous use.
- Local disable cancels the outbound socket. Station/device revocation updates D1
  generations and the active Durable Object. If propagation fails, the request
  fails visibly and existing browser leases still expire within sixty seconds.
  Re-enabling a trial or signing in again does not restore revoked device trust.

For rollback, revoke pilot stations and trials first. Confirm observer retirement,
then withdraw the staging route or restore a previously verified Worker version.
Keep the authority database and audit record of the change; do not delete QSO or
application settings. A desktop build without Remote continues normal local use.

Before a shared pilot, exercise real Auth0 sign-in/sign-out/recovery, OS vault
failure and deletion on supported platforms, another-network station reconnect,
mobile Safari/Chrome suspension, active revocation, hardware measurement aging,
and desktop/app/service restarts. Test simultaneous observers and idle operation
while inspecting billing metrics. Payment and attended RF/amp control require their
own reviewed behavior and operating tests.

Nexus remains GPL-3.0-only. See root `NOTICE`, `COPYING`, and
`licenses/remote/THIRD-PARTY.txt`; the hosted build emits `remote-licenses.txt`.
