# Nexus 1.6.0 — FT2, and satellites that stay put

*2026-08-16*

Two headlines, both proven on the air before this release was cut.

**Nexus speaks FT2.** The fast slotted mode from the Decodium community (IU8LMC's WSJT-X
fork) joins the FT dropdown: FT4 with a halved symbol time — 3.75-second periods, about
167 Hz wide, decoding to −10.8 dB — for when band turnover matters more than the last few
dB. It follows Decodium's own band plan from 160 m to 23 cm, runs the same auto-sequencer
as FT8/FT4, and its first QSOs were made (and its timing then *corrected against Decodium's
own source* — answers now land in the very next slot, and transmissions start at the slot
boundary exactly as every other FT2 station's do) before this shipped. Logs as MFSK/FT2 so
LoTW and the online logbooks accept the record. Built from Decodium's GPL modem sources —
see the credits.

**Satellite passes hold their split.** The bug where keying a V/V pass yanked the transmit
frequency back to the downlink half a second in is dead — traced on a real CI-V capture to
a stale keyed-state poll, and fixed at the root: the rig's own reported frequency is now the
evidence, the poll runs five times a second during passes, and the pass pins its transponder
row at AOS. The uplink **mode** is commanded too (no more inherited LSB on an FM bird), a
V/V pass steps the IC-9700 out of its crossband-only satellite mode and back, and FM birds
finally key their CTCSS tone — the ISS repeater opens.

**Take this one if you work satellites, want a faster digital mode, or use the Phone/CW
scope.**

---

## The scope, round two

1.5.0 made signals rise and fall honestly; 1.6.0 makes the geometry match your rig. The
Phone scope **centers your dial** — a labelled line, voice extending right on USB and left
on LSB, with the occupied sideband getting three-quarters of the panel. The CW scope
**centers your sidetone pitch**, so a zero-beat station sits mid-window under the hairline.
And both windows now **follow your radio's filter** — a 500 Hz CW filter fills the display
instead of floating inside dead margins, and Phone's new Auto span tracks your bandwidth.

## SSTV: compose, and the smart ident

Lay **text over your transmit picture** — one-click CQ, 73, and Reply cards (Reply fills in
the other station's call from the newest FSK ID heard), plus free text in two styles: the
ident's own pixel font, or big outlined banner text. Eight colours, four sizes, drag to
place. And the corner ID plate **retires itself when your text carries your callsign** —
your layout identifies the picture; delete that text and the plate returns. The gallery
gained a pencil: load any received picture into the composer and answer it.

## Also

**Every waterfall is now a panel** — hide the scope strip in any cockpit from the ⊞ menu,
on by default, remembered. **Grid squares join the watch list**: enter `FN31` or `EM7*` in
Settings ▸ Spots & Alerts and get the loud alert the moment a station decodes from there —
on every band, because a square you asked for by name is never chatter. The band roster
stops aging stations out too fast on short-period modes. And the setup wizard's example
callsign is nobody's callsign.

Windows, Linux, Raspberry Pi, **and macOS** (Apple Silicon — first added to 1.5.0, now a
first-class platform in every release).

Thanks this release: **ON8ST** for the macOS groundwork and seven merged contributions in
1.5.0, and **IU8LMC / the Decodium project** for FT2's modem sources (GPL-3, full
provenance in NOTICE).

Full detail, as always, in the [CHANGELOG](../CHANGELOG.md).

73 — KD9TAW
