# Nexus Remote browser pilot

This service connects an approved browser to an outbound Nexus desktop connection.
It displays radio and SPE/KPA amplifier observations and provides an observer
preview of the existing Nexus Operate workspace. Remote operating commands,
receive audio, QSO writes, payment collection and a native mobile application
are not implemented in this pilot.

The browser uses Auth0 Authorization Code + PKCE. The Worker validates RS256 tokens
against a pinned issuer, API audience and SPA client, then maps `(issuer, subject)`
to an internal account ID. An account needs a manually enabled trial. Pairing and
browser approval are separate local actions in Nexus. Clearing browser cookies
requires approval again. A login or account recovery cannot approve a browser.

The desktop stores its Remote pairing and credential in one atomic OS credential
entry under `org.hamradiotools.nexus.remote`. The application preview shares a
closed projection of operating settings; connector credentials and full profiles
are excluded. Enrollment proofs remain in memory. Pairing persists,
but observation starts disabled after every desktop restart. No inbound shack
port or router forwarding is required. The desktop is pinned to
`https://remote-staging.hamradiotools.io` for this pilot.

## Existing Nexus workspace

**Open Nexus** loads the same `ui/src/App.tsx`, Operate, CW, Phone, RTTY, PSK, Needed, Spots and Logbook components as the
desktop. The explicit adapter below `api.ts` owns one authenticated station
session. It does not install a Tauri global or forward arbitrary command names.
Native use still selects Tauri, and the existing LAN TV transport remains separate.

The native connection retains protocol version 1 for older services and advertises
the version 2 stream extension separately. The public service configuration and
browser hello negotiate the supported version. Version 1 keeps its original five
reads: `get_snapshot`, `get_settings`, `get_band_plan`, `get_spectrum_row` and
`get_meters`. Version 2 adds the passive `get_scope_snapshot` and `get_cw_state`.
Version 3 adds `get_remote_page`, negotiated only when the station also advertises
`x-nexus-application-query-version: 1`. Its collection channel is independent of
the unchanged seven stream topics. Version 4 adds `get_remote_recall` when the
station also advertises `x-nexus-application-recall-version: 1`. The original six
collections remain closed in version 3; versions 4 and later accept `recall`.
Version 5 adds the passive `get_rtty_state` and `get_psk_state` stream topics when
the station also advertises `x-nexus-application-keyboard-version: 1`. It retains
version 4's collection grammar. Each browser keeps its negotiated topic set;
older browsers and stations still negotiate versions 1/2/3/4.
Version 6 adds `get_remote_insights` when the station also advertises
`x-nexus-application-insights-version: 1`, alongside every preceding extension.
It adds only the argument-free `awards` and `statistics` collections. The nine
stream topics remain unchanged, and each older version retains its vocabulary.
An older pilot keeps its existing FT observation; an older monitor-only installer
reports the workspace update requirement without losing compact observation.

In version 2, existing panel polling renews local interest. Interest changes cross
the socket; the room combines the interests of approved browsers into one native
watch. The station batches due topics at 100 ms (spectra), 200 ms (meters/CW/RTTY/PSK),
500 ms (snapshot) and 1,000 ms (settings/band plan). Interest expires after 2,500 ms
without a consumer read. No interested browsers means no native application data.
Top-level deltas require an exact acknowledged base; a joining observer receives a
full value. The legacy contract retains one outstanding read/result per browser.

Each native batch spends one room-issued credit; each browser frame spends one
browser-issued credit. The next credit acknowledges the previous response exactly
once. Each receiver counts its request's full round trip toward measurement age,
so delayed packets cannot appear fresh by arrival time or clock synchronization.
Batches are bounded to 768 KiB, seven topics (nine from version 5) and three seconds. Stale sessions hide
station readings and close portaled menus/dialogs. Unavailable scope/decoder data clears
that readout independently. Session changes discard cached readings and late results.

The shack's engine owns the snapshot, logbook and hardware. Remote spectrum
observation leaves the desktop averaging window and analysis requests intact. The
CW getter observes decoder state without changing sensitivity. RTTY/PSK getters
copy the existing native DTOs and 4,000-character text rings, including character
confidence, AFC and reported TX state. These sampled rings are not lossless text
history. Browsing never arms a decoder, clears text, changes tuning or sends text;
start the decoder locally in Nexus. A stopped decoder and unavailable data have
separate messages. Reported TX activity does not confirm RF output. Encoding runs outside
the engine lock, and busy engine reads are refused without queuing. Room socket
attachments contain only bounded authority and routing metadata, never these
application values or contact history. After hibernation, a new watch epoch requests
full samples; old epochs cannot refresh a restored session. A slow or failed browser is retired
independently of the station and other approved observers.

Collection queries address only decodes, needs, spots, logbook, DXCC locations,
feed health, exact-call recall and full-log award/statistics summaries. Pages contain at most 128 rows and 256 KiB. Each browser has one
outstanding query/result, a three-second deadline and a 16-request/second ceiling;
the room also caps the aggregate at 32/second using persisted per-browser counters.
The station runs one collection worker separately from the live socket loop.
Sealed snapshots expire after 60 seconds; their cursors bind collection, search,
filter and offset. An expired cursor requires a fresh snapshot. At most eight
captures and 16 MiB of encoded collection payload are retained at the station;
object overhead is additional. The cloud retains routing metadata only.

Logbook search covers the station's in-memory log, retaining the newest 2,000
matches before the byte cap. The existing table shows page and match counts and
identifies the retained window. Needs/spots retain up to 3,000 rows, with visible
counts when capped. Award/privilege/source calculations are shared with native
Nexus. Needs scoring runs after releasing the engine lock. Reads do not invoke
shared-log recovery, write QSOs, import, confirm, export or upload contacts.

Cockpit recall returns the newest 20 exact-call contacts, full-log worked and
confirmation counts, band/mode and entity context, and the newest nonempty note.
It copies at most 128 contacts per engine-lock acquisition and computes DXCC
context after releasing the lock. A retained opaque log token detects mutation
or replacement between chunks, including edits that leave the count unchanged;
an inconsistent or over-budget read is refused. The read has a two-second total
deadline and bounded summaries, and never reports a truncated summary as a new
entity/band/mode. Recall bypasses capture reuse: selection or **Refresh** reads
the current log. It does not poll. Names, locations and notes come from prior
contacts; this is not a station callbook lookup.

Official Awards and Statistics use the complete station log, independently of the
Logbook's 2,000-contact display window. Each summary copies only relevant fields,
128 records per lock acquisition, and refuses a changed log or station identity.
Award and geographic calculations reuse the native folds; descriptive statistics
match the existing desktop roll-up and browser label ordering. The bounded query
worker allows two seconds, one million rows, 256 bytes per relevant text field,
32 MiB of cumulative copied text, 100,000 distinct calls and 2,048 tally groups.
These are refusal limits, not commercial capacity claims; heap overhead is additional.
An indivisible summary contains no contact rows and at most 128 KiB of metadata.
The existing views display capture age, remove expired results after 60 seconds,
and offer **Refresh summary**. Reads occur on entry, refresh and station recovery,
without background summary polling. Unavailable or inconsistent results clear the
old totals. Journey, confirmation diagnostics and uploads are not connected.

The independent decode display journal retains up to 3,000 rows from the current
Remote connection, with a unique session/context identifier and monotonic sequence.
It samples existing snapshot publications; it is not an offline or lossless decode
archive. Queries fetch new/updated rows when snapshot decode context changes.
Reconnect, band/tier context changes and retention limits are explicit in the UI.
It does not access or change the FT control history or start decoding/DSP work.

Operate, CW, Phone, RTTY and PSK have partial observation support in this preview. CW includes
the decoder transcript, sent-text history and callsign candidates. CW/Phone share
the station scope, meters, settings, amplifier status and observed band activity.
RTTY/PSK retain their existing cockpit, waterfall, decoder transcript and amplifier
strip, with station-local decoder and transmit controls disabled.
Needed, Spots and the paged Logbook now support read-only browsing. Existing QRZ
profile links open in the browser, without contacting the station or its vault.
FT selection and CW/Phone/RTTY/PSK callsign entry use the existing Nexus recall card;
its contact rows open the existing filtered Logbook. Stale sessions and changed
callsigns discard old results. QSO entry, memories, rotator and voice-keyer/audio data remain
unavailable and are identified in their existing panes. Other navigation destinations display
their availability limit, and the full Settings panel is not mounted with partial
settings. Station controls, including the existing amplifier controls, are
disabled; all mutation names are also refused by the transport and native parser.
Opening a workspace or navigating it never selects the shack's operating mode.
The original **Observe station** view remains available.

This read adapter is an incremental integration boundary, not paid-service
completion. Before enabling remote operation, the protocol needs explicit station
control leases, expiring and deduplicated commands, native authorization at
execution, and attended transmitter/amplifier acceptance. Full read parity also
needs additional history sources and remaining per-feature data contracts. Shared
subscriptions require measured load and WAN acceptance before commercial capacity
claims. Audio needs its own negotiated media path. Billing must
materialize service entitlement separately from browser trust and station control;
payment or account recovery must never arm a radio.

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

Miniflare's development dependency `sharp` is overridden to the patched 0.35.4 for
[GHSA-rgj7-g3m4-5g8c](https://github.com/advisories/GHSA-rgj7-g3m4-5g8c).
Remove the override when the pinned upstream toolchain requires a patched version.

A local workerd limitation is covered explicitly: when HTTP revokes both sockets
before either client has sent an application message, both close frames arrive
promptly but TCP teardown can wait for the `ws` client's 30-second close timeout.
A minimal service without Nexus reproduces it. Tests require immediate authority
retirement and entry into CLOSING, then require eventual close as well. The browser
source hides readings whenever its socket is no longer OPEN. Verify the actual
browser close/reconnect latency on the deployed staging endpoint before acceptance.

## Staging setup

The [staging administration guide](STAGING.md) covers the new Auth0 tenant, existing
Cloudflare account references, manual deployment workflow, artifact checks and rollback.

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
Finally the user enables observation locally and selects **Open Nexus** or
**Observe station** in the browser. The initial workspace acceptance run uses a
desktop browser; the compact observation view also supports mobile web.

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
