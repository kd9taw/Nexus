# Tiers: FT1 (Fast) vs DX1 (Robust)

Tempo has two waveform tiers, switched with the **Fast · Robust** toggle in the top bar. The toggle is **never silent** — the active tier is always shown, because the waveform changes T/R timing, occupied bandwidth, and on-air etiquette.

Both tiers carry the **same 77-bit messages**, so Chat, QSO, Field Day, store-and-forward, and broadcast all work identically on either. You don't pick a modem mid-conversation; you pick **Fast** or **Robust** and keep talking.

---

## The short version — which one do I pick?

| Use **Fast (FT1)** when… | Use **Robust (DX1)** when… |
|--------------------------|----------------------------|
| You want **conversational pace** (4 s round trips). | The path is **disturbed** — multipath, Doppler, auroral, polar, deep fading. |
| Conditions are reasonable — regional NVIS or good-condition national. | You need **national reach on a marginal path**. |
| You're running **Field Day rate**. | You're doing **store-and-forward** to a distant station. |
| You're on a **roomy** band. | You're on a **cramped** band (17 m, 12 m) — DX1's ~50 Hz fits where FT1's ~150 Hz doesn't. |

Start on **Fast**. If decodes get flaky on a long or disturbed path, switch to **Robust**.

---

## The physics tradeoff

Weak-signal text faces a tradeoff that is **fundamental, not an engineering shortfall**: **cycle time vs. weak-signal reach**. FT8/JS8Call-class modes are extremely sensitive but slow — a rigid 15-second slot plus multi-frame messages makes a round trip on the order of ~30 s. You can shorten the cycle, but every second you take out of the integration window costs sensitivity. There is no single waveform optimal at both ends — hence two tiers.

---

## Fast — FT1

- **4-CPM** continuous-phase modulation (h = 1/2, BT = 0.3), LDPC(174,91) FEC, iterative turbo equalization, with **live incremental-redundancy HARQ (IR-HARQ)** — on by default. A frame that fails to decode standalone (RV0) is buffered and joint-turbo-combined with its retransmissions (RV1/RV2), which the QSO sequencer escalates through (RV0→RV1→RV2) until it decodes. Simulated gain: **~+2.5 dB threshold shift and ~2× QSO completion in the marginal −11…−13 dB zone.** A HARQ.RVn badge shows how many redundancy versions were combined.
- **4 s T/R** period (~3.5 s of waveform inside the 4 s frame), ~150 Hz occupied bandwidth.
- **Coherent** — it extracts the most information per second of air time, so it's the fast, conversational tier. But coherence is exactly what multipath/Doppler spreading destroys, which is where DX1 takes over.
- **Simulated AWGN 50%-decode threshold ≈ −15 dB.**

## Robust — DX1

- **Non-coherent 8-FSK** (M = 8 orthogonal tones, 3 bits/symbol, Gray-coded) with the **same LDPC(174,91)** FEC, soft-decision decoded.
- **6.25 Hz baud / tone spacing → ~50 Hz occupied bandwidth.** A linear-chirp preamble syncs time and frequency at the receiver.
- **Full-passband receive.** DX1 RX decodes **every** signal across 200–2900 Hz per slot (like FT1's Costas search), not just the tuned carrier — your RX offset is now just a waterfall marker / TX-pairing hint.
- **15 s T/R** period — slower, but it never relies on carrier phase, so it survives the fading that collapses coherent modes.
- **Simulated AWGN 50% threshold ≈ −18.6 dB**, with only a **~3.7 dB** penalty under per-symbol Rayleigh fading — where FT8-class coherent modes lose **10+ dB**. That small fading penalty is the entire reason the mode exists.

---

## ⚠ These numbers are simulation, not on-air

The thresholds above (FT1 ≈ −15 dB, DX1 ≈ −18.6 dB, ~3.7 dB fading penalty) are from **simulation only** — AWGN and Rayleigh-fading sweeps in the test harness. They have **not yet been confirmed on the air**, and that on-air decode-rate-vs-SNR validation is the project's remaining gate. Don't read these as guaranteed on-air sensitivity. If you get Tempo on the air, honest decode-rate reports are exactly what the project needs (see the honest beta note on [Home](README.md)).

---

## Known limits today

- **No FT8/FT4 tier yet.** The FT8/FT4 internals are compiled into the modem, but no decode pipeline is wired — that tier is **Phase 2**. See [Roadmap](Roadmap.md).

---

## See also

- [Operating Guide](Operating-Guide.md) — the tier toggle in the top bar and how the modes use it.
- [Frequency Plan](Frequency-Plan.md) — where DX1's narrow signal is the better fit (cramped bands).
- [Architecture and Protocol](Architecture-and-Protocol.md) — how the engine modulates/decodes per tier.
- Developer deep-dive: [`docs/ARCHITECTURE.md`](https://github.com/kd9taw/tempo/blob/main/docs/ARCHITECTURE.md) §1–§6.
