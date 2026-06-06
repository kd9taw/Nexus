# FAQ

Short, honest answers. For depth, follow the links.

---

### Is Tempo on the air yet?

**Not validated on-air.** Tempo is **beta**. The FT1 and DX1 waveforms are **validated by simulation only** (AWGN + Rayleigh-fading sweeps). The simulated thresholds (FT1 ≈ −15 dB, DX1 ≈ −18.6 dB AWGN, ~3.7 dB fading penalty) have **not yet been confirmed on the air** — that on-air decode-rate-vs-SNR validation is the project's hard remaining gate. The app itself is feature-complete and runs on Windows; the waveforms just haven't been proven over real paths. If you get it on the air, honest decode reports are the single most useful contribution. See [Tiers FT1 vs DX1](Tiers-FT1-vs-DX1.md).

### What rigs and operating systems are supported?

**OS:** Windows is the primary (and currently only) desktop target. macOS/Linux desktop builds are on the [Roadmap](Roadmap.md). **Rigs:** any rig you can key by **CAT** (Hamlib `rigctld`, with a 56-model dropdown), **serial RTS/DTR**, or **VOX**. CAT works offline because the installer bundles Hamlib. See [Rig and Audio Setup](Rig-and-Audio-Setup.md).

### Does Tempo replace WSJT-X / JS8Call?

No — it's a different tool for a different job. WSJT-X/JS8Call are excellent at what they do. Tempo targets **fast conversational text** (FT1) plus **fading-resilient reach** (DX1) with a **chat-first** interface, for the off-grid/preparedness use case. It also **interoperates** with the WSJT-X ecosystem (UDP API, PSK Reporter), so JTAlert/GridTracker/N1MM+/loggers work alongside it.

### Why does Tempo use its own frequencies?

Tempo's waveform is **new**, so transmitting on the FT8/FT4/JS8/WSPR/PSK watering holes would cause mutual QRM. Tempo ships **dedicated calling frequencies** placed clear of those holes (and of CW, FM calling, APRS, satellite, and repeater segments). See [Frequency Plan](Frequency-Plan.md).

### Is it legal to operate on those frequencies?

The defaults were chosen to fall inside **US General-class data privileges** (judged on the emission, ~1.5 kHz above the dial) and clear of CW calling frequencies — but they are **proposed, editable defaults, not regulatory channels**. **You are responsible for operating within your own license privileges and local/national band plan.** US Technician operators: only the **10 m** and **6 m** Tempo channels are within your HF/VHF privileges. R1/R3 operators must re-vet against their national plan. See [Frequency Plan](Frequency-Plan.md).

### What's the difference between Tempo and FT8?

FT8 is a single, very sensitive but **slow** mode (rigid 15 s slots). Tempo offers **two tiers**: **FT1** (4 s, coherent, conversational) and **DX1** (15 s, non-coherent, built for fading immunity), with a chat layer on top. DX1's whole point is surviving fading that costs coherent modes like FT8 10+ dB — at a simulated ~3.7 dB penalty. See [Tiers FT1 vs DX1](Tiers-FT1-vs-DX1.md).

### Does IR-HARQ actually help? Can I turn it off?

**Yes — it's live and on by default.** When a frame fails to decode on its own, Tempo buffers it and **joint-turbo-combines** it with the RV1/RV2 retransmissions (each carrying distinct parity), so a QSO that would have stalled can still complete. Through the full live pipeline this buys **~+2.5 dB of threshold shift and roughly 2× QSO completion in the −11…−13 dB zone** (simulated). There's a **HARQ on/off toggle** (default on), a `HARQ.RVn` decode badge, and a session rescue counter. Like everything else, the gain is **simulation-validated, not yet confirmed on the air**. See [Tiers FT1 vs DX1](Tiers-FT1-vs-DX1.md).

### Does DX1 hear the whole band now?

**Yes.** DX1 RX now decodes **every signal across 200–2900 Hz** each slot — a full-passband Costas-style search, like FT1. The RX offset is just a waterfall marker / TX-pairing hint now, not a single tuned carrier. A slot takes ~3–4 s to scan. (Still beta: on-air decode rates remain unverified.) See [Tiers FT1 vs DX1](Tiers-FT1-vs-DX1.md).

### Can I use Tempo for Field Day?

Yes. Tempo has a native **Field Day** mode: the `CALL CALL [R] CLASS SECTION` exchange in one frame, a dupe-checked log with section multipliers and 2-points-per-QSO scoring, and **ADIF / Cabrillo** export. Contacts are **operator-initiated** by design, because ARRL Field Day prohibits fully-automated QSOs. See [Operating Guide](Operating-Guide.md).

### Is the installer safe? Why does Windows warn about it?

The installer is a per-user offline build that bundles WebView2 and Hamlib. Because the published binaries are **cross-compiled and unsigned**, Windows **SmartScreen** may warn ("Windows protected your PC") — click **More info → Run anyway**. This is expected for an unsigned beta. If you'd rather verify it yourself, you can [build from source](Building-from-Source.md). Treat all beta binaries with appropriate caution.

### When is the FT8/FT4 tier coming?

It's **Phase 2**. The FT8/FT4 internals are already compiled into the modem library, but **no decode pipeline is wired** yet, so there's no working FT8 tier today. See [Roadmap](Roadmap.md).

### macOS / Linux desktop builds?

Phase 2. The headless Rust core already builds and tests on Linux, but the Tauri desktop shell is currently packaged for Windows only. macOS/Linux desktop builds are on the [Roadmap](Roadmap.md).

### Why does Tempo "start passive"? Will it transmit on its own?

Tempo **starts passive (hunt-and-pounce)** — it listens and decodes but **won't transmit until you act**. The CQ **beacon is opt-in** (off by default). You can also globally drop to listen-only with the **Monitor / Muted** control, and stop any transmission instantly with **Stop TX**. See [Operating Guide](Operating-Guide.md).

### How do I report a bug or an on-air result?

Open an issue on the repo: <https://github.com/kd9taw/tempo>. For on-air reports, include band, dial, mode/tier, distance, conditions, and what you saw vs. expected — that's exactly the data the project needs.
