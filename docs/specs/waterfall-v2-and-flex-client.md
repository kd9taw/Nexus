# Waterfall v2 and the standalone Flex client — assessment and spec

**Status:** draft spec, 2026-10-01. Written from a code survey of this tree at `6ba639e`, a
competitive survey of the SDR console landscape, and a maintainer Q&A whose decisions are
recorded in §4. Nothing here is built. It is the document to pick up when the work starts.

**One-line decision:** two headline minor releases, in this order — **Release A, Waterfall v2**
(a GPU display engine with a calibrated scale, feeding every existing spectrum source) and
**Release B, the standalone Flex client** (Nexus as a full SmartSDR-protocol client, no
SmartSDR, SmartSDR CAT or DAX drivers required, multi-slice, full TX surface, SmartLink). The
IQ / openHPSDR backends come after both and are scoped only in outline (§7).

---

## 1. Why this, and why in this order

The maintainer's own observation started it: *"the waterfall still seems slow, clunky and very
2-D — even on my 9700 I don't visually notice a difference from the non-CI-V version, and I
think others on Flex see it the same way."* The code survey (§2) shows that is literally true,
for three reasons that have nothing to do with the hardware path:

1. **The rig's scope only ever reaches two cockpits.** In DATA mode the engine clears the RF
   row so the audio FFT takes over (`crates/tempo-audio/src/service.rs:6509`), and the shared
   `Waterfall.tsx` discards RF rows outright (`ui/src/components/Waterfall.tsx:650`). On the
   FT8/JS8/RTTY/PSK/SSTV screens the 9700's scope is never drawn.
2. **Where it is drawn, it is repeated, not interpolated.** A CI-V sweep is 475 points over a
   115200-baud serial link, a few sweeps a second; `PhoneScope.tsx` paints a row every 50 ms
   whether or not a new sweep arrived, so the waterfall advances by duplicating the last row.
3. **Every source is funnelled through one 512-bin, 0..1-normalised row** with a visual-AGC
   stretch at draw time, rendered on canvas-2D with `putImageData`. There is no calibrated dB
   axis, no auto dynamic range, no per-pixel trace from the source's full resolution, and no
   GPU path. The 3-D view is a 256-column, 96-row ridge fill on the same 2-D canvas.

So the display engine, not the hardware path, is the first workstream. It lifts the Icom, Flex
and audio waterfalls at once, and it gives the Flex client something worth feeding.

The second workstream is Flex because that is where Nexus already has users. Today the proven
Flex path still needs SmartSDR, SmartSDR CAT and DAX running on a Windows PC, drives one slice
per Nexus window, and the native port-4992 path is an off-by-default, read-only second client
that *has never run against a real radio* (`docs/rigs/flexradio.md`). Those users cannot
consolidate on Nexus until Nexus is the client.

---

## 2. Where Nexus stands today (facts from the tree)

### 2.1 Spectrum sources

There is **no IQ path anywhere** in the tree: no SoapySDR, no rtl-sdr, no HackRF/Lime/Pluto,
no openHPSDR Protocol 1 or 2, no TCI, no KiwiSDR. The only "IQ" strings are a parsed Flex status
key and three comments. Every spectrum display is fed by one of four producers:

| Source | Where | What arrives | Rate / resolution | Status |
|---|---|---|---|---|
| Sound-card audio FFT | `crates/tempo-core/src/spectrum.rs`, `crates/tempo-audio/src/rxdsp.rs` | Hann-windowed real FFT (microfft), 0–4000 Hz, 120 dB span | 20 ms tick, `FFT_N` 1024/2048/4096, published as 512 bins | shipped, universal |
| Icom CI-V scope `0x27` | `crates/tempo-audio/src/civ/scope.rs` | 475 points, values 0–160, absolute RF span | a few sweeps/s over serial at 115200 | shipped; IC-7300/7610/9700/705/905; 9700 field-verified |
| FlexRadio VITA-49 FFT (`0x8003`) | `crates/tempo-audio/src/flexspectrum.rs`, `crates/tempo-net/src/flexvita.rs`, `flexcat.rs` | u16 bins, `X_PIXELS` 2048 default, 200 kHz span, 15 fps; read-only second client | 15 fps | **opt-in, never run on hardware** |
| Yaesu FT-710 FT4222 bridge | `crates/tempo-audio/src/yaesu_wf.rs` | 850 usable bins, ~84 frames/s | 84 fps | behind off-by-default `yaesu-wf` feature |

The gate is `native_spectrum_kind()` in `crates/tempo-audio/src/rigmodels.rs:749`
(`SpectrumKind::{IcomCiv, FlexVita}`). FTDX101, TS-890 and K4 scopes are not supported; those
rigs are Hamlib CAT entries only. Thetis, PowerSDR, piHPSDR, SDR Console and SDRuno exist only
as Hamlib CAT profiles (model numbers 2054/2048/2040/2056/2051).

### 2.2 The row contract

`SpectrumFeed` (`crates/tempo-app/src/engine.rs:60–590`) holds three slots — `audio` (running
power average), `rf` (latest value), `scope` (narrow request) — and `row()` lets an RF row win
while under 1 s old, else audio while under 2 s old. `SPECTRUM_BINS = 512`. The DTO is
`Spectrum { row: Vec<f32>, lo_hz, hi_hz, source }` (`crates/tempo-app/src/dto.rs`), with every
producer normalising to 0..1 before publishing ("the UI's AGC/LUT does the display stretch").
Tauri commands: `get_spectrum_row` (audio only, by contract) and `get_scope_row(lo, hi, window)`
(RF-aware). Remote serves `peek_audio_row` / `peek_scope_row` as JSON rows of 512 floats every
100 ms (`src-tauri/src/remote_service/application.rs`).

### 2.3 Rendering

| Component | Lines | Feed | Draws | Gestures |
|---|---|---|---|---|
| `components/PhoneScope.tsx` | 1490 | `get_scope_row` (RF-aware) | canvas-2D, trace + waterfall, `ROW_MS` 50, rAF | click snaps to signal (`tuneSnap.ts`), drag = passband box, wheel = scrollback / sliders |
| `components/Waterfall.tsx` + `waterfall.ts` | 1306 + 1336 | `get_spectrum_row` (audio only; drops RF) | canvas-2D, `rowMs` 50–120 | click/shift/ctrl set RX/TX audio offsets |
| `dss.ts` | 176 | history ring | canvas-2D 3-D ridge, 256 cols × 96 rows | — |
| `waterfallHistory.ts` | 221 | — | retained 8-bit ring, 2048 rows × 1024 cols, per-row `{loHz, hiHz, tsMs}` | pause + scrollback |
| `MiniSpectrum.tsx` | 163 | audio | filled trace | none |

Shared: `colormaps.ts`, `waterfallPalette.ts`, `PalettePicker.tsx`, `useScopeTune.ts` (clicks
command immediately; drags coalesce to one `set_frequency` per 120 ms). The two history and 3-D
files are ports from AetherSDR (GPLv3), recorded in `NOTICE`.

The UI already runs **three.js** (`ui/package.json`) for the Globe, so WebGL is a proven runtime
on every shipped platform's webview. No spectrum component uses it.

### 2.4 Audio and CAT plumbing the new work plugs into

- `AudioBackend` trait (`crates/tempo-audio/src/backend.rs`): capture/play 12 kHz mono, a
  `spectrum_tap()`, and a `TxTee` alternate TX route ("EXCLUSIVE, NOT PARALLEL") that Flex DAX
  already uses. Device audio is `cpal` (`device.rs`, 3889 lines), resampled to `MODEM_RATE`
  12 000 by a 64-tap polyphase filter (`capture_resample.rs`).
- Flex DAX over the network (`flexdax.rs`, 1204 lines): VITA-49 classes `0x03E3` / `0x0123`,
  24 kHz → 12 kHz, **both directions** (sets `transmit set dax=1`, which is radio-wide and
  disconnects the mic in every program — documented trap), DAX channel 1 only.
- CAT: Hamlib `rigctld` over TCP by default (`rig.rs`); a native CI-V daemon (`civ/`) and the
  OmniRig shim (`omnirig/`) both **serve the rigctld protocol** to the rest of the app, which
  is the pattern a Flex client reuses; a CAT broker on :4532 (`rigctld_server.rs`) shares the
  radio with other programs.
- Already-parsed SmartSDR surface (`crates/tempo-net/src/flexcat.rs`): `slice`, `display pan`
  (create/set/remove), `stream create/remove`, `sub slice|pan|meter all`, `meter`,
  `client udpport|program`, `transmit set dax`, `ping`; status keys for slice (`in_use`,
  `active`, `tx`, `dax`, `client_handle`), pan (`center`, `bandwidth`, `x_pixels`,
  `stream_id`) and meters. VITA classes decoded: FFT `0x8003`, meters `0x8002`, DAX audio,
  discovery `0x534C`. **Not** decoded: waterfall tiles (`0x8004` in FlexLib), DAX IQ, mic
  audio, opus.
- TX safety the Flex client must thread through: the TX-enable latch (`set_tx_enabled`, OFF at
  launch), the wall-clock watchdog (`tx_watchdog_min`, `reset_tx_watchdog`), identity validation
  at the keying boundary, licence-privilege gating (`crates/tempo-app/src/privileges.rs`,
  `bandplan/licensed.rs`), the stop line (CLAUDE.md "UI layout contract").
- Overlay data that already exists: cluster/RBN rows (`SpotRow` in `ui/src/types.ts`, drawn
  today in `BandMap.tsx` / `BandStrip.tsx`), band privileges by licence class, FT decode history.

### 2.5 Remote

Both rows cross the hosted Remote as passive 100 ms JSON reads; the browser renders the same
components; click-to-tune is gated by `useRemoteScopeClick.ts`; span/ref settings go through
`crates/tempo-app/src/engine/remote_radio/scope.rs` with `ScopeFamily::{IcomCiv, Flex, None}`.

---

## 3. Competitive landscape (what the users compare against)

Full notes with sources are in Appendix A. The points that shape the spec:

- **"Zeus" is three products.** The benchmark the maintainer means is **ZeusSDR** (formerly
  OpenHPSDR Zeus): a GPL station engine (C#/.NET, Protocol 1+2, WDSP) driving a proprietary
  WebGL client, with a **WebGPU 3-D panadapter**, a **0–60 MHz wideband display** and native
  FT8/FT4 inside the console. Apache Labs forked its last GPL version as ANAN Core. Zeus Radio
  (zeusradio.com) is an older closed ZS-1 console and is not the comparison.
- **AetherSDR** (GPLv3, C++20/Qt 6, weekly CalVer releases, lead KK7GWY) is the closest sibling:
  Flex 6000/8000/Aurora is its primary backend with full TX over the SmartSDR protocol and
  VITA-49 waterfall tiles; Hermes-Lite 2 over Protocol 1 with TX since 2026-09-17; ANAN over
  Protocol 2 receive-only; Icom over RS-BA1; RTL-SDR. QRhi GPU spectrum and waterfall at up to
  60 fps, one trace value per screen pixel, 3-D stacked traces with history reprojection across
  pan/zoom, up to 8 detachable pans, colour-coded multi-slice markers, TNF drawn through the
  waterfall, SmartLink WAN, TCI server, DAX 8 RX + 1 TX + 4 IQ channels, SpotHub cluster
  overlay. eHam reviewers: "completely beats SmartSDR in function and features"; one complaint
  is "dark display and lack of spectrum customization options". Nexus already ports two of its
  files legally; the maintainer has approved porting C/C++ generally (§4).
- **SmartSDR** (Flex, closed): v4 Basic free per radio, SmartSDR+ paid yearly; unlimited slices
  and pans, TNFs, DX spots on the pan (iOS). No native macOS or Linux client — the gap a
  standalone Nexus client fills.
- **Thetis / piHPSDR / deskHPSDR** set the waterfall expectations the HPSDR crowd carries over:
  short log-recursive averaging (deskHPSDR 250 ms) instead of 0.95 exponential smoothing that
  "blurs CW, SSB syllables and FT8 slots into continuous noise"; auto dynamic range (row mean
  −5 dB floor, +55 dB span); AVG/PEAK detector modes; per-display FFT size and window; DX
  cluster and RBN spots on the pan; filter-edge dragging.
- **What "impeccable" means, in the users' words** (collected from reviews and issue trackers):
  GPU rendering at 60 fps; one analyzer value per screen pixel from all bins, never bin-skipping;
  calibrated 1 Hz-normalised dB with auto range; transient-preserving averaging; colour themes,
  smoothing, speed, timestamps; multi-slice markers; spot and band-limit overlays; smooth
  pan/zoom with history reprojection; TNF markers; waterfall history; IQ recording; diversity.

---

## 4. Maintainer decisions (Q&A, 2026-10-01)

| Question | Decision |
|---|---|
| Benchmark | ZeusSDR (OpenHPSDR Zeus) and AetherSDR |
| First target users | **Flex owners already on Nexus** — give them what they miss from SmartSDR so they can consolidate; serve the other SDR groups long-term |
| Sequencing | Display engine first, then the Flex client |
| TX in the first Flex release | **Yes** — on a Flex the radio is the transmitter, so this means Nexus owning the SmartSDR TX surface, not an IQ modulator |
| Flex ambition | **Standalone client, no SmartSDR needed** |
| Flex bench | Beta users with Flex radios (no radio on the maintainer's bench) |
| Flex gaps users name | multi-slice/multi-pan in one window; full TX surface (profiles, ATU, antenna, tune, power, mic/DAX TX); SmartLink WAN remote |
| SmartSDR API generations | v3 (6000) **and** v4 (8000 / Aurora) |
| Platform | Windows first |
| Waterfall v2 must-haves | all four groups: GPU renderer + full-res rows; calibrated dB + auto range + averaging; multi-slice markers + filter-edge drag + TNF markers; overlays (cluster, RBN, band edges by licence class, decodes) |
| Remote | desktop first, Remote follows in a later release |
| Packaging | **two headline minors**, one per release |
| Porting C/C++ | approved, with the per-file licence check from CLAUDE.md as the gate |

---

## 5. Release A — Waterfall v2 (headline minor)

**Headline an operator would name:** "a real GPU panadapter — calibrated, 60 fps, with your
spots and band edges drawn on it."

### 5.1 Goals and non-goals

Goals: every existing spectrum source (audio FFT, Icom CI-V, Flex VITA-49, FT-710) renders
through one new engine at display refresh rate, at the source's own resolution, on a calibrated
dB scale with auto range and transient-preserving averaging, with multi-slice markers,
filter-edge dragging, TNF markers, and spot / band-edge / decode overlays.

Non-goals for A: no new hardware, no IQ, no change to the Remote wire contract (the Remote keeps
the 512-bin rows through an adapter), no change to WSJT-X-parity behaviour in the FT cockpit's
audio waterfall (its gestures still set audio offsets, its default view stays 0–4 kHz).

### 5.2 Row contract v2

Replace the single normalised row with a typed frame that keeps what the producer knows:

```
SpectrumFrame {
  source: audio | civ | flex | yaesu          // unchanged vocabulary
  seq: u64                                    // sweep counter — the renderer draws a NEW row only when this advances
  t_ms: u64                                   // producer timestamp (clock-aligned rows; AetherSDR 26.9.3 went there)
  lo_hz, hi_hz: f64                           // span, absolute RF or audio passband, as today
  bins: Vec<f32>                              // native resolution: 512 audio, 475 civ, up to 2048 flex, 850 yaesu
  scale: Db { ref_dbm: f32, per_unit: f32 }   // or Relative — Flex/Icom publish calibrated dBm, audio dBFS
  slice: Option<u8>                           // which slice the span belongs to (A, for now)
}
```

Rules: producers stop normalising to 0..1 (the "UI AGC does the stretch" contract is retired);
`SpectrumFeed` keeps the three slots and the freshness rule but stores frames; `Spectrum` (the
old DTO) stays as a derived view so `get_spectrum_row`, `MiniSpectrum`, Companion/UDP and the
Remote keep working unchanged in A. The Yaesu and CI-V `ScopeSweep` already carry span and
resolution; they gain `seq` and a calibration where the rig documents one (CI-V 0–160 is
relative to the rig's REF setting — publish it as `Relative` with the ref read over CAT).

### 5.3 Producer changes

- **Audio:** expose FFT size (1024/2048/4096 already exist as `WindowN`), window choice
  (Hann today; add Blackman-Harris with the per-window gain compensation the calibrated axis
  needs), and publish dBFS, 1 Hz-normalised.
- **CI-V:** publish each completed sweep once with its `seq`; stop the 1 s "RF wins" window from
  masquerading a stale sweep as fresh — the renderer interpolates visually between sweeps, the
  producer never repeats.
- **Flex:** decode **waterfall tiles** (`0x8004`) alongside FFT `0x8003` so the waterfall is the
  radio's own, timestamped and at the radio's rate, and request `x_pixels` equal to the pan's
  device-pixel width (per-pixel trace from the radio, no client resample). This is what
  AetherSDR does and what SmartSDR users see.
- **Yaesu:** unchanged except `seq`.

### 5.4 Renderer

- **WebGL2**, not WebGPU: WebGL2 is already proven in the shipped webview on all three platforms
  (three.js Globe); WebGPU is absent on WebKitGTK and only recently on WKWebView. Design the
  renderer behind a small interface so a WebGPU path can be added without touching callers.
  Keep the canvas-2D renderer as the fallback when context creation fails.
- **Waterfall:** history lives in a GPU texture ring (R8 or R16 per bin, rows × native bins);
  each new row is one `texSubImage2D`; the fragment shader samples the ring with the current
  span mapping and the palette LUT, so pan, zoom, palette change, scrollback and resize are all
  one uniform change and a redraw — no CPU re-render of history. Reprojection across retunes
  uses the per-row `{lo_hz, hi_hz}` the history ring already stores.
- **Trace:** one value per screen pixel from all source bins covering that pixel (max for the
  peak detector, mean for the average detector), computed in the shader; never bin-skipping.
- **Cadence:** draw at display refresh (rAF, 60 fps where available; `reducedMotion` keeps its
  slower path). A new waterfall row is committed only when `seq` advances; between sweeps the
  trace shows the newest frame and the waterfall does not scroll. Row height follows the source
  rate so a 3-sweep/s CI-V scope and an 84-fps FT-710 both fill the screen at the same
  time-per-pixel the operator chose.
- **3-D (DSS):** move the ridge fill to the same texture ring (vertex displacement from the ring,
  one draw call), at full column count; keep AetherSDR's noise-floor anchoring and median-of-3
  impulse rejection (port candidates: `DssRenderer::reprojectFrequencyFrame`).
- **Budget:** < 2 ms GPU, < 1 ms main-thread per frame at 2048 bins × 2048 rows on the 1024×768
  floor; measured, not asserted (see §5.9).

### 5.5 Scale, range and averaging

- Calibrated **dBm axis** on Flex and FT-710 (Flex publishes calibrated levels; Yaesu's are
  relative and say so), **dBFS** on audio, **relative** on CI-V with the REF level labelled.
- **Auto dynamic range**: floor = row mean − 5 dB, span 55 dB (deskHPSDR's rule), with manual
  ref/range as today's G/Z sliders, per cockpit, persisted and **clamped on load**.
- **Averaging**: log-recursive, default 250 ms time constant, operator-selectable 0 (off) …
  2 s; AVG and PEAK detector modes; the 0.95-exponential smear that users complain about in
  Thetis is the explicit anti-pattern.
- **Audio FFT size / window** controls surface in the pane's ⚙ strip (Fast/Balanced/Sharp exist).

### 5.6 Markers and gestures

- **Slice markers:** every receiver the radio reports (Flex slices; Icom Main/Sub; the audio
  passband on a Hamlib rig) draws as a colour-coded VFO marker with its passband; the active
  one carries the filter edges.
- **Filter-edge drag** on the active marker commands the rig's passband (Flex `slice set
  filter_lo/hi`, Icom filter width, Hamlib `PASSBAND`), through `useScopeTune`'s coalescing.
- **TNF markers** (Flex) and the manual/auto notch (Icom) draw through the waterfall as a
  vertical band, draggable where the rig allows.
- Existing gestures stay: click snaps to signal (`tuneSnap.ts`), drag = passband box, wheel =
  zoom (with reprojection) or scrollback while paused, right/shift/ctrl in the FT waterfall.
- **Keyboard parity:** every pointer gesture has a key path (the layout contract's
  no-trap rule).

### 5.7 Overlays

- **Spots:** cluster and RBN rows (`SpotRow`, already in the Needed/BandMap pipeline) painted
  as callsign tags above the trace with age fade; click = the same QSY/prefill action BandMap
  does; density capped with the BandMap's existing collision rules.
- **Band edges by licence class:** from `privileges.rs` / `bandplan/licensed.rs`; out-of-privilege
  regions tinted, the TX-lockout rule unchanged — the overlay *shows* the gate, it does not
  replace it.
- **Decodes on the RF pan:** when an RF pan is visible beside the FT cockpit, FT8/FT4 decodes
  and the Rx/Tx offsets draw at dial + offset (this is AetherSDR's closed request #4526, worth
  shipping first).
- Overlays are **data-only layers** (a second transparent canvas as `Waterfall.tsx` already
  does for its overlay), so the GPU waterfall never re-renders for a spot change.

### 5.8 Where it mounts

- `PhoneScope` (Phone, CW) and `Waterfall` (FT, JS8, RTTY, PSK, SSTV, pop-out) are rebuilt on
  the engine; their public props and test ids are kept so the structure and stop-line suites
  stay valid.
- The FT cockpit keeps the WSJT-X audio waterfall as its default pane. **Open question (§9):**
  whether to offer the RF pan as an *additional* `CockpitPaneFrame` in the digital cockpits
  (the row contract allows it; the DATA-mode `clear_rf` would move from the engine to the
  pane's choice of slot).
- Pane sizing stays in `cockpit-panes.css` per the layout contract; the renderer sizes to its
  frame and never the reverse.

### 5.9 Verification

- **jsdom never lays out and never has a GL context.** Every unit gate stays green through a
  broken shader. Each PR under `ui/` runs `scripts/browser-probe`, and the renderer gets a
  **pixel fixture**: a synthetic frame set (a carrier, a two-tone, a noise floor step, an FT8
  slot) rendered headless in Chromium and compared against stored PNGs with a tolerance; the
  positive control is a deliberately wrong palette that must fail it.
- **Cadence probe:** feed a 3-sweep/s synthetic CI-V source and assert the waterfall advanced
  exactly the committed-row count, not the frame count — the repeated-row defect, as a test.
- **Perf probe:** the §5.4 budget measured in the browser-probe run on the floor viewport.
- **Hardware:** the 9700 for CI-V (the maintainer's bench); FT-710 and Flex through beta
  testers with the report template in §6.7.
- **Calibration control:** on a Flex, a known-level signal generator reading vs the pan's dBm
  label; on audio, a −20 dBFS tone vs the axis.

### 5.10 Risks

- WebGL context loss (sleep, driver reset) must fall back and recover without a reload. The
  Globe handles neither today (no `webglcontextlost` listener in `ui/src`), so the waterfall
  renderer introduces that handling and the Globe should adopt it.
- `x_pixels` at device-pixel width raises Flex UDP bandwidth; over SmartLink/WAN this needs
  the pan's `fps` lowered automatically (AetherSDR does this).
- Linux WebKitGTK GL is the weakest platform; Windows-first per the decision, but the
  fallback must be exercised in CI (`remote-browser` builds `ui/`, so a headless GL-less run is
  the natural shard).
- The retired "UI AGC" contract touches every producer and the Remote adapter — a wide change;
  land the frame type and the adapter first, behind a flag, then move renderers one at a time.

---

## 6. Release B — the standalone Flex client (headline minor)

**Headline:** "Nexus runs your Flex by itself — every slice on one screen, full TX, SmartLink —
no SmartSDR, SmartSDR CAT or DAX drivers, on Windows, macOS and Linux."

### 6.1 Architecture

One **FlexBackend** that is, to the rest of Nexus, three things it already knows how to consume:

1. a **CAT source** that serves the rigctld protocol to the engine, exactly as the CI-V daemon
   and the OmniRig shim do — so every cockpit, the broker on :4532 and the capability tables
   work unchanged, with a Flex-native `RigCaps` instead of a `--dump-caps` parse;
2. an **`AudioBackend`** whose capture is DAX RX over VITA-49 and whose playback is DAX TX over
   VITA-49, through the existing `TxTee` exclusive route, per slice and per channel (not channel
   1 only), with the 24 kHz ↔ 12 kHz resampler already in `flexdax.rs`;
3. a **spectrum producer** of v2 frames (§5.2) per panadapter, FFT and waterfall tiles.

It registers as a **GUI client** (`client program Nexus`, its own `client_handle`), binds
slices by handle, and owns what it creates. It is a first-class radio in the multi-radio
selector; the existing SmartSDR-CAT (2036) and native (23005) profiles stay for the
coexistence case and become the documented fallback.

### 6.2 Feature scope

| Area | In B | Later |
|---|---|---|
| Slices and pans | all slices the radio allows on one screen; per-slice mode/filter/AGC/NB/NR/APF/mute/audio gain; one pan per slice group, detachable (`DetachedPanel` exists) | diversity |
| TX surface | `xmit`, tune, RF power, TX profile select, ATU start/bypass, antenna select, TX filter lo/hi, mic/DAX TX source, processor, CW via the radio's keyer (speed, pitch, break-in), MOX/PTT interlocks, meters on the radio's calibration | PureSignal-equivalent n/a on Flex |
| Audio | DAX RX per slice, DAX TX, remote audio (opus) stream for the hosted Remote | DAX IQ out to CW Skimmer |
| Receiver extras | TNF create/move/remove, drawn by A's renderer | diversity, wideband |
| Discovery | LAN (`flexdisc.rs` exists), **SmartLink** (§6.5) | — |
| Protocol | SmartSDR API **v3 and v4** (§6.6) | — |

### 6.3 TX safety on a Flex

The radio is the keying boundary, which is new: today every keying path in Nexus is either
Hamlib PTT or a native CI-V/OmniRig command, and all of them sit behind the same gates. The
Flex client threads **every** gate through its own `xmit`/`mox`/tune commands:

- the **TX-enable latch** stays OFF at launch and remains the universal gate; `xmit 1` is never
  sent while it is off, and a latch drop sends `xmit 0` immediately;
- the **wall-clock watchdog** runs on Nexus's clock and sends `xmit 0` on expiry, with a
  readback that confirms the radio reports `tx=0` — a receipt is not RF cessation;
- **identity validation** and **licence-privilege gating** run before every keying command,
  against the *slice* that is the TX slice (`tx=1`), not the dial Nexus last commanded;
- the **stop line**: Stop TX in every Flex-driven cockpit sends `xmit 0` + stream flush and is
  outside every removable pane, verified by the existing stop-line sweeps with the Flex props
  App gives them;
- the **radio-wide DAX mic trap** (`transmit set dax=1` disconnects the mic for every client)
  is handled by setting it only for the over, restoring it on unkey and on disconnect, and
  surfacing the state as the Phone screen already does;
- a lost TCP session or a missed `ping` during TX unkeys locally and the radio's own TX
  timeout is left at its default as the backstop.

FT-mode TX/timing/sequencing semantics are untouched by this release (the modem still produces
12 kHz audio into `TxTee`); anything that changes on-air FT behaviour needs its own sign-off.

### 6.4 CAT and settings model

`RigCaps` for Flex is authored, not parsed: RX coverage from `radio` status, TX bands from the
licence table, split by slice pair, RF power floor 0, native VFO read. Per-mode power ceilings,
amplifier follow-band and the Remote `radioLevels` capability map onto slice/transmit setters
with readback, under the same owner transaction rules `ARCHITECTURE.md` describes.

### 6.5 SmartLink

A research-then-build item. What is known: SmartLink is Auth0 login + a TLS relay that hands the
client the radio's public endpoint and a connection token; AetherSDR implements it in C++
(`src/core/backends/flex`), portable under GPLv3. What is not known: whether Flex's terms permit
third-party SmartLink clients without registration, and how it interacts with Nexus's own hosted
Remote (two WAN paths to one radio; the Remote's station room already brokers controller leases).
Ship LAN first; SmartLink in the same release only if the terms check clears early.

### 6.6 SmartSDR v3 vs v4

The protocol differences between v3 (6000 series) and v4 (8000 / Aurora) are **not established
in this tree** and were not fully established by the research pass. Prerequisite: a tester
census (model, firmware, SmartSDR version) and a protocol diff from FlexLib's public source
(licence to be checked per file) and the `smartsdr-api-docs` wiki. The client negotiates on
`version` and keeps one code path with versioned capability flags.

### 6.7 Verification without a radio on the bench

- A **simulator** in-tree: a SmartSDR-protocol server that replays recorded status/VITA
  sessions and answers commands (AetherSDR has a `sim` backend to port from; FlexLib's test
  fixtures if licence allows). Every Flex unit test runs against it; the positive control is a
  simulator fault injection (reordered VITA, a dropped `ping`, `tx=1` that never arrives) that
  must trip the matching guard.
- **Tester builds** carry a prerelease suffix (`1.17.0-test1`), never a public patch number
  (CLAUDE.md versioning rule). A **report template** ships with them: radio model, firmware,
  SmartSDR version, network path (LAN/SmartLink), what was tried, what the radio showed vs what
  Nexus showed, with the Flex log excerpt. The native path's "never run on hardware" banner in
  `docs/rigs/flexradio.md` comes down only on a filed report per feature.
- **Order of hardware verification:** discovery → bind → pan/waterfall → DAX RX → CAT readback
  → DAX TX into a dummy load → TX surface → SmartLink.

### 6.8 Risks

- Flex firmware changes break third-party clients at every major; the simulator must be
  re-recorded per firmware line.
- Multi-client slice ownership (SmartSDR and Nexus both bound) is a common support case;
  the client must display *whose* slice is whose and never steal one.
- The "no DAX drivers" claim holds only on the native path; the Windows DAX devices remain the
  bridge for other programs (JTAlert, CW Skimmer) until DAX IQ/audio export exists.

---

## 7. Later — IQ backends (outline only)

Not in A or B. Recorded so B's abstractions leave room for it:

- **openHPSDR Protocol 1** (Hermes-Lite 2, Red Pitaya, older ANAN): a documented UDP format
  with no library (TAPR "Metis – How it works", HL2 wiki); HL2 is the beachhead (~1000 units,
  $339). Nexus would own the DDC → demod → 12 kHz chain and a DUC TX chain; the TX gates in
  §6.3 apply to a boundary that keys in microseconds. Port candidates: AetherSDR
  `backends/hl2/{MetisProtocol.h, MetisClient, Hl2RxDsp, Hl2TxDsp, WdspChannel}`; hpsdr-rs
  (GPL-2.0-or-later, Rust) for shape.
- **Protocol 2** (ANAN G2 line): documented ("openHPSDR Ethernet Protocol v3.8"), more complex.
- **Receive-only USB** (RTL-SDR, Airspy, HackRF RX): via `seify`/`soapysdr` crates
  (Apache/Boost). **SDRplay** is a closed API — runtime `dlopen` only, never bundled.
- **The roadmap's full-band CW skimmer** needs exactly this IQ path (or Flex DAX IQ from B).

---

## 8. Sources to port and their licences

Porting C/C++ is approved; the CLAUDE.md gate is per-file headers, NOTICE entry, README credit,
GPL-3.0-only compatibility, before commit.

| Source | Licence (as found) | What to port | Verify before vendoring |
|---|---|---|---|
| AetherSDR (`aethersdr/AetherSDR`) | GPL-3.0 | Flex backend (client handle, slices, pans, waterfall tiles, DAX, SmartLink), `DssRenderer::reprojectFrequencyFrame`, sim backend | per-file headers (already done twice) |
| WDSP (Warren Pratt NR0V, via TAPR) | labelled GPL-2.0; AetherSDR ships it as GPLv3-compatible | display analyzer (one value per pixel), NR/NB later | **the "or later" clause per file — unconfirmed** |
| FlexLib (FlexRadio) | unknown here | protocol constants, v3/v4 diff, fixtures | licence per file; may be reference-only |
| deskHPSDR / piHPSDR | GPL-3.0 | averaging and auto-range rules (behaviour, not code) | — |
| hpsdr-rs (G0ORX) | GPL-2.0-or-later | P1/P2 shape, later | — |

---

## 9. Open questions

1. **RF pan in the digital cockpits** as an additional pane, or Phone/CW only in A? (§5.8)
2. **SmartLink terms** for third-party clients; and the relationship to Nexus Remote. (§6.5)
3. **SmartSDR v4 protocol diff** — tester census first. (§6.6)
4. **WDSP's licence clause** — decides whether its analyzer is ported or re-derived. (§8)
5. **Remote follow-up release:** widen the stream contract to v2 frames, or keep 512 rows and
   send only the trace? Relay bandwidth decides.
6. **Which Flex models the beta testers run** (6400/6600/6700 vs 8600/Aurora) — sets the
   simulator recordings.

---

## Appendix A — competitive notes and sources

Collected 2026-10-01. Sites blocked by the research box's proxy are marked *(snippet)*; those
facts came from search snippets and should be re-checked before being relied on.

**ZeusSDR / OpenHPSDR Zeus.** GPL-2.0-or-later engine (`Zeus-SDR/station-engine`, C#/.NET 10,
P1/P2, WDSP, HTTP/WebSocket; KB2UKA, N9WAR); proprietary client; v0.11.0 (2026-07-07) added
one-button PureSignal, WDSP 2.00, detachable workspaces, FT8 Auto CQ, logbook, WebGPU 3-D
panadapter, 0–60 MHz wideband *(snippet)*. Apache Labs fork: ANAN Core (`n9bc/zeus`). Client
price not found.

**Zeus Radio** (zeusradio.com / hfrelectronics.com): closed Windows console for the ZS-1; HPSDR
P1, HiQSDR, ExtIO; v2.9.3; RX licence ~2000 RUR, TX ~6000 RUR *(snippet)*. Not the benchmark.

**AetherSDR** (`aethersdr/AetherSDR`): GPL-3.0, C++20/Qt 6.12; 228 stars, 125 forks, 3202
commits; weekly releases v26.8.4 … 26.9.5 (2026-09-27); Linux AppImage, signed macOS DMG,
Windows installer. Backends under `src/core/backends/{flex,hl2,anan,icom,rtl,sim}`. HL2 TX
merged PR #5747 (2026-09-17). Panadapter: QRhi, up to 60 fps, DSS since v26.7.1, up to 8
detachable pans, TNF through the waterfall (PR #5679), one point per pixel via WDSP analyzer
(PRs #5814, #5920), WSJT-X overlay request #4526. eHam review page 16326.

**SmartSDR**: closed; v4 Basic free, SmartSDR+ paid yearly; unlimited slices/pans; iOS/Windows;
community thread 8032305.

**Thetis** (`ramdor/Thetis`, archived 2026-04-02 at 2.10.3.13; `mi0bot/OpenHPSDR-Thetis` for
HL2): GPL-2.0-or-later (header verified), Windows .NET 4.8, SharpDX. **piHPSDR** (`dl1ycf`):
GPL-3.0, v3.0 with PipeWire, DX cluster, client/server, TCI. **deskHPSDR** (`dl1bz`): GPL-3.0,
250 ms log-recursive averaging, auto range (mean −5 / +55 dB), cluster + RBN on the pan.
**SDR Console**: closed, Windows, free for hams; TX on ANAN/HL2/Lime/Pluto; "no diversity" is
the recurring complaint. **ExpertSDR3**: closed, free, EE hardware only, exposes TCI (MIT spec).
**SDR++**: GPL-3.0, RX-only upstream; SDR++Brown adds HL2 TX. **SDRangel**: GPL-3.0.
**Quisk**: GPL-2 (or-later unverified). **Gqrx / CubicSDR**: RX-only. New in 2026: NereusSDR
(GPL-3, QRhi), hpsdr-rs (Rust, GPL-2+), KymoSDR, thetis-on-the-web (issue #18 is the best
written averaging spec).

**Hardware / protocol paths** (TX-capable unless noted): openHPSDR P1 — UDP 1024, discovery
`0xEFFE02`, 1032-byte frames, 48–384 kHz, documented; P2 — documented PDF in
`TAPR/OpenHPSDR-Firmware`; Hermes-Lite 2 — P1, ~1000 built *(snippet)*; ANAN G2 — P2; Red
Pitaya — Pavel Demin P1 emulation, MIT; SunSDR — no raw protocol, TCI via ExpertSDR3 only;
Flex — SmartSDR TCP + VITA-49 UDP, API docs public; Icom IC-7610/7760 — IQ only through a
Windows-only ExtIO DLL, scope via CI-V; Elecraft K4 — pan stream not openly documented
(`dc0sk/K4remote`, Rust GPL-3, implemented it clean-room); Pluto — libiio LGPL; LimeSDR —
Apache; HackRF — BSD, half-duplex; RTL-SDR/Airspy/SDRplay/KiwiSDR — RX-only (SDRplay API
closed). SoapySDR core — Boost 1.0.

**Rust ecosystem**: `soapysdr` (Apache-2.0 / Boost), `seify` and `futuresdr` (Apache-2.0),
`rustfft` (MIT/Apache), `hpsdr-rs` (GPL-2.0-or-later), `radio-utils-protocol` (MIT/Apache, young),
`rtl-sdr-rs` (MPL-2.0). No GPL-2-only crate in the set; the only closed path is SDRplay.
