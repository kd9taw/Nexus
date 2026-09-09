# Nexus 1.9.1

*Released 26 August 2026 — a transmit-path fix release on top of 1.9.0*

Three things in 1.9.0 could put your signal somewhere you did not intend. None of them will have
been obvious from your side of the radio, which is exactly why this is worth taking.

If you are on 1.9.0, update. If you are on 1.8.1 or earlier, go straight to this one.

---

## RTTY: the licence check could be wrong by nearly 2 kHz

1.9.0 fixed a real problem — RTTY used to transmit on a fixed tone no matter where you had
netted, so you answered on a frequency nobody was listening on. That fix was right.

What went with it: the licence check still worked from the old fixed tone. So when you click the
waterfall to net onto a station, your signal moves and the check did not move with it. At the
extremes it was out by about 1.9 kHz.

That does not matter in the middle of a band. Near the bottom of a segment it is the difference
between legal and not — and because AFSK puts your signal *below* the dial, netting **up** moves
your signal **down**, which is the direction you reach for when answering a station low in the
passband.

The check now works from the tone you are actually sending.

## RTTY: Reverse did nothing on transmit

If you run a rig in USB data — an IC-9700, for instance — and have Reverse switched on, receive
honoured it and transmit did not. It was being applied twice, and the two cancelled out. You
decoded the other station perfectly and answered with your tones the wrong way round, so they
saw nothing.

It applies once now, and receive and transmit agree.

## Working split, in both directions

The licence check judged your **receive** dial. Under split that is not the frequency you are
transmitting on, and getting that wrong went both ways.

**It locked you out of a legal contact.** Receiving a DX station in a segment you may not
transmit in, transmitting where you may — the ordinary way DX is worked, since expeditions sit
in the quiet part of the band and listen up where the pile-up can answer — got you a red TX lock
even though your transmit frequency was fine.

**And it let through one that was not.** A legal receive frequency with your transmit VFO parked
somewhere you may not use would key without a word.

Nexus now judges the frequency your signal actually leaves on. When it cannot tell where that
is, it refuses rather than guessing — a refusal you can see and fix beats a transmission you
cannot.

## CW and Operate have a real split control

They only ever *displayed* that split was on. Phone has had a working one for a while; now all
three do, so you can set up "work him up 5" from the cockpit you are actually in.

## Nexus can follow the radio's own split

New in Settings, off by default. Turn it on and Nexus follows a split you set at the front panel
instead of only one it set itself.

It is offered only where your radio can answer the question honestly, and **Nexus asks the radio
rather than asking you**. Some radios cannot report split without being made to switch VFOs to
find out — on those the option is not offered at all, because the asking is the damage. If it is
greyed out for you, that is your radio's CAT, not a judgement about you.

---

## Downloads

Windows · Linux AppImage · Linux .deb · Raspberry Pi (bookworm and trixie) · macOS on Apple
Silicon.

73 — KD9TAW
