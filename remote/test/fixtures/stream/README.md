# The stream wire contract, version 1

These fixtures are the contract between the three parties to a streamed Remote session: the
station (Nexus at the shack), the relay (the Worker's room) and the page. Each side tests its own
parsers and emitters against the same files, so a change to one side that the others have not
made fails a test instead of a session. Rust reads them in `crates/tempo-stream` and in the
station's relay parser (`src-tauri/src/remote_service/transport.rs`).

A file holds cases by hop. Every case in an accepted list must parse on the receiving side and
come back out unchanged; every case in a `refused` list must be refused by the receiver's parser
or its validation, as the case says. Identifiers, addresses and keys in these files are
invented: UUIDs are all-`0`-and-`1` patterns and IP addresses come from the documentation ranges
(RFC 5737). The station's answer and its candidate are real str0m 0.24 output, taken from a
throwaway session answering the fixture offer (its ICE password and DTLS fingerprint were that
session's and are used nowhere else), so a check against them is a check against what the
station actually sends. The page's ICE password and fingerprint are made up.

## Negotiation

- A station that can stream sends `x-nexus-stream-version: 1` on its relay connect, beside the
  headers it already sends. Every station on a platform that can capture sends it (Windows
  today), whether or not the operator has switched streaming on, so an offer to a station with
  streaming off is answered `streamDisabled` rather than never answered.
- The relay forwards a `stream*` message to a station only if that station advertised the
  header, exactly as it does for `audio*`. The station's parser refuses unknown messages by
  closing the whole control socket, so a station that did not advertise must never see one.
- `/config` advertises `streamVersion: 1` for the page.

## Signalling (`signal.json`)

Signalling rides the relay sockets that already exist. The page offers and the station answers.

| Hop | Messages |
|---|---|
| page → relay | `streamSignal { leaseId, payload }` |
| relay → station | `streamSignal { sessionId, deviceId, leaseId, payload }`: the relay writes `sessionId` and `deviceId` from its own admission record and never reads them off the page's message |
| station → relay | `streamSignal { sessionId, payload }`, `streamState { sessionId, streaming, reason? }`: addressed to one session |
| relay → page | the same without `sessionId`, and the relay's own `streamState` (below) |
| relay → station | `streamEnd { sessionId, reason }`: the relay ends that session's stream |

Payloads carry `kind`:

- page → station: `offer { sdp, publicKey, signature }`, `candidate { candidate, sdpMid }`,
  `close {}`. The offer is signed with the browser's device key (A5, below). `publicKey` and
  `signature` come together or not at all: an offer without them parses (a page from before the
  device key), and the station refuses it at admission, by name, rather than at its parser, which
  would close the whole control socket.
- station → page: `answer { sdp }`, `candidate { candidate, sdpMid }`.

`leaseId` is the page's claim to station control. The station admits an offer only for a
browser holding the operator's station-control grant and this live lease, and it checks that
before any WebRTC state exists. A lapsed lease or a revoked device ends the stream.

`streamState.reason` is one of a fixed set, and a page must refuse anything else:

- from the station: `notController`, `streamDisabled`, `streamUnavailable`, `streamInUse`,
  `invalidOffer`, `connectionFailed`, `streamClosed`, `remoteOff` when it is ending a stream
  the relay ended (below), and the two device-key refusals `deviceNotPinned` and
  `deviceKeyMismatch` (A5, below);
- from the relay alone, which the station never sends: `serviceAccessExpired` (the account's
  command entitlement has lapsed: the relay holds the offer) and `tryLater` (the page is over the
  relay's signalling budget).

The relay answers a page itself, without the station, in three cases
(`roomToBrowserFromRelay`): the station never advertised the stream (`streamUnavailable`), the
entitlement lapsed (`serviceAccessExpired`), and the budget (`tryLater`).

**`streamEnd`** (`roomToStationEnd`). When Remote access is switched off by hand mid-stream, the
relay sends the station `streamEnd { sessionId, reason: "remoteOff" }` (operator decision,
2026-09-27: end it within about 2 s). It may end a stream with another reason from the set, such
as `serviceAccessExpired`. The station ends that session's transmit presence at once, so any
transmission halts on the radio loop's next poll, tears the session down, and sends its last
`streamState` with the same reason. A `streamEnd` for a session that is not streaming changes
nothing.

### Binding a stream to the approved browser (A5)

The relay stamps `sessionId` and `deviceId`, so on its word alone a compromised relay could offer
a stream as any approved browser. The offer is therefore signed by the browser itself, with a key
the station pinned when the operator approved that browser at the radio.

- **The key.** Each browser holds one ECDSA P-256 key pair per station, made with WebCrypto when it
  first needs it: when it confirms a pairing, asks the station for approval, or finds itself
  approved with no key. The private key is non-extractable and never leaves the browser
  (it lives in IndexedDB as a `CryptoKey`). `publicKey` is the public half as its SPKI DER, in
  lowercase hex: always **182** characters, the fixed P-256 prefix
  `3059301306072a8648ce3d020106082a8648ce3d030107034200` then `04` and the 64-byte point.
- **The pin.** The station pins `SHA-256(SPKI DER)` for that browser when the operator approves it
  at the radio, and both ends show the same short form of it beside the browser's name: its first
  8 bytes as four groups of four uppercase hex digits (`3F2A 9C1B 77E0 4D12`). That comparison is
  the human check that the key the station pins is the browser's own and not one the service
  substituted.
- **The signature.** `signature` is ECDSA P-256 with SHA-256 over these 160 bytes, as the IEEE
  P1363 `r‖s` WebCrypto produces (64 bytes), in lowercase hex (**128** characters):

      "nexus-stream-offer/1" (20 bytes of ASCII)
      ‖ SHA-256(fp)          (32 bytes)
      ‖ stationId ‖ deviceId ‖ sessionId   (each the 36-character lowercase UUID)

  `fp` is the offer's own DTLS certificate fingerprint: the 32 bytes of its `a=fingerprint:sha-256`
  value. Every such line in the offer must carry the same value (a browser writes one per media
  section); an offer whose lines disagree is `invalidOffer`. The ids are the ones the relay stamps
  and the station's own; the page learns its `sessionId` from the socket's `session` message.
- **What the station checks, at admission, before any WebRTC state exists:** that it pinned a key
  for this browser (else `deviceNotPinned`: a browser approved before keys existed is approved
  again at the radio, once); that the offer is signed, that `SHA-256(publicKey)` is that pin and
  that the signature holds (else `deviceKeyMismatch`). **After the DTLS handshake:** that the
  certificate the page presented has the signed fingerprint (else the session ends
  `deviceKeyMismatch` before any input or PTT is admitted).
- **The key's way to the station** (outside these files; the service's device routes): the page
  sends `publicKey` in its device request's body (`POST stations/:id/device`, beside `name`), and in
  its pairing confirm (`POST pair/confirm`, beside `id`), which the approval gives the browser it
  approves. The service checks it is a P-256 point and stores it. A browser already approved sends
  its key the same way (a browser approved before keys existed, or one whose key changed). The
  service stores it and leaves the approval alone. The station's pin still holds the old key or
  none, so it refuses that browser's streams until the operator approves it again at the radio.
  `native/devices` lists each browser's `publicKey`, or `null` for one that has none, only to a
  station that sends `x-nexus-device-key: 1`. An older Nexus parses that list with
  `deny_unknown_fields`, so it must never see the field. The pairing approval's `device` carries
  it by name.
- **Order.** The page holds its candidates while it signs the offer and sends them straight after
  it, so the relay forwards one session's signals in the order they were sent. An offer that waits
  on the service's entitlement check holds back the candidates behind it.

The fixtures carry the shape only: their `publicKey` has the right prefix and length but is not a
point on the curve, and their `signature` is not a signature. No key material is in these files.

### Bounds

- The station's control socket closes on any frame over 8,192 bytes, which would drop the whole
  Remote connection. So the relay must never forward a stamped stream message over **7,168
  bytes** (measured as the JSON it sends). The page keeps its offer small by restricting the
  video transceiver to VP8 (`setCodecPreferences`), and the mic transceiver to Opus when it
  exists.
- An SDP is at most **6,144 bytes**; a candidate string at most **512**; `sdpMid` at most 32
  characters from `[A-Za-z0-9_-]`; an offer's `publicKey` exactly 182 and its `signature` exactly
  128 lowercase hex characters.
- The station refuses an offer that is not DTLS-SRTP (no `a=fingerprint:sha-256`, or any media
  line on a profile other than `UDP/TLS/RTP/SAVPF` or `UDP/DTLS/SCTP`), that has no VP8 video
  line the page can receive, or that has no data channel. It answers `invalidOffer`.
- Candidates trickle one per message. The station advertises server-reflexive (and, once TURN
  exists, relay) candidates only. It never sends a host candidate, and its `raddr`/`rport` are
  `0.0.0.0 0`, so no LAN address leaves the shack. Both ends use the same STUN server,
  `stun:stun.cloudflare.com:3478`, with no credentials; the page gives it to its peer connection
  so it has a reflexive candidate of its own.

## Data channels (`channel.json`)

The page creates three data channels before `createOffer`, each with the label and reliability
shown, and one audio line it sends, for its microphone. The station identifies the channels by label.

| Label | Reliability | Carries |
|---|---|---|
| `control` | reliable, ordered | today's operation requests (`state`, `heartbeat`, `release`, `stopTransmit`), input, their replies, `pttState`, `micState` |
| `ptt` | unordered, `maxRetransmits: 0` | `pttHold`, `pttRelease`, `held` |
| `audio` | unordered, `maxRetransmits: 0` | receive audio, exactly the relay's `audioRx` bundle and `audioState` |

**Receive audio** has no request. The station starts sending it when the `audio` channel opens
and stops when it closes, so it flows for the whole stream: `audioRx` bundles, and `audioState`
when listening starts or ends. When the station cannot listen it says so at once with
`audioState { listening: false, reason }`: `audioUnavailable`. A current station never sends
`audioInUse` here: Listen on the relay's lane and every stream hear the station through its one
encoder, and a stream's own lane only ever serves that stream's session. A station from before
that shared encoder sent `audioInUse` when a browser was already listening on the Remote page;
the value stays in the contract so that station's message still parses. The page's Listen
control is local: it plays or drops what arrives, muted until the operator unmutes.

**Operation requests** on `control` are today's JSON operation requests, byte for byte, with one
addition: `heartbeat` carries `decodedFrameAt`. The station stamps the session and device from
the stream's admission; nothing on the channel names them. Replies are
`operationResponse { requestId, value | error }`, with the same `value` today's operation
protocol returns at operation version 4. A `heartbeat` reply also carries `presence`: whether the
station holds transmit presence for this session after that heartbeat.

**`decodedFrameAt`** is required on every stream `heartbeat` and is either `null` or the RTP
timestamp (an unsigned 32-bit integer, 90 kHz, exactly as carried on the station's video track)
of the most recent video frame the page has decoded and shown. Read it from
`requestVideoFrameCallback` metadata `rtpTimestamp`. A browser that does not expose it there may
send the video receiver's latest `getSynchronizationSources()` `rtpTimestamp`, but only while
`requestVideoFrameCallback` keeps firing. Otherwise send `null`, never an estimate. No wall clock
crosses this boundary: the station maps the timestamp back to its own capture clock. A heartbeat
whose frame is more than 2 s old, or `null`, renews the lease but not the transmit presence, so a
page that cannot see the station cannot keep it transmitting. The station sends at least two
frames a second while the picture is still, so a still window does not read as a stale one.

**Transmit presence.** While a stream is attached, the station holds a presence permit for the
session. Only a `control` heartbeat whose picture is fresh renews it. The page's ordinary
heartbeat on the relay socket keeps the lease, which stays bound to that socket's session, but
never presence. When presence lapses, whether the heartbeats stopped or the picture went stale,
the station stops every transmission within 5 s plus one radio tick of the last renewal: 5 s
after the connection drops, and up to 7 s after the picture freezes (2 s for the picture to go
stale, then the lapse). Input, `held` and the held PTT are admitted only while presence is live.
A stop is always admitted, blind or not and with the lease lapsed or not: `stopTransmit`,
`pttRelease` and `release`.

**PTT** is a held state, never a keydown/keyup pair. While the page's PTT control is held it
sends `pttHold { holdId, seq }` every 100 ms on the unreliable `ptt` channel. `holdId` is a fresh
UUID per press, and `seq` counts up from 0 within it. The station keys on the first hold it
accepts and releases the over when no hold arrives for 200 ms. `pttRelease` releases at once, as
a courtesy: the gap is the guarantee. A press the station has ended never keys again, whatever
arrives late, so a new press needs a new `holdId`. The station reports what it did on `control`
as `pttState { holdId, keyed, reason? }`, where `reason` is one of `refused`, `lapsed`,
`released`, `stopped`.

**The microphone** (the page's audio line, `sendonly` Opus, 20 ms frames, DTX off). A held PTT
ARMS an over and the operator's voice keys it: a press with no audio arriving puts nothing on the
air. So `pttState.keyed` says the press was taken, not that the rig is on the air. The station
says what its over is doing on `control` as `micState { armed, keyed, noPowerOut, ended? }`,
whenever a field changes. `armed`: an over is armed, by the page's PTT or by a press made through
the picture. `keyed`: the operator's voice has keyed the rig. `noPowerOut`: the voice has been
arriving for about two seconds and the rig reports no power out. It is display only (the rig's
SSB audio source is most likely its own microphone), it is never sent for a rig with no power
reading, and it changes nothing about the over. `ended` appears only on the message that reports
an over's end: `released` (the operator let go), `stopped` (a stop at the station, TX off,
leaving Phone, a tune, the licence), `audioGap` (no audio for 200 ms), `presence` (the session's
transmit presence lapsed), `ceiling` (the 10-minute limit), `watchdog` (the TX watchdog) or
`routeChanged` (the station's transmit audio route changed).

**Input** is a DOM-level description of what the operator did over the video, never an OS input
event. `x` and `y` are fractions (0 to 1) of the decoded video frame, which is the station's
Nexus window only. `modifiers` is a bitmask: 1 Shift, 2 Control, 4 Alt, 8 Meta. `clicks` is the
DOM click count (0 for a move). The page coalesces moves to at most 60 per second.

**Every key is an ordinary key, Space included.** The station's window decides what a key does,
exactly as it does for a local keypress: in the Phone cockpit Space is PTT under the desktop's own
rules, and typed into a field it is a space. The page's own PTT control, outside the picture,
sends `pttHold`.

**`held`: the held-key dead-man.** Anything held down over the picture is a re-asserted state,
never a press left standing. The page sends
`held { keys, buttons, seq }` on `ptt` at once when the set changes, then every 100 ms while
anything is held, and not while nothing is. `keys` is the DOM `code` of each key held (0 to 16,
each 1 to 32 ASCII letters and digits, no repeats; a key with an empty code is never
re-asserted). `buttons` is the same 0 to 31 mask as `pointer.buttons`. `seq` counts up from 0 over
the stream, one per message, as an unsigned 32-bit integer. The station hands it to its window
unchanged, only while presence is live. The window releases, with a proper key-up or pointer-up,
anything held and not re-asserted within 200 ms; `held` never presses anything; a `held` whose
`seq` is not above the last one seen is ignored; and `reset` is the backstop.

| Message | Fields and limits |
|---|---|
| `pointer` | `action` down, move, up or cancel; `x`, `y` in 0 to 1; `button` -1 to 4; `buttons` 0 to 31; `modifiers` 0 to 15; `pointerType` mouse, touch or pen; `clicks` 0 to 3 |
| `wheel` | `x`, `y` in 0 to 1; `deltaX`, `deltaY` finite, magnitude at most 10,000; `deltaMode` 0 pixel, 1 line, 2 page; `modifiers` |
| `key` | `action` down or up; `key` (DOM key value, 1 to 32 characters, no control characters); `code` (DOM code, 0 to 32 ASCII letters and digits); `modifiers`; `repeat` |
| `text` | `text`: 1 to 256 characters of committed text, no control characters |

Every `control` message from the page is at most 1,024 bytes.

## The station's answer to the page's offer (`station-answer.json`)

`offer` is the offer the page's own peer makes (`stream-link.ts`, `start()`: VP8 received, Opus
sent, the three channels), taken from Chrome 140. `answer` is what the station's own code wrote for
it (`Session::accept`, str0m 0.24, on Windows). The compiled stream scenario's stand-in station
answers with Chrome's own SDP; this is the answer the shack sends instead. The scenario hands it to
the page and requires the page to take it (its parser, `secureAnswer` and Chrome's
`setRemoteDescription`), after checking that the page's live offer still carries every media line,
codec and header extension the answer names. The Windows session test
`the_recorded_answer_is_what_the_station_writes_for_the_page_offer` requires the station to write the
same answer today, apart from what every session makes new. When either goes red, record the pair
again: the offer from the page's peer in Chrome, the answer from `Session::accept` on Windows. The
ICE credentials and fingerprints in both are from throwaway sessions and are used nowhere else.

## The station's own webview (`webview.json`)

The station delivers admitted input to its own main window as the Tauri event
`remote-stream-input`, and to no other window. The payload is the input message the station
admitted, and the page's `held` set, with `x` and `y` as fractions of that window's viewport (the
station streams the window's client area, so the two coordinate spaces are the same), plus
`reset {}`, which the station sends when a stream ends and each time its presence lapses. A Nexus-owned module in the page turns
these into synthetic DOM events at `document.elementFromPoint`, and `reset` releases anything
that module is still holding down. Nothing here ever becomes OS input: the station has no
`SendInput`, `keybd_event` or `mouse_event` call.
