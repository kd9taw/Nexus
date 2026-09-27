# The stream wire contract, version 1

These fixtures are the contract between the three parties to a streamed Remote session: the
station (Nexus at the shack), the relay (the Worker's room) and the page. Each side tests its own
parsers and emitters against the same files, so a change to one side that the others have not
made fails a test instead of a session. Rust reads them in `crates/tempo-stream` and in the
station's relay parser (`src-tauri/src/remote_service/transport.rs`).

A file holds cases by hop. Every case in an accepted list must parse on the receiving side and
come back out unchanged; every case in a `refused` list must be refused by the receiver's parser
or its validation, as the case says. Identifiers, addresses and keys in these files are
invented: UUIDs are all-`0`-and-`1` patterns, IP addresses come from the documentation ranges
(RFC 5737), and every ICE password and DTLS fingerprint is made up.

## Negotiation

- A station that can stream sends `x-nexus-stream-version: 1` on its relay connect, beside the
  headers it already sends. It sends it only when the operator has turned streaming on and the
  platform can capture (Windows today).
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
| relay → page | the same without `sessionId` |

Payloads carry `kind`:

- page → station: `offer { sdp }`, `candidate { candidate, sdpMid }`, `close {}`.
- station → page: `answer { sdp }`, `candidate { candidate, sdpMid }`.

`leaseId` is the page's claim to station control. The station admits an offer only for a
browser holding the operator's station-control grant and this live lease, and it checks that
before any WebRTC state exists. A lapsed lease or a revoked device ends the stream.

`streamState.reason` is one of a fixed set, and a page must refuse anything else:
`notController`, `streamDisabled`, `streamUnavailable`, `streamInUse`, `invalidOffer`,
`connectionFailed`, `streamClosed`.

### Bounds

- The station's control socket closes on any frame over 8,192 bytes, which would drop the whole
  Remote connection. So the relay must never forward a stamped stream message over **7,168
  bytes** (measured as the JSON it sends). The page keeps its offer small by restricting the
  video transceiver to VP8 (`setCodecPreferences`), and the mic transceiver to Opus when it
  exists.
- An SDP is at most **6,144 bytes**; a candidate string at most **512**; `sdpMid` at most 32
  characters from `[A-Za-z0-9_-]`.
- The station refuses an offer that is not DTLS-SRTP (no `a=fingerprint:sha-256`, or any media
  line on a profile other than `UDP/TLS/RTP/SAVPF` or `UDP/DTLS/SCTP`), that has no VP8 video
  line the page can receive, or that has no data channel. It answers `invalidOffer`.
- Candidates trickle one per message. The station advertises server-reflexive (and, once TURN
  exists, relay) candidates only. It never sends a host candidate, and its `raddr`/`rport` are
  `0.0.0.0 0`, so no LAN address leaves the shack. The page should give its peer connection a
  STUN server so it has a reflexive candidate of its own.

## Data channels (`channel.json`)

The page creates three data channels before `createOffer`, each with the label and reliability
shown. The station identifies them by label.

| Label | Reliability | Carries |
|---|---|---|
| `control` | reliable, ordered | today's operation requests (`state`, `heartbeat`, `release`, `stopTransmit`), input, their replies, `pttState` |
| `ptt` | unordered, `maxRetransmits: 0` | `pttHold`, `pttRelease` |
| `audio` | unordered, `maxRetransmits: 0` | receive audio, exactly the relay's `audioRx` bundle and `audioState` |

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
session. A fresh-frame heartbeat renews it. When it lapses, whether the heartbeats stopped or the
picture went stale, the station stops every transmission within 5 s plus one radio tick. Input
and the held PTT are admitted only while presence is live.

**PTT** is a held state, never a keydown/keyup pair. While the page's PTT control is held it
sends `pttHold { holdId, seq }` every 100 ms on the unreliable `ptt` channel. `holdId` is a fresh
UUID per press, and `seq` counts up from 0 within it. The station keys on the first hold it
accepts and releases the over when no hold arrives for 200 ms. `pttRelease` releases at once, as
a courtesy: the gap is the guarantee. A press the station has ended never keys again, whatever
arrives late, so a new press needs a new `holdId`. The station reports what it did on `control`
as `pttState { holdId, keyed, reason? }`, where `reason` is one of `refused`, `lapsed`,
`released`, `stopped`.

**Input** is a DOM-level description of what the operator did over the video, never an OS input
event. `x` and `y` are fractions (0 to 1) of the decoded video frame, which is the station's
Nexus window only. `modifiers` is a bitmask: 1 Shift, 2 Control, 4 Alt, 8 Meta. `clicks` is the
DOM click count (0 for a move). The page coalesces moves to at most 60 per second.

| Message | Fields and limits |
|---|---|
| `pointer` | `action` down, move, up or cancel; `x`, `y` in 0 to 1; `button` -1 to 4; `buttons` 0 to 31; `modifiers` 0 to 15; `pointerType` mouse, touch or pen; `clicks` 0 to 3 |
| `wheel` | `x`, `y` in 0 to 1; `deltaX`, `deltaY` finite, magnitude at most 10,000; `deltaMode` 0 pixel, 1 line, 2 page; `modifiers` |
| `key` | `action` down or up; `key` (DOM key value, 1 to 32 characters, no control characters); `code` (DOM code, 0 to 32 ASCII letters and digits); `modifiers`; `repeat` |
| `text` | `text`: 1 to 256 characters of committed text, no control characters |

Every `control` message from the page is at most 1,024 bytes.

## The station's own webview (`webview.json`)

The station delivers admitted input to its own main window as the Tauri event
`remote-stream-input`, and to no other window. The payload is the input message the station
admitted, with `x` and `y` as fractions of that window's viewport (the station streams the
window's client area, so the two coordinate spaces are the same), plus `reset {}`, which the
station sends when a stream ends or its presence lapses. A Nexus-owned module in the page turns
these into synthetic DOM events at `document.elementFromPoint`, and `reset` releases anything
that module is still holding down. Nothing here ever becomes OS input: the station has no
`SendInput`, `keybd_event` or `mouse_event` call.
