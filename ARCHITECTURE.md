# Nexus architecture map

One page to orient before editing. The authoritative crate list is `ls crates/`; each crate's
`//!` module header is its contract — read it first.

## The three layers

```
ui/            React + TypeScript (Vite)  — views, cockpits, panels
   ↕ Tauri invoke/events (ui/src/api.ts ↔ #[tauri::command] fns)
src-tauri/     the shell — ~240 commands, settings, caches, connectors glue
   ↕ plain Rust calls
crates/        domain logic (workspace)   — DSP, protocol, propagation, logbook
   ↕ FFI (cabi shims)
libtempo/      CMake-built native cores   — vendored WSJT-X Fortran/C + FT1 modem
```

Data flows down as commands, up as Tauri events. The UI holds no domain truth; snapshots are
rebuilt from Rust state (do not assume a UI field is stale-frozen — it usually is not).

## Nexus Remote

The shack PC keeps the radio and stays the authority. Everything a remote operator sees is the
station's own Nexus, and everything they do reaches it as input to that Nexus or as one of a
handful of lease and Stop requests; the station decides what is legal and what transmits. The
hosted half (`remote/`: a Cloudflare Worker, one Durable Object room per station, D1 for accounts,
stations, devices and entitlement) carries pictures, sound, signalling and ciphertext and nothing
else. The trust model, routes and limits are in [remote/README.md](remote/README.md).

**Three ways in, one authority.**

- **The hosted page is a stream.** `ui/src/remote-web` builds a page whose operating surface is
  `StreamView.tsx`: the station's own window over WebRTC (VP8 picture, data channels for input,
  PTT and the operation requests, Opus audio both ways), plus Listen (receive audio on the room's
  audio lane) and Stop TX. The station's side is `src-tauri/src/remote_service/stream.rs` over
  `crates/tempo-stream` (`session`, `offer`, `video`, `stun`, `keep_awake`). The old command page,
  which drove the station through its own forms, and its separate FT8/FT4 transmit and logging
  permissions are gone: `remote_service/operations.rs` answers anything that page sent
  `stationUnsupported`, and nothing runs.
- **Nexus on another computer, the full client.** Client mode (`src-tauri/src/client_mode.rs`)
  restarts Nexus with no station of its own and sends every command its screens make to the shack,
  routed and admitted at both ends by one class table (`crates/tempo-client/src/classes.rs`,
  enforced by `src-tauri/src/command_dispatch.rs`; `command_classes.rs` fails a registered command
  with no row). The command link (`tempo-client`'s `link`, `wire`, `tls`) is TLS 1.3 with both raw
  keys pinned. On the shack's own
  network it is a direct connection to `remote_service/lan.rs`, which listens only on one private
  IPv4 address, never a VPN or virtual adapter, and is found by name through Windows' DNS-SD
  (`tempo_stream::lan`). Over the internet the same TLS channel runs inside a pipe through the
  station's room (`remote/src/pipe.ts`, `remote_service/tunnel.rs`), which the room passes on
  unread; the station proves its pairing key and admits only a computer approved at the shack.
  Beside the link run Stop's road and the audio road, each a UDP socket keyed from the link's TLS
  session, found by ICE with a TURN fallback on the internet (`tempo-client`'s `stop_road`,
  `audio_road`, `ice`, `turn`). `still_there.rs` ends an idle internet link after fifteen minutes
  without operator input and a minute more without an answer. `tempo-client` has no Tauri, webview
  or engine in it: without its `device` feature it builds on no engine crate (the log and what
  Stop's road carries of the engine's state are `crates/tempo-base`; the Opus codecs, resamplers
  and headset rings both ends use are `crates/tempo-codec`).
- **A stream window on the shack's network.** A paired computer's Stations on this network window
  (`ui/src/lan`, served by `src-tauri/src/lan_client.rs` from a loopback origin with no Nexus
  command) shows the same stream over the LAN listener.

**Sign-in, pairing and keys.** A station is paired by a code made and approved at the shack, and
can be bound to its owner's account by a sign-in at the shack (`remote_service/account.rs`); the
station checks the issuer's proof itself, with the issuer's keys cached for when the internet is
down. A device listed by the service on the strength of a sign-in is still granted nothing until it
is approved at the shack (`GRANTS_ON_SIGN_IN` is off). Each pairing has a station signing key
(P-256, kept in the OS credential store, `remote_service/station_key.rs`) whose public half is
pinned at the service: the station signs every stream answer with it, and a page checks each
answer against the key the service lists, refuses one without a good signature, keeps the first key
it saw for the station (refusing the stream while the service lists another) and shows that key's
fingerprint for comparison with the shack's. In the other direction, the browser's own device key,
pinned when it was approved, signs its stream offer, and its DTLS certificate must match; the
relay's operation and audio lanes carry a per-message proof from the same key
(`remote_service/lanes.rs`), except `state` and `stopTransmit`.

**Grants, the lease and presence** (`remote_service/operations.rs`). Only a control grant given at
the shack (or restored after a restart for a device still approved) lets a device take the one
controller lease. The lease is five seconds, renewed by heartbeats, bound to the connection that
took it, and shared by every road; a computer driving the station (on the LAN or through the room)
can take control from the one holding it, whose over ends as a Stop ends it. No grant and no lease
arms TX. While a streamed or client session is attached, the engine holds a transmit presence
permit for it (`crates/tempo-app/src/engine/remote_stream.rs`), minted from the lease and renewed
only by a heartbeat that names a fresh picture (for the full client, a heartbeat on Stop's road
naming a snapshot pushed under two seconds before). When it lapses, the next radio-loop poll runs
`halt_tx`, which ends every transmission at the station, however it started. Presence only ever
stops. A PTT held from afar is honoured only while presence is live, in Phone, and only with the
operator's microphone audio arriving (`engine/remote_mic.rs`); a 200 ms gap in either ends the
over.

**Stop.** Any device holding station control may stop, and an admitted Stop stops everything
transmitting at the station, however it started; it is never refused for its age. Stop's admission
does not wait on the Engine, a file write, D1 or an ordinary command
(`crates/tempo-app/src/remote_control.rs`, `remote_service/operations/transmit_stop.rs`). Each
road has its own way to it: the room's operation lane (no proof needed), the stream's data channel,
and for the full client both the command link and Stop's road, either of which alone stops the
station.

**Audio.** `crates/tempo-codec/src/receive_audio.rs` holds bounded, device-rate copies of the
station's receive audio, published by the RX DSP thread (`crates/tempo-audio/src/rxdsp.rs`)
without draining its capture ring, and copies nothing while no one reads it. One Opus encoder
(`receive_encode.rs`, fanned out by `remote_service/audio.rs`) exists only while someone listens
and feeds every listener: Listen on the relay's audio lane, a stream's audio channel, the client's
audio road. Each listener has its own bounded backlog and
loses its own oldest frames when it falls behind. Listen and a stream's audio need the control
grant and a live lease.

**Entitlement and billing.** Access is the account's entitlement as the Worker reads it from D1: a
trial that starts when a station is approved at the radio, a grant made by hand, or a subscription.
Subscriptions go through a separate, private billing Worker that holds the payment keys. Its
binding is written into a deploy only when that environment's `billing.deployed` is true in
`remote/scripts/staging-common.mjs`, and both environments ship with it false, so billing is off
by default and the `billing/checkout` and `billing/portal` routes answer `billingNotAvailable`.

No arbitrary Tauri bridge exists: a page reaches the station only through the requests above, and
the station's native routes refuse any request carrying an `Origin` header.

## Crates (roles, not an exhaustive list)

| Crate | Role |
|---|---|
| `tempo-core` | Transceiver core: FT1 protocol state, framing, HARQ, presence |
| `tempo-app` | UI-facing application logic wrapping tempo-core; `engine.rs` is the hub (decode passes, QSO log journal, spectrum feeds) |
| `tempo-audio` | Real-radio transport (feature `device`): sound card loop, CAT, PTT, CW keying dispatch (`service.rs`) |
| `tempo-fast` / `tempo-fast-sys` | Safe wrapper / raw FFI for the FT1 4-CPM turbo modem in libtempo |
| `ft8`, `ft4`, … | One thin crate per WSJT-X mode wrapping libtempo's vendored decoder (pattern repeats as modes are added: fst4, q65, jt65, msk144, wspr) |
| `modes` | Mode + signal-source abstractions — the spine that lets cockpits share one rig |
| `propagation` | Openings intelligence, needs engine, spots, repeaters/Program, p533 HF prediction, live fetchers (feature `live`) |
| `tempo-net` | WSJT-X-compatible UDP telemetry + PSK Reporter |
| `tempo-sstv` | SSTV encode/decode |
| `deepcw` | ONNX inference for the AI CW decoder (model is gitignored — a build without it must fail loudly, not silently ship) |

## Navigating the big files

There are four files where most edits land. Don't scroll them — search them:

- `src-tauri/src/lib.rs` (~11K lines, ~240 commands): find the `#[tauri::command]` whose name the
  UI calls in `ui/src/api.ts`; work outward from there.
- `crates/tempo-app/src/engine.rs` (~15K lines): organized by `impl` blocks
  (SpectrumFeed, DecodePass, DecodeJob, PendingMsgJournal, …) — search the type, not the line.
- `crates/tempo-audio/src/service.rs` (~8K lines): the device-thread loop; keying dispatch lives
  here. Transmit-safety invariants concentrate in this file — smallest possible diffs.
- `ui/src/components/SettingsPanel.tsx` (~6K lines): grouped by `settings-featgroup` blocks;
  search the visible label text.

## Vendored native code

`libtempo/vendor/` holds WSJT-X DSP sources (GPL) plus other vendored cores, built by
`libtempo/CMakeLists.txt` via each crate's build.rs. Rules: keep per-file license headers, keep
NOTICE current, prefer byte-identical vendoring with local patches documented in the commit
message. `modem-state-manifest.toml` tracks Fortran module-scope state (`save` symbols) — the
audit gate for decoder re-entrancy.

## Test geography

| Layer | Where | What it proves |
|---|---|---|
| Unit | `#[cfg(test)]` in-crate (~1.8K tests) + fixtures in `tests/fixtures/` | Domain logic, parsers, protocol state |
| UI | `ui/src/**/*.test.ts*` (vitest, ~850) | Components, feature logic |
| CI matrix | `.github/workflows/ci.yml` | Workspace + feature-gated builds, clippy `-D warnings`, MSRV, cargo-deny, Windows cross, Pi ARM |
| Decode parity | maintainer-side lab (recorded corpora vs stock WSJT-X) | The decoders decode what the reference decodes |
| On-air | maintainer with real rig | TX behavior, timing, interop — not automatable here |

Release mechanics: `scripts/release-prep <version>` aligns the three version manifests
(`src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`, `ui/package.json`), stamps the
CHANGELOG, and re-cuts the bundled TLE seed snapshot
(`src-tauri/resources/tles/tles.json` — the installer's offline satellite catalog, a
committed snapshot with no other moment that refreshes it; best-effort, a stale seed
warns and never fails the bump). Tagging and publishing are maintainer-gated (see
CLAUDE.md).
