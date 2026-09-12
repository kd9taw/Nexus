# Nexus Remote

Nexus Remote puts your station's screen in a browser. The shack PC keeps the radio
and stays the authority; the browser sends gestures and shows what the station
reports back. The service in the middle carries the picture and the commands and
nothing else — the log, the settings and the radio never leave your computer, and
if the service is down, nothing at the station changes.

Everything in this directory is the hosted half: a Cloudflare Worker, a Durable
Object that relays one station to its approved browsers, and a D1 database holding
accounts, stations and entitlement. The browser application itself is built from
`ui/src/remote-web`; the native side that opens the outbound connection lives in
`src-tauri/src/remote_service` and `crates/`.

## The trust model

Read this part before changing anything here.

- **The station is the authority.** Nexus decides what is legal. The browser asks;
  it never commands the radio directly, and the service never originates anything.
- **Pairing finishes at the radio.** A code is generated at the shack and approved
  by a press at the shack. There is no path that pairs a station you are not
  standing at, and that is deliberate rather than unfinished.
- **Every grant is separate and re-armed.** Approving one browser approves only
  that browser. Control and transmit are distinct permissions, and they do not
  survive a Nexus restart.
- **A local operator always wins.** Anyone at the shack takes the radio back.
- **The native path cannot be forged from a page.** `native()` in `authority.ts`
  refuses any request carrying an `Origin` header, so a browser cannot reach the
  station endpoints whatever credential it holds.
- **No observation is persisted.** Frames live in socket attachments and memory;
  nothing about what your radio is doing reaches D1 or Durable Object storage.

## Identity and entitlement

The browser signs in with Auth0 (Authorization Code + PKCE, public client, no
secret). The Worker validates RS256 against a pinned issuer, API audience and SPA
client, then maps `(issuer, subject)` to an internal account ID.

Entitlement is a fourteen-day trial, and it starts when a station is **approved at
the radio** — not at sign-in, because an account with no paired station cannot use
the service and would otherwise burn its trial waiting. The clock is written in the
same D1 batch that creates the station, with `ON CONFLICT(account_id) DO NOTHING`:
that is both the concurrency argument and "one trial, ever" expressed in SQL, so a
second station never restarts the first.

`trial()` reports four distinct states — `none`, `active`, `ended`, `disabled` —
because `enabled` plus an expiry cannot tell them apart and they are different
sentences to an operator. Two gates consume them: `requireEligible()` admits
pairing while a trial runs **or** has never started, and `requireTrial()` guards
`device`, `ticket` and `renew` — that is its only call site. Observation is gated
separately by `observerDeadline` in the relay, which also expires a session already
in flight, so neither function is the whole entitlement story on its own.

`trials.started_at` is null for rows written before migration `0002`, and is never
backfilled — `expires_at` minus fourteen days would invent a start date, and an
invented one cannot afterwards be told from a real one.

## Bounds

- Two enabled stations per account; eight non-expired browser enrollments per
  station; four simultaneous observers per station.
- Pairing codes and pending browser approvals expire after ten minutes. Approved
  browser cookies last thirty days. One-use WebSocket tickets expire after fifteen
  seconds. Credentials and tickets are stored as SHA-256 digests.
- Browser sessions revalidate account, device and trial authority every thirty
  seconds with a sixty-second maximum lease. ACKs and pings cannot renew authority,
  and expiry does not depend on a client sending anything.
- A station emits at most one sample per 500 ms tick while watched. The service
  allows three seconds for a sample and five for an observer ACK, coalesces
  publications and keeps only the latest frame. Hibernation discards that frame and
  restores bounded authority, order and ACK state from socket attachments.
- Revocation updates D1 generations and the live Durable Object. If that fails the
  request fails visibly, and existing leases still expire within sixty seconds.
  Re-enabling a trial or signing in again does not restore revoked device trust.

## Verifying a change

Node 24, plus the Linux dependencies in `.github/workflows/ci.yml` — that workflow
is the command source, and `scripts/gates` derives from it. From the repo root:

```sh
npm --prefix ui ci
npm --prefix remote ci
npm --prefix ui run build:remote
WRANGLER_SEND_METRICS=false npm --prefix remote run types
npm --prefix remote run check
WRANGLER_SEND_METRICS=false npm --prefix remote run build
npm --prefix remote test
npm --prefix remote run test:staging
npm --prefix remote run test:native
npm --prefix remote run test:browser
```

What these actually cover, and what they do not:

- `build` is a Wrangler **dry run**, never a deployment. The committed config has an
  unconfigured identity provider and an all-zero D1 ID.
- The service tests run real workerd, real D1 SQL, hibernation and TCP WebSockets,
  with signing keys made in memory. `remote/test/runtime.mjs` applies **every**
  migration in order — never a named one, because pinning it to `0001` once meant a
  new migration silently did not exist in the test database.
- **D1 `batch()` transactional behaviour is unproven on hosted D1.** Every test here
  runs local Miniflare. Code that needs atomicity should read its rows back and say
  so, rather than trusting a rollback nobody has demonstrated.
- `test:native` builds and runs an ignored Rust probe whose vault and radio readings
  exist only under `cfg(test)`. It does not test an OS keychain, a real radio, an
  Auth0 tenant or another network.
- `test:browser` drives installed Chrome (`CHROME_BIN` overrides) against a
  simulated issuer that really verifies PKCE. Only the provider is intercepted; the
  browser reaches real local Worker APIs, assets and WebSockets. It is slow.
- `remote/` has its own `package.json` and needs its own `npm ci`; `tsc` is not on
  the path there otherwise.

A known local workerd quirk: when HTTP revokes both sockets before either client
has sent an application message, close frames arrive promptly but TCP teardown can
wait for the `ws` client's thirty-second timeout. A minimal service without Nexus
reproduces it. The tests require immediate authority retirement and entry into
CLOSING, then eventual close; the browser hides readings whenever its socket is not
OPEN. Measure real close/reconnect latency on staging before accepting it.

## Deploying

Staging administration — the Auth0 tenant, Cloudflare resources, artifact checks
and rollback — is in [STAGING.md](STAGING.md). Deployment is a `workflow_dispatch`
on `.github/workflows/remote-staging.yml` with one of `inspect`, `provision`,
`deploy` or `recover`.

`deploy` checks the public identity configuration, verifies the exact source is
anonymously reachable, runs the gates above, packages the tested bytes with a source
receipt, validates the upload without deploying, **applies D1 migrations**, uploads,
and then verifies the live Worker and its admission refusals. Schema and code ship
together or not at all. Nothing else — no test, no build — performs any of it.

Account credentials come through normal provider tooling, never this repository and
never the browser configuration. Auth responses, cookies, tickets, provider claims
and observations must not reach logs or tracing; observability is off in the
committed configuration.

For rollback: revoke pilot stations and trials first, confirm observer retirement,
then withdraw the route or restore a previously verified Worker version. Keep the
authority database and the audit record. A desktop build without Remote goes on
working locally.

## Before opening it up

Real Auth0 sign-in, sign-out and recovery; OS vault failure and deletion on every
supported platform; a station reconnecting from another network; mobile Safari and
Chrome suspension; active revocation mid-session; desktop, app and service restarts;
simultaneous observers and idle operation with billing metrics in view.

Nexus remains GPL-3.0-only. See the root `NOTICE`, `COPYING` and
`licenses/remote/THIRD-PARTY.txt`; the hosted build emits `remote-licenses.txt`.
