# Nexus 1.1.0 — the radio lands in the right mode every time, radio sharing that holds, and the awards page earns your trust

*2026-08-10*

A band change can no longer leave the rig in the wrong mode, sharing your radio with
VarAC or WSJT-X now survives everything Nexus does, satellite contacts tag themselves
for LoTW, the awards page computes what ARRL actually grants, and a new-user's first
ten minutes — installer, wizard, first radio — got a full pass.

**Everyone should take this one.** The wrong-mode-after-band-change bug has been in
every release since 1.0.1, and it is the kind you blame on your radio.

---

## The rig follows your click — every band, every section

Modern rigs keep their own per-band memory of the last mode used there, and on a
band-crossing retune that memory could silently override the mode Nexus had just
commanded: pick 12 m in the CW section and the rig lands in DATA-U because FT8 was the
last thing you ran on 12 m. Nexus believed its command stood and never checked. Every
band-crossing retune now reads the rig's real mode back and re-asserts your choice once
if the rig overrode it — your click wins, deterministically, and the FTdx10 CW pitch-walk
fix from 1.0.1 stays intact.

Two doors into the same wrong room also closed: a Settings save made after switching
sections could quietly revert the rig to the old section's mode, and picking an SSTV
frequency left the *previous* screen's mode policy in charge (arriving from RTTY put
20 m SSTV in DATA-L). SSTV now claims its section when you pick a frequency: you listen
and talk in plain USB/LSB, and the DATA mode is only commanded while a picture is
actually going out.

## Radio sharing that stays up — and is on by default

The "Share this radio" address is now answered by Nexus itself from live radio state,
instantly, instead of forwarding to the underlying rigctld on your busy serial link.
VarAC's "Input string was not in a correct format", the disconnects on every Test CAT
and settings save, the long-verb spellings — all gone against the new address. Shared
programs can key the rig (turn that off in the same block for read-only), and every
Nexus transmit safeguard still applies to them. Also: the helper servers now bind
localhost only, matching what the UI always promised.

## Satellites and awards

Log a QSO while a transponder is held and your dial is on the bird's downlink, and the
record gets `PROP_MODE=SAT` plus the LoTW designator, written as the pair TQSL demands —
your pass QSOs count toward Satellite VUCC and upload as creditable satellite contacts
with nothing to edit. (ISS is the one honest exception; add its fields by hand.)

The awards page was put next to an official ARRL account with a 31,000-QSO log and every
difference chased down: 5-Band DXCC and 5-Band WAS now judge per band the way ARRL does,
satellite QSOs no longer inflate band DXCC, VUCC is the real 50 MHz-and-up award with
per-band thresholds instead of a grand total, and IOTA's checkmark follows IOTA's rules
(cards and Club Log, never LoTW).

## Working stations

Double-clicking a station in the Call Roster now moves your RX and TX to where that
station was actually heard — exactly like a Band Activity double-click, and Hold Tx is
respected. Clicking a spot no longer bounces the rig off a default frequency on the way.
Switching FT8↔FT4 clears the stale station list so you can't be handed a wrong-cycle
answer, and the cycle the app picks for you is now visibly underlined.

## The first ten minutes

The installer shows the WebView2 step instead of appearing frozen, speaks eight
languages, and the docs no longer tell you to install anything by hand. The setup wizard
finds your radio on entry — scanning USB and network, probing generic cables at real
baud rates, asking *which radio is this?* when the answer is ambiguous instead of
guessing — keys it via CAT instead of the old silent VOX default, hands out sound cards
one-per-radio so two identical Yaesus can't share one, and ends on the live setup-health
strip. A second radio is one button. And the goals quiz is gone: everything starts on.

## Also in this release

Field mode for operating in daylight (one tap, high contrast, bigger type), backup and
restore of your whole setup to a file, per-operator logging and per-operator ADIF export
for shared POTA/Field Day stations, the band map tunes by click and scroll, stroked
callsigns resolve in the callbook, KG4 calls stop being filed as Guantanamo, memory
channels and watchlists moved to real config storage that survives reinstalls, the
Linux .deb declares its full dependencies, and rigs keyed by a Digirig-class interface
no longer transmit the moment Nexus connects — while rigs on plain USB CAT cables keep
their hardware handshake exactly as before.

---

Full details in the CHANGELOG. Windows, Linux and both Raspberry Pi builds:
https://hamradiotools.io/nexus/
