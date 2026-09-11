# Nexus Remote browser pilot

The compact session bar keeps control ownership, Release and unresolved command
recovery above the existing Nexus workspace. **Remote access** opens connection
help and Disconnect. Expanding those details leaves the cockpit and current
authority intact; connection recovery still requires a new explicit acquisition.

This service connects an approved browser to an outbound Nexus desktop connection.
It reuses the existing Nexus application for station observation and adds
separately authorized manual general-log QSO entry, receiver/amplifier controls
and bounded frequency, section, decoder and configured-radio changes.
Transmitter commands, receive/browser microphone audio, payment collection and a native mobile
application remain outside the implemented pilot. A compatible station build is
required for each negotiated feature; browser deployment does not upgrade Nexus
at the shack.

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

Workspace entry prepares each native section and decoder QSY in order, including
intermediate profile banking. Only the final radio is configured by the selection worker. The commit shares native section, area, session and tier verbs
under the stable source lock and existing A7 guard; it grants no transmit access.
A routed entry that ends back at its original radio uses the same physical
connection and capture stream while committing the intermediate native profile
changes. Unconfirmed writes never become local retries. Hardware and WAN
acceptance remain required.

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
Version 7 adds only `get_remote_dxpeditions` and the argument-free `dxpeditions`
collection, requiring `x-nexus-application-dxpeditions-version: 1` and all preceding
advertisements. It preserves every v1–v6 vocabulary, including mixed browsers and
hibernation recovery.
Version 8 adds only `get_remote_memories` and the argument-free `memories`
collection, requiring `x-nexus-application-memories-version: 1` and every preceding
advertisement. Versions 1–7 retain their exact vocabularies.
Version 9 adds only `get_remote_ota` and the argument-free `ota` collection,
requiring `x-nexus-application-ota-version: 1` and every preceding advertisement.
Versions 1–8 retain their exact vocabularies.
Version 10 adds only `get_remote_field_day` and the argument-free `fieldDay`
collection, requiring `x-nexus-application-field-day-version: 1` and every preceding
advertisement. Application versions 5 through 10 use the version 5 stream.
Version 11 adds `get_js8_state` and the argument-free `get_remote_js8_context`
(`js8Context` collection), requiring `x-nexus-application-js8-version: 1` and every
preceding advertisement. Its version 11 stream has ten topics. Versions 1–10
retain their exact vocabularies, including across hibernation.
Version 12 adds the closed SSTV/APRS state and authenticated image/roster reads,
requiring `x-nexus-application-station-modes-version: 1`. Version 13 adds
`get_remote_navigation` for Connect, Path and Satellites, requiring
`x-nexus-application-navigation-version: 1`. Version 14 adds
`get_remote_configuration` for Settings and saved Radio Programming, requiring
`x-nexus-application-configuration-version: 1`. Each version preserves the earlier
vocabularies. Navigation/configuration documents are capped at 2 MiB, chunked and
paged under a station-issued capture identity, with explicit source expiry.

The full existing Settings view uses an explicit native projection that withholds
secrets and private paths. Its startup effects and individual write controls stay
guarded. Programming reads the saved channel bank; it does not read/write a radio,
run a shell command or expose arbitrary files. SSTV images are opaque authenticated
PNG/BMP resources, and APRS retains its native map and independent RF/Internet health.

An older pilot keeps its existing FT observation; an older monitor-only installer
reports the workspace update requirement without losing compact observation.

In version 2, existing panel polling renews local interest. Interest changes cross
the socket; the room combines the interests of approved browsers into one native
watch. The station batches due topics at 100 ms (spectra), 200 ms (meters/CW/RTTY/PSK),
500 ms (snapshot/JS8) and 1,000 ms (settings/band plan). Interest expires after 2,500 ms
without a consumer read. No interested browsers means no application values cross
the station socket.
Top-level deltas require an exact acknowledged base; a joining observer receives a
full value. The legacy contract retains one outstanding read/result per browser.

Each native batch spends one room-issued credit; each browser frame spends one
browser-issued credit. The next credit acknowledges the previous response exactly
once. Each receiver counts its request's full round trip toward measurement age,
so delayed packets cannot appear fresh by arrival time or clock synchronization.
Batches are bounded to 768 KiB, seven topics (nine at version 5, ten at version 11)
and three seconds. Stale sessions hide
station readings and close portaled menus/dialogs. Unavailable scope/decoder data clears
that readout independently. Session changes discard cached readings and late results.

The shack's engine owns the snapshot, logbook and hardware. Remote spectrum
observation leaves the desktop averaging window and analysis requests intact. The
CW getter observes decoder state without changing sensitivity. RTTY/PSK getters
copy the existing native DTOs and 4,000-character text rings, including character
confidence, AFC and reported TX state. These sampled rings are not lossless text
history. Browsing never arms a decoder, clears text, changes tuning or sends text.
Explicit receiver gestures require the separate station-control grant below.
A stopped decoder and unavailable data have
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
strip. Supported decoder controls require a station-control grant; transmit
controls remain disabled.
Needed, Spots and the paged Logbook now support read-only browsing. Existing QRZ
profile links open in the browser, without contacting the station or its vault.
DXpeditions reuses the existing station board, work-now cards, calendar and
per-band heatmaps. Its reader only projects the station's cached board and matching
seven-day prediction cache; it never fetches a feed or starts a predictor. The base
cache records callsign, grid and log revision; changed context or data at least five
minutes old is unavailable until the native producer refreshes. Lock acquisition
is nonblocking. Each board list has at most 256 entries, outlooks at most 16 bands
of 24 hours, text at most 1,024 bytes (website 2,048), and serialized projection at
most 192 KiB. Excess refuses the whole board. The forecast reader examines at most
32 native cache entries and uses the same key as the producer: UTC day, horizon,
grid, model, power/gain, rounded measured SSN and sorted target calls.
Missing/busy forecasts remain explicitly unavailable. Available forecasts carry
their age and remaining six-hour/UTC-day lifetime. The browser also expires a read
after 60 seconds, clears it on loss/failure, and offers explicit Refresh. Work, map
routing, popout, Chase and alarms are unavailable remotely; website gestures open
the existing HTTP(S)/QRZ destination in the user's browser. A compatible station
build is required. Automated synthetic-data checks do not establish DSP/WAN load
capacity or live expedition/shack acceptance.

Memories reuses the actual channel manager's list/grid, groups, favorites, nets
and search. The installed main WebView, after loading its existing durable store,
publishes its already loaded canonical bank into an ephemeral native cache only while
local observation is enabled. A five-second local check coalesces changes and
refreshes an unchanged cache every 20 seconds. The native-only publication command
requires the main window and current local enable generation. Disable clears the
cache; delayed publications from a previous enable cannot revive it. No browser
query or publisher initiates a file read, UI-state migration or durable-store write.
An uninitialized bank is unavailable until its native owner loads it normally.

The closed bank admits at most 512 channels, 64 groups, 64 group references per
channel, seven net days, 1,024 bytes per text field and 128 KiB of serialized JSON.
Unknown fields, duplicate IDs and oversized/invalid banks refuse the whole read.
Native and browser readers independently expire the publication after 60 seconds;
the browser includes transit/capture age and hides failed or lost data. Refresh
preserves browser filtering and list/grid choice. The browser never adopts the
station bank into its own memory store. Tuning/recall, edits, reorder, imports,
exports, starter packs and net alarms remain station-local. Full entry details,
write round trips, hardware tuning and live station comparison remain separate.

POTA/SOTA reuses the existing hunter cards, program/band/mode filters and sort.
The read projects the station's existing per-program spot caches, activation and
hunt context, complete-log activation count, park directory/import counts, and
canonical worked-park and own-call open-band badges. It triggers no feed fetch,
prediction, file operation, Hunt/QSY or activation change. The native POTA/SOTA
feed remains separate from the cluster/RBN unassisted switch, as on the desktop.

Each feed admits at most 512 spots, 1,024 bytes per text field and 128 KiB of raw
text; the complete source is limited to 192 KiB. Feed data expires after 15 minutes,
independently for POTA and SOTA. Missing caches remain unavailable until the
station's normal feed owner loads them. An empty successful feed is distinct from
an unavailable or expired feed. Activation counting uses bounded log chunks with
profile, activation and log-change checks. The browser includes capture/transit
age, expires the whole observation after 60 seconds, and clears it on station
loss or failed refresh while retaining local filters. A compatible station build
is required; station commands, file operations and live shack comparison remain
separate work.

Field Day reuses the existing event dashboard, score, worked sections, earned and
planned bonuses, complete event log and club board. The v10 extension requires
every prior extension plus `x-nexus-application-field-day-version: 1`. Its closed,
argument-free `get_remote_field_day` query captures the native display and its
four scoring/operator settings under one nonblocking engine lock. The normal
desktop snapshot uses the same display constructor; rule and score calculations
remain native. The master switch is never enabled by the browser or event date.

The source admits at most 2,048 event contacts, 4,096 club duplicate keys, 128 club
positions, 1,024 UTF-8 bytes per text field, 128 KiB of status text and 224 KiB for
the whole query. Excess returns unavailable explicitly; it never presents a
truncated log as complete. Capture/transit age expires after 60 seconds and ages
club position readings. Refresh, source failure and station loss clear old data;
the local bonus disclosure survives refresh. Older pilots show the availability
message and do not send the query. Navigation never changes the station mode.
Event setup, QSO/scoring writes, operating, exports and separate club-board windows
remain station-local. Larger-event pagination and live shack comparison remain
future verification and parity work; a compatible station build is required.

Tempo reuses the native Fast/Deep conversation workspace, recent chats, heard
stations, composer, waterfall and radio header. Existing snapshots already carry
complete conversations and native delivery fields, so this view works with every
application observation version without a new query or station producer. Held,
sending, confirmed, delivered, no-ACK, abandoned, partial and legacy messages keep
their native meaning; the browser does not infer new acknowledgement semantics.

Thread/band selection, roster filtering and history scrolling are browser-local.
Arriving messages preserve the native follow-newest behavior and leave an operator
reading older history in place. The existing 768 KiB whole-snapshot ceiling refuses
oversized sources rather than silently shortening conversations. Station loss
hides old values. Compose/send, resend, Work/double-click, archive, CQ,
heartbeat, Roam and frequency-memory shortcuts are guarded in the actual UI as
well as refused by the transport. Larger-history capacity and live station
comparison remain outstanding; observation does not enable operation.

JS8 uses the native cockpit, four-speed activity and roster, inbox, queued frames,
and pending-reply/HB/CQ indicators. Observer navigation never enters JS8 mode.
The state copy is bounded before cloning: 200 activity rows, 500 heard stations,
100 inbox messages, 2,048 remaining queued frames, 1,024 UTF-8 bytes per text field,
192 KiB of display text and 384 KiB for the complete sample. The original queue,
decoder and transmit timing are unchanged. Relative ages and countdowns account
for the station clock and conservative transport age without changing native UTC
timestamps or row identities. Samples expire after three seconds.

The shared roster context joins heard calls against the complete log in bounded
lock sections and reuses an unchanged log generation. It preserves native latest
contact semantics, including empty fields, and distinguishes confirmed unworked
calls from unavailable history. It refuses changed generations, more than one
million QSOs or a context exceeding 192 KiB. History and the native licensed band
plan expire after 60 seconds; Refresh and source loss clear old context. Callsign
selection, pinning and recall stay local to the browser. Inbox changes, queue
changes, sending, tuning and every transmit latch remain station-local. A
compatible station build and attended comparison are required for JS8 parity.

FT selection and CW/Phone/RTTY/PSK callsign entry use the existing Nexus recall card;
its contact rows open the existing filtered Logbook. Stale sessions and changed
callsigns discard old results. Manual QSO entry uses the separate operation contract
below. Cockpit memory recall, rotator, voice-keyer/audio and other hardware controls
remain unavailable in their existing panes. The read transport continues to refuse
all mutation names; the operation parser admits only its closed, versioned grammar.
Opening a workspace or navigating it never selects the shack's operating mode.
The original **Observe station** view remains available.

This adapter is an incremental integration boundary, not paid-service completion.
Logging authority does not authorize station controls. The amplifier worker
requires generation-bound execution, cancellation and fresh readback. Attended
radio/amplifier acceptance, remaining tuning gestures and transmitter operation remain incomplete. Full read parity also
needs additional history sources and remaining per-feature data contracts. Shared
subscriptions require measured load and WAN acceptance before commercial capacity
claims. Audio needs its own negotiated media path. Billing must
materialize service entitlement separately from browser trust and station control;
payment or account recovery must never arm a radio.

## Manual general-log operations

Operation version 1 remains supported; version 2 is advertised independently of
the application read version. An approved browser needs a separate local logging grant
and explicitly acquires the station's single logging lease. Grants start empty
after restart. The station owns the boot identity, connection/lease generations,
1-second heartbeat, 5-second lease, 2-second command window, context revision,
sequence and bounded result history. Local takeover, observation disable and
revocation invalidate authority; reconnect never reacquires it.

The hosted application starts with the full Nexus interface. The session Info
panel offers an optional Quick Operate presentation on the same mounted App.
Its Operate, Hunt and Log destinations preserve the current Phone/CW contact draft;
Full Nexus returns to the original navigation. Phone/CW put the general-log form
first, with radio detail expandable and amplifier state visible. Enlarged phone
layouts use one main contact scrollport, while prior-contact history retains its
bounded list. This presentation neither grants station control nor adds remote
transmission. Other operating modes keep their existing layouts; keyboard/locale,
media and complete mobile acceptance remain open.

CW, Phone/SSB, RTTY, PSK/QPSK and JS8 reuse the existing manual LogEntry form. The
station chooses the normal QSO time at append; an explicit UTC override is retained.
Field Day and QSO WAV recording configurations refuse this manual-form path.
Editing/import/export and lookup remain separate work. Global UI control permission stays false;
individual receiver/amplifier affordances use the narrow capability grants.

Manual append reuses native enrichment, deduplication and connector queues. The
actual open file is synchronized after releasing the engine lock. `fileSynced`
confirms local persistence, not external delivery. Failed append/sync and a lost
reply can be uncertain; no automatic retransmission occurs. The station retains
at most 1,024 receipts for ten minutes and refuses old sequences after expiry.

Operation v4 connects the existing Log QSO button and confirmation dialog through
the separate `qsoLogging` capability. It needs logging permission and the current
controller lease; it does not require or grant transmit permission. Current-QSO
requests bind the displayed contact identity, tier and exchange. Confirmation and
discard bind the exact pending contact, including replacement by identical fields.
The native current-QSO implementation shares the local button's eligibility,
record construction and write-once transition. Its pending-confirmation path
retains the original contact through append and storage sync; only a successful
sync can authorize clearing that same hold. Replacing even an identical hold
invalidates old completion/discard tokens. Journal preparation syncs outside
Engine and rechecks the hold before publication. Confirmation changes only the
four existing dialog fields, preserving native split frequency and end time;
pending journals now retain those fields across restart and still read legacy
journals. Outcomes distinguish a synchronized log append, synchronized pending
confirmation, discard, no eligible contact and unconfirmed persistence. The
browser waits for a later station snapshot before updating the existing UI;
draft confirmation edits survive temporary permission or observation loss. Its
Stop TX remains available inside the dialog through the independent Stop route.
Full operating acceptance still requires actual station, hardware and WAN checks.

Before submission the browser retains its operation ID and bounded contact fields.
Station-specific Web Locks protect writes, result recovery and explicit dismissal
across tabs. Contention or unavailable locking refuses the gesture; it never queues
a later log action. The original command deadline/context remains binding while
acquiring that lock or waiting briefly for an overlapping heartbeat. Reload can
show the submitted fields and query the receipt without resending. An expired
receipt requires checking the actual station log before dismissing the saved check.

The cloud validates and routes a closed request grammar, injects authenticated
session/device identity, and retains only bounded routing metadata during
hibernation. It cannot report native application or persistence success. It does
not store QSO fields in D1. Logging permission is not an end-to-end TX authorization
claim; hosted-origin compromise, operator permits, media, hardware cancellation
and commercial acceptance require their own review before a paid operating release.

## Receiver and amplifier operations

With the `ampFollowBand` capability on operation v3, Settings → Radio → Amplifier
uses the existing follow-band checkbox and Save button. Only that boolean is
submitted, with the displayed radio, prior choice and exact public Settings
revision. The station owns the settings path and preserves other fields/profiles.
Newer local settings refuse an old browser form. A successful atomic native save
returns `settingsSaved`; a failed save leaves the prior choice and returns
`persistenceFailed`. Neither means that the amplifier has changed band or that RF
is off. Enabling requires the existing fresh idle/disarmed hardware checks;
disabling can be saved with unknown hardware state. Disconnect and lease expiry
leave the saved preference intact. Retained receipts prevent a duplicate or lost
response from replaying the save.

The full browser reuses the original observation stream for amplifier readings,
follow state and actual connection/read identities. Manual Remote band steps are
disabled while follow is on. Settings refreshes retain an unchanged draft only
under its original revision and freshness deadline. Native Settings captures
refresh immediately after the projected configuration changes. A failed amplifier
write retires its exact observation connection and clears both displays; late
replies cannot replace a newer connection. Physical SPE/KPA and WAN/device
acceptance remain required before a complete-operation claim.

`x-nexus-operation-version: 2` adds a closed `stationControl` request. A browser
needs approval, an explicit per-browser **Allow station controls** grant at the
shack, and the shared controller lease. Grants reset after a station restart.
Logging permission and payment never grant these controls. Older stations retain
their logging/observation contract and refuse station-control requests.

The existing RTTY, PSK, SSTV and APRS monitor buttons operate native receivers.
CW/RTTY/PSK Clear, RTTY/PSK AFC reset and waterfall netting, and PSK mode/reverse
use the same Engine verbs as local gestures. APRS Remote monitoring is receive
only and cannot upgrade the automatic-ack interlock. Navigation does not arm a
receiver or change the station's operating mode.

The existing amplifier Operate/Standby and band-step buttons submit one intent
with the displayed precondition. The port owner needs a newer sample from the
same amplifier connection, fresh unkeyed radio PTT, an idle station and TX
disabled. Manual band steps also require follow-band disabled. Unsupported band
endpoints refuse; the shared SPE 15K model token cannot establish 4 m capability
across hardware series. No wrap-around is inferred. Standby never means Stop TX.

The shared native button/follow worker also rechecks the completed poll's opaque
connection before writing. Changing a profile/port during serial I/O retires its
old reply; follow uses the current saved setting and band. A local button takes
precedence over an automatic step. Native KPA writes require a current CAT idle
reading because KPA supplies no transmit flag; SPE can use its own idle flag,
while observed physical radio keying always vetoes a change. Write permission
expires with the supporting readings and serial I/O never holds Engine. These
guards do not prove an RF
interlock against keying that has not been observed. Attended bench verification
remains required before release.

Each hardware intent has a fixed deadline bounded by the five-second lease,
independent of later heartbeats. Native context changes and revocation reach the
worker after it releases the Engine lock. A later measurement on the same link
confirms application; accepting a queue entry or a wire acknowledgement does not.
Results are pending, applied, rejected or unknown. The browser retains uncertain
intent across reload, shares its Web Lock with manual logging, and checks the
receipt without replaying the action. Local takeover clears both permissions.
Physical SPE/KPA command and cancellation acceptance remains required.

## Frequency and mode operations

Operation v3 adds frequency, mode and tier capabilities. The native header and
public configuration retain `x-nexus-operation-version: 2` / `operationVersion: 2`,
with explicit `x-nexus-operation-max-version: 3` / `operationMaxVersion: 3` for
new peers. Absence preserves the older version. The relay forwards the lower
browser/station version; v1 logging and v2 receiver/amplifier controls remain
usable. Native admission refuses v3 actions through v2, and both station and
relay project capabilities for the negotiated version. Routing checkpoints
retain that version through room hibernation. Bounded unknown capability hints
are ignored, while action names and arguments remain a closed vocabulary.

The `radioSelection` v3 capability connects the existing radio pills and Settings
Make active button to the station's queued `radio.select` action. It requires
local station-control permission, the displayed active-radio connection, an idle
disarmed station and a configured target. The radio owner prepares the incoming
connection, retains native profile choices and confirms its tuning before adoption;
failed or expired work is never retried as a local command. Host rotator profile
synchronization runs outside the radio loop. The browser waits for later snapshot
and Settings samples naming the selected radio, and Settings refreshes its complete
configuration document. Peg-lock and transmission retain separate permissions.
Physical capture, shared-port PTT/keyers, slow CAT and WAN acceptance remain open.

The `frequency` capability admits only `radio.frequency`, separately
from the future `radio` capability. The existing main dial on FT, Phone, CW,
RTTY, PSK and Tempo accepts typed MHz; FT/Tempo channel selects use the same
intent. Other controls retain their own permissions. Opening a cockpit never
changes the station's operating mode. An idle, disarmed station, a current radio
connection and fresh unkeyed PTT are required. The requested named band must
match the native band plan, and the existing routing policy must keep the active
radio (including the operator's peg choice). Held satellite/channel contexts,
pending local tuning/VFO/offset requests and split refuse this increment.
Ordinary FM tuning confirms native repeater configuration within the same request.
Normal RX tuning is not restricted to the operator's TX privileges; transmit
guards remain in the native engine.

The separate `mode` capability admits explicit `radio.mode` requests. “Use this
mode” in the FT, Phone, CW, RTTY, PSK and Tempo headers enters the station's
Digital, Phone, CW, RTTY or Keyboard section using native remembered-frequency
and sideband policy. Tab navigation remains passive. Mode entry also requires
the native source, an idle disarmed station and the same active-radio limits.
Tier/decoder selection uses its own capability and action.

Preparation resolves CAT mode, dial and routing without changing settings or
banking a memory. Canonical commit calls the shared native section-entry verb
with manual arming disabled. The local desktop keeps its existing arming policy.
When entering a capped mode, the transaction requires readable RF power before
the first write, applies any necessary reduction after mode/dial selection, and
confirms the reported power. It carries an already-lower level forward and never
raises power to a ceiling. Unreadable or unconfirmed power leaves the operation
rejected or unknown; it cannot become a deferred power change.

The internal `Rig::remote_retune` consumes one native-resolved request. It checks
the expected CAT dial/mode, fresh unkeyed PTT and simplex, writes mode before frequency,
and reuses the native band/filter policy. A reported band-stack override permits
one correction inside that same command; missing or inconsistent readings stop
the transaction. Permission is rechecked after reads and at every socket write.
An uncertain operation is never retried. Its returned CAT readback leaves the
completion pending: it does not establish committed settings or physical RF.

The existing RadioLoop consumes the intent once, before normal reconciliation,
using its owned CAT connection outside the Engine mutex. It publishes the final
readback, rechecks the original permission and station context, then calls the
native frequency or section setter and saves the current settings under that mutex. It
consumes only its own retune flag and adopts the confirmed position and power, so neither
lease expiry nor a failed save causes a subsequent CAT retry. A save failure
keeps the confirmed live position but reports an unknown outcome; it never
rolls back a whole settings snapshot over local preferences. The browser waits
for a later station sample after an applied receipt. Unknown outcomes require
checking the station, never automatic replay.

Readout digits and arrow keys, CW/Phone scope-wheel input and tuning-strip
nudges share one browser tuning owner. Each short burst captures the original
station authority, rechecks the displayed dial against a fresh station sample,
and submits one frequency intent. Input is consumed on submission; disconnected
or expired work cannot return as a deferred target. Further input requires a
fresh explicit gesture after the command settles.

CW and Phone scope clicks use that same frequency owner and the existing
PhoneScope signal detector, sideband and CW pitch calculations. Pointer-down
captures the displayed observation and original authority; release submits at
most one absolute target. A changed scope mapping, moved pointer, loss of
capture, blur, Escape or unmount cancels the press. A second pointer cannot
replace it. The target is rechecked against a fresh station snapshot before
the native frequency transaction. This click path does not enable native
drag timers or edge scanning, and an uncertain result is never replayed.

The separate operation-v3 `receiverFilter` capability connects the existing CW
and Phone BW steppers using the closed `radio.filterWidth` action. It binds the
displayed cockpit, exact prior width and active radio. The existing radio owner
reads the actual mode, bandwidth, dial, PTT and split state before a single width
write. A later matching readback completes it. The physical mode is preserved,
including a front-panel change that differs from the app's commanded sideband.
No Settings save, native retry slot, frequency write or transmit action is added.
The normal station stream supplies the displayed width; a receipt does not
replace that sample. Local filter gestures retire older remote work.

Expanded-operation version 3 can advertise `receiverDsp` for the existing CW/Phone
NB, NR, automatic-notch and five AGC buttons, plus Phone manual-notch enable.
Closed `radio.function` and `radio.agc` actions bind the displayed cockpit, prior
reading, radio and connection. The existing radio owner checks actual frequency,
mode, DSP state, PTT and split before one permitted CAT write, then confirms the
requested value and unchanged receive position. Missing support or an uncertain
result cannot enter native retry slots. Local function/AGC picks retire pending
Remote work. AGC repicks preserve the desktop's explicit reassert behavior;
confirmed choices update reconciliation state without writing Settings. Browser
selection follows later station samples. NR depth, manual-notch frequency, COMP
and VOX remain separate work. Physical-radio and WAN acceptance remain pending.

The separate operation-v3 `phoneMode` capability connects the existing Phone
AUTO, USB, LSB, FM and AM buttons through `radio.phoneMode`. It binds the displayed
override and actual CAT mode, retains the station's automatic-mode policy and
native AM visibility, and confirms the new mode at the unchanged receive dial.
The native override remains transient; no Settings save or future retune is
queued. An unconfirmed change is not retried. The normal stream supplies the
displayed selection. Stations advertising `fmTuning` also support the existing
FM button, AUTO into FM, and transitions out of FM. The native owner resolves
saved shift, target-frequency offset (including an explicit override) and CTCSS,
probes them before tuning, applies and reads them back under the original request,
then adopts its native reconciliation cache. Missing readback cannot confirm the
choice; uncertain work is not retried, including after later dial polls. A
partially written tune holds automatic dial/mode/repeater reconciliation until
a new explicit native retune or confirmed Remote tune/radio selection. Readings
and unkeying continue while held. The picker remains transient.

Stations advertising `fmReceiver` also accept the existing level, DSP, AGC and
bandwidth controls while the radio reports FM or PKTFM. Bandwidth retains native
visibility: the Phone cockpit hides its SSB width buttons when FM is commanded.
These commands preserve
the current dial, mode and repeater configuration; only bandwidth reissues the
same mode with a new width. The native owner refuses receiver adjustments while
a tune is unconfirmed or its FM configuration is still pending. Hardware readback
is required, and later polls do not retry an unconfirmed adjustment. Older
stations keep FM receiver controls disabled.

Typed frequencies and named CW/Phone band picks also follow the station's saved
radio routing rules. A routed tune uses one native selection transaction: prepare
the incoming radio at the requested dial, confirm both radios, then adopt and save
through the native QSY or band-recall owner. It does not select the incoming
profile's remembered frequency as an intermediate command. Peg-lock and bandless
routing keep their desktop meanings. Failure does not queue a retry or activate
an unconfirmed radio. Host handoff support is required.

Operating-section changes share that handoff when native mode entry recalls a
frequency on another radio. Preparation uses the destination mode's routing and
profile policy; commit runs native mode entry with its original dial banking and
reset order. Entry without a QSY keeps the active radio. A mode power ceiling is
confirmed before adoption, retaining a lower incoming level through later polls.
Changing the section does not arm Remote transmission.

CW and Phone Work spots use the same incoming-radio owner when routing selects
another radio. The exact spot dial is the only tuning target. Native Work owns
mode entry, outgoing memory, override cleanup and the contact/navigation hint;
those effects occur only after confirmed hardware adoption. A canceled or
unconfirmed handoff leaves the original contact context intact. Transient AM
entry retains its native power reduction even when the spot ends in SSB.

Decoder changes also use that owner when the native tier channel resolves to
another radio. Hardware preparation targets the final channel once. Commit
acquires the existing decoder source mutex without waiting, then runs native tier
installation and QSY with the held reset guard. Native decoder labels, channel
fallbacks and offset rules remain authoritative. Busy decoding refuses admission
or final adoption; a missing channel preserves the active radio and frequency.
Same-tier selection retains its existing complete no-op. Radio/profile adoption
is saved; this adds no persisted decoder preference.

Repeater-channel holds and satellite/split transactions,
band-memory/spot shortcuts, scope dragging, continuous scanning and attended hardware/WAN
acceptance remain incomplete. A compatible station build is required; this
source increment does not update existing installations or establish paid readiness.

## Decoder selection

The `tier` capability admits `radio.tier` from the existing FT8/FT4/FT2,
advanced decoder and Tempo Fast/Deep selectors. The station must use the native
digital receiver, remain disarmed and idle, and keep the same radio. Native
`set_tier` owns decoder replacement, decode/roster clearing, JS8 entry/exit,
Field Day submode, MSK144/WSPR offsets and configured periods. Destination
preparation shares native working-frequency overrides, specialty-mode channel
fallback and the 500 Hz no-QSY threshold. LF/MF channels absent from the
arbitrary-dial band table are admitted only as exact native stock channels;
this does not widen arbitrary tuning or TX guards.

Selecting the current tier remains a complete no-op, including an off-default
dial or a busy decoder. A different tier uses the same CAT/readback transaction
as the other radio intents. Decoder installation tries the stable source mutex
before admission and again at commit; it never waits behind a long decode while
holding the Engine lock. If the decoder starts during CAT work, an uncommitted
outcome is unknown and is not replayed. The shared source mutex is retained to
serialize native decoder jobs. Like the local tier verb, this changes live state
without adding a settings-file save. Local armed tier changes retain their
existing behavior; Remote does not alter FT sequencing or grant transmission.

Tier selection alone does not establish complete Tempo/JS8 workspace behavior,
message sending or a complete remote operating mode. Physical radio acceptance
and the remaining operational contracts still apply.

## Digital workspace entry

The distinct operation-v3 `workspace` capability admits only `radio.workspace`
with `ft`, `tempo` or `js8`. The existing “Use this mode” button enters that
workspace; mounting or revisiting a browser tab never sends an operating command.
The UI binds the displayed active radio and observation connection, requires
fresh known-idle PTT and leaves the native snapshot as the source of mode state.
An applied receipt does not invent a new decoder snapshot. Older peers without
the workspace capability remain passive.

The station resolves its existing digital CAT policy, FT/Tempo area memories,
working-frequency overrides, JS8 session entry and power ceiling before any
mutation. FT/JS8 entry from a manual section reuses native section frequency
memory/home policy even when the digital tier is unchanged. Tempo retains its
own section-frequency behavior. A same-tier digital return preserves an
operator-tuned dial; changed tiers use the native channel/fallback rules.

The existing RadioLoop owns the CAT transaction and final readback. Admission
and commit both try the stable decoder mutex; contention refuses without
waiting under the Engine lock. The shared native mode/area helpers perform
decoder replacement and HARQ reset under that same held guard at commit.
Local callers retain their normal lock acquisition and all original transition
effects. A valid local operating-spec gesture retires pending remote work even
when its label and CAT context are unchanged; an invalid spec changes nothing.

The host saves current Settings after the confirmed native transition, never a
browser-supplied settings object. Missing/changed authority, hardware context,
readback or required power reduction cannot claim success. A save failure
keeps confirmed live state with an uncertain receipt and no deferred CAT retry.
No entry arms transmit. This does not complete message sending, QSO operation,
radio handoffs, FM/split/satellite contexts, remote audio or hardware acceptance.

## Decoder settings

Operation v3 advertises `decoderSettings` separately from receiver controls and
tier/workspace entry. Its closed `decoder.js8Speed` and `decoder.msk144Period`
intents carry the displayed prior value and one new native choice. The existing
JS8 speed chips and MSK144 period selector send only explicit gestures, bound to
the displayed station connection. They keep displaying actual station samples;
a saved receipt cannot invent the new decoder state. Older peers remain passive.

The station requires the matching active tier, native digital source, idle
disarmed radio, fresh known-unkeyed observation and current authority. JS8 speed
uses the shared native installer beneath the existing source mutex, acquired
without waiting behind a decode. MSK144 retains its native narrow setter; no
full Settings apply, QSO reset, queue clearing or new slot policy is introduced.
Valid local speed/period gestures invalidate old pending remote hardware work.

The native host atomically saves the current full Settings with only the one
field changed, then applies the existing runtime verb. Save failure leaves both
runtime and persisted choice unchanged. Once admitted, the saved choice survives
later disconnection; duplicate and result requests recover the same receipt
without repeating the mutation. The JS8 receive-speed mask, other decoder form
settings, transmit operations and physical acceptance remain separate work.

## Receiver settings

Operation v3 advertises `receiverSettings` independently from `decoderSettings`.
Closed `decoder.depth` and `receiver.rxOffset` intents include the displayed
tier and prior value. Depth uses the native snapshot's 1..3 fallback; RX uses
the displayed finite frequency and a new value in the native 200..4000 Hz range.
The host requires the same fresh, matching idle native digital radio and local
authority as the other saved decoder choices. It saves only that projection
before the shared native setter. Local adjustments invalidate pending Remote
hardware work without changing TX generation. Other preferences/profiles,
source identity, slots and TX markers remain under their native policy.

The browser uses the actual depth chips, FT RX entry, FT/JS8/Tempo waterfall
and JS8 offset rows. A saved receipt does not manufacture a new station sample.
An RX draft is discarded if the local value changes during editing or permission
is lost. Only the RX gesture is admitted; right/Shift and Ctrl/Command cannot
move TX or both markers with receive permission. Older peers remain passive.
RX gain, other Settings forms, TX/QSO/media operations and physical acceptance
remain separate work.

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

### FT8/FT4 operating controls (operation v4)

Compatible peers opt into operation v4 with `x-nexus-operation-ft-version: 1`.
The existing FT cockpit CQ and TX On/Off buttons use the `ftOperate` capability.
Nexus at the shack must grant this browser station control and separate transmit
permission. The grant is boot-scoped; granting it does not start transmission.
CQ and TX enable carry the displayed tier, radio context and transmit generation,
and the station requires current radio readings before calling the existing FT
verbs. Native sequencing, message generation and timing remain authoritative.

TX Off retains native behavior: an over already in flight may finish. Stop TX
uses a separate `stopTransmit` request that revokes the current generation without
waiting for the Engine, ordinary commands or receipt storage. A delayed command
from before Stop cannot rearm that generation. Stop stays visible and clickable
when station display data becomes unavailable. Its acknowledgement confirms
revocation, not physical RF cessation. Later local arming belongs to the local
operator and cannot be stopped by a former remote owner.

Local transmit revocation also bypasses pending durable file operations. The
station reconciles the removed grant before accepting subsequent commands;
older settings refreshes cannot restore the revoked grant's display. Older
operation versions retain their existing capability vocabulary and cannot arm FT.

The separate `ftCall` capability connects double-click and keyboard Work gestures
in the existing decode, roster and station-card surfaces. The station checks the
selected message/report/offset against its native decode history, or the selected
roster entry against its current station data, before calling the native routine.
The desktop identity check and saved double-click arming preference still apply.
Plain decode selection fills the browser's DX fields; Ctrl-double-click can also
move RX with receiver permission, without borrowing transmit authority.

The `ftExchange` capability connects S&P, Resend and free text in the same FT
strip. It carries the rendered partner, exchange state, queued message and CQ-run
flag, plus the original controller window and transmit generation. A changed
exchange is refused before the native verb runs. S&P uses the native decoder reset
under a nonblocking lock; none of these commands changes the TX-enable policy.
Remote free-text drafts remain until confirmation, including when a request is
refused or its outcome is uncertain. Period and TX-offset controls
and complete durable QSO logging still require their own operation paths.

The separate `ftMessages` capability connects the existing Tx1–Tx5 buttons and
Alt+number shortcuts, including deliberately typed targets. Tx6 uses the existing
directed-CQ parser and `ftOperate`. Each message choice binds the displayed QSO
before asynchronous work; the station compares it before the shared native
override. Native arming preference, parity and immediate-slot behavior remain.
Draft editing and Generate/Clear stay local to the browser, while the next-message
indicator follows station confirmation. Full QSO logging and TX settings remain
incomplete.

Decode/roster Call selection currently requires the native 240-row history or current roster.
Older displayed history, typed Call-button targets, complete QSO progression and logging,
other modes, browser audio and hardware/WAN acceptance remain incomplete.
Source verification is not installation or deployment.
