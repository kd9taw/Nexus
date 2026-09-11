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
browser approval remain separate checks. Radio handoffs, mode-specific operating
actions and remote transmission remain incomplete; no arbitrary Tauri bridge exists.
See [the Remote contract and limits](remote/README.md#existing-nexus-workspace).

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
