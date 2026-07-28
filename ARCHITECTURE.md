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
(`src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`, `ui/package.json`) and stamps the
CHANGELOG; tagging and publishing are maintainer-gated (see CLAUDE.md).
