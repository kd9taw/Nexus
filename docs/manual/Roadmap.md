# Roadmap

> **Forward-looking.** The **Phase 2** items on this page are **planned / not-yet-built**, not a description of current behavior; the "What's done today" section is. For what Tempo does *today*, see [Operating Guide](Operating-Guide.md) and [Tiers FT1 vs DX1](Tiers-FT1-vs-DX1.md). The authoritative engineering list is [`docs/ARCHITECTURE.md`](https://github.com/kd9taw/tempo/blob/main/docs/ARCHITECTURE.md) §11.

---

## What's done today

The full application is feature-complete and runs on Windows: all operating modes (Chat, QSO, Field Day), messaging, store-and-forward, broadcast, rig control, WSJT-X/PSK interop, settings, and themes; both the FT1 fast tier and the DX1 robust tier are wired end-to-end; the headless test suite is green; and there's a clean Windows installer. Shipped in **v0.2.0 (beta)**.

Two items that were Phase 2 are now **live end-to-end**:

- **IR-HARQ (RV soft-combining)** on FT1, on by default. A frame that fails to decode standalone (RV0) is buffered and joint-turbo-combined with its retransmissions (RV1/RV2), each carrying a distinct Costas sync and punctured LDPC parity. RV detection runs through a coherent CPM-Costas discriminator (>99% accurate, <1% false to −11 dB) and the QSO sequencer drives RV escalation on implicit NAK/ACK. Measured: combiner +1.3 dB AWGN and +3.2 dB under 1 Hz/1 ms fading (3-TX); through the full live pipeline ~ +2.5 dB threshold shift and ~2× QSO completion in the −11…−13 dB zone. UI adds a HARQ.RVn decode badge, a HARQ on/off toggle, and a session rescue counter.
- **Full-band DX1 receive search** — the DX1 receiver now decodes **every** signal across 200–2900 Hz per slot (coarse chirp-correlation carrier sweep → median-threshold peak-pick → CRC-14-gated decode per survivor, ~3–4 s/slot), like FT1's Costas search. `rx_offset_hz` is now just a waterfall marker / TX-pairing hint.

The waveforms are **validated by simulation and Windows cross-build only** so far (FT1 AWGN 50% ≈ −15 dB; DX1 ≈ −18.6 dB AWGN with a ~3.7 dB fading penalty) — including the IR-HARQ and full-band gains above. On the cross-build: all modem self-tests, `tempo.exe`, and the NSIS installer build clean, and 5/5 Windows test exes pass. That sets up the #1 item below.

---

## Phase 2 — planned

### 1. On-air validation *(the gating item)*

The hard remaining gate: **decode-rate-vs-SNR on real paths.** Simulation says the modems *should* work; only operators on the air can confirm they *do*. Honest on-air reports — band, dial, mode/tier, distance, conditions, decodes vs. expectations — are the single most useful contribution. This blocks treating Tempo as operationally reliable.

### 2. DX1 depth and breadth

- **Lower-rate LDPC** for **deeper DX1 thresholds** (more reach on the most marginal paths).
- **Wider DX1 variants and multi-slot stacking.**

### 3. The FT8/FT4 tier

A third tier alongside FT1/DX1. The `Tier::Ft8` variant exists and the FT8/FT4 DSP internals are already compiled into `libft1`, but **no decode pipeline is wired** — so there's no working FT8 tier yet. Wiring it up is Phase 2. See [Tiers FT1 vs DX1](Tiers-FT1-vs-DX1.md) and [FAQ](FAQ.md).

### 4. macOS / Linux desktop builds

The headless Rust core already builds and tests on Linux; the **Tauri desktop shell** is currently packaged for Windows only. macOS and Linux desktop builds are planned.

---

## How to help move it forward

The fastest way to advance the roadmap is to **get Tempo on the air and report what you hear** — that's item #1 and it unblocks everything operational. File issues and on-air reports at <https://github.com/kd9taw/tempo>; code/doc contributions are welcome too (see [`CONTRIBUTING.md`](https://github.com/kd9taw/tempo/blob/main/CONTRIBUTING.md) and [Building from Source](Building-from-Source.md)).
