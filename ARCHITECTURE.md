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

The hosted Remote pilot reuses `App.tsx` through `applicationTransport.ts` beneath
`api.ts`. `remote/` supplies the authenticated Cloudflare station room; the outbound
native session in `src-tauri/src/remote_service/` owns closed, versioned reads and
a separate versioned operation contract. Nexus at the shack owns distinct local
logging and station-control grants, the shared controller lease, expiring
context-bound commands, deduplication and durable append receipts. Receiver
gestures call native Engine verbs; amplifier commands reach the existing port
owner with revocable permission and later readback receipts. Frequency and mode intents
run through the active RadioLoop before normal settings reconciliation: fresh
CAT readback precedes the native QSY and atomic settings save under the Engine
mutex. Explicit section entry shares native memories and power policy, remains
disarmed, and confirms any power reduction within the same CAT transaction.
Decoder selection shares native tier/channel policy and the stable decoder
mutex; a busy decode refuses the remote transition without blocking the Engine.
Same-tier selection remains a complete no-op. Operation v3 adds the radio
capabilities while preserving v1 logging and v2 receiver/amplifier clients.
The explicit `workspace` capability enters FT, Tempo or JS8 through the same
native area, mode, channel-memory and session helpers. Preparation is passive;
the existing radio owner commits after CAT/power readback. Decoder installation
and HARQ reset use the same held serialization guard, avoiding a nested lock
while preserving the native transition effects. Newer valid local operating
specs retire older remote work even when the dial and CAT mode are unchanged.
The separate `decoderSettings` capability connects the existing JS8 speed chips
and MSK144 period selector. Exact prior values and idle native context guard an
atomic one-field Settings save before the shared native runtime setter. JS8
installation uses the same stable source lock without blocking Engine; MSK144
keeps its narrow period semantics. A failed save publishes no runtime change,
and a receipt never substitutes for the later station sample.
The separate `receiverSettings` capability connects decode depth and RX-only
audio offsets through the same atomic persistence path. It binds the displayed
tier and prior value, then uses the native narrow setters without replacing the
decoder or changing TX offsets, timing, queues or permission. FT/JS8/Tempo keep
their existing waterfall gestures, and FT's RX field discards a draft if the
local value changes or authority is lost. TX and combined-marker gestures
require their own future capability; receive permission cannot enable them.
Native amplifier buttons and saved follow-band also validate the exact completed
serial poll after I/O. Current settings, observed physical PTT and read expiry
bound each write; local gestures take precedence over automatic steps. These
checks never use an amplifier as a transmitter stop or alter FT sequencing.

Amplifier follow-band is a dedicated operation-v3 capability. The actual Settings
checkbox retains its checkbox-then-Save interaction; the request carries only
the desired boolean, displayed radio, prior choice and bounded public Settings
revision. The native host checks authority and fresh idle hardware before enabling,
then atomically saves the narrow change under the existing Engine owner. Disabling
needs no hardware reading. A saved-setting receipt does not claim hardware readback
or RF state, and lease loss never resets a saved preference. Settings captures
invalidate on their own projected revision without invalidating unrelated planning
documents. The amplifier strip uses the existing aged observation stream and its
connection identity, rather than borrowing authority for an older displayed radio.
Unconfirmed targets never become deferred local retunes. Login, entitlement and
browser approval remain separate checks.

The `radioLevels` operation-v3 capability connects existing RF power, mic gain,
noise-reduction depth, compressor depth and notch-frequency controls. The active
radio owner compares the displayed prior value and physical position, checks idle
PTT/split state, then performs one permitted CAT write and reads the actual level.
Native limits and per-mode power ceilings still apply. A local setter retires old
remote work, including an away-and-back change. Only confirmed readback enters
native desired/observed state and the owner's caches; uncertain writes never become
retries. Browser drags and held adjustment keys retain a context-bound draft and submit
the released target once. Lost authority, changed readings/context or canceled
input discard that draft permanently. Inputs return to station samples after
release; a receipt does not fabricate readback. This capability grants neither
audio-drive control nor permission to transmit.

Radio selection has a passive native Settings projection sharing the local
handoff's outgoing-profile banking, daemon-port separation and monitored-or-saved
tune resolution. The projection grants no hardware access and cannot be applied
wholesale; actual selection still owns context retirement and hardware completion.
Radio handoffs, mode-specific operating actions and remote transmission remain
incomplete; no arbitrary Tauri bridge exists.
See [the Remote contract and limits](remote/README.md#existing-nexus-workspace).

Remote Phone and CW contact forms mount on first visit and retain their component
identity across workspace navigation. Hidden hosts pause scopes, meters, CW display
polls and collection reads, and cannot acquire gesture authority from the shared
session. Returning resumes display interest without logging, retuning or replacing
the controller lease. Drafts stay in the existing forms; this does not add durable
draft storage. Native cockpit mounting and transmitter cleanup remain unchanged.

Full Nexus remains the hosted default. Optional Quick Operate is browser-local
presentation state above that same App, with Operate, Hunt and Log destinations
and a return to the full interface. Phone/CW prioritize the existing contact form,
keep amplifier readbacks and stop controls outside optional radio detail, and
pause folded scopes. Presentation changes neither pane preferences nor authority,
and cannot resubmit an unconfirmed QSO. Other modes retain their existing layouts;
Quick is not yet complete mobile or operating parity.

The receive-audio foundation is local to `tempo-audio`: the sole `RxDsp` capture
consumer offers bounded device-rate mono copies through `receive_audio.rs`, before
display resampling. It never consumes the decoder ring or gives media a CAT/TX
handle. Inactive readers copy nothing; congestion drops media, and capture-source
replacement ends subscriptions. Its age limit measures DSP publication, not physical
capture time. This seam does not authorize a browser or provide playback, signaling,
codecs or a relay. A future media adapter must bind the actual input and radio to a
local grant, including the service's possible System default recovery fallback.
Capture descriptions now retain the radio and requested input from the open
request alongside the resolved backend label and explicit System default flag.
They travel atomically with the source epoch, stay local and grant no access.
Capture teardown retires media before releasing the old device; an unsuccessful
reopen keeps local retry behavior and cannot resume the old media reader. An OS
endpoint label does not prove the physical source behind virtual/default routing.

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
