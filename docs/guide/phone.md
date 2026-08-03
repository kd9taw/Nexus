# Phone (SSB)

The Phone cockpit is a traditional rig-panel experience for voice operating —
SSB on HF and FM for VHF/UHF and repeaters. It doesn't try to reinvent voice —
you talk on the rig's own mic — but it gives you the shack-monitor conveniences a
modern app should: live dial read-back, a fast colored bandscope, a voice keyer
for the calls you make over and over, and crash-safe QSO recording, all with your
logbook and license privileges wired in.

Phone is an opt-in section. Turn it on in the first-run wizard's "which modes?"
step or in [Settings ▸ Features](settings-reference.md#features).

<!-- TODO: capture screenshot — the Phone cockpit — dial read-back, bandscope, PTT, voice keyer -->

## The tour

**Live dial read-back.** The cockpit polls the rig about every 750 ms: spin the
VFO knob and the displayed frequency follows. On SSB the sideband is automatic —
LSB below 10 MHz, USB above — so the rig lands on the right sideband for the band.
A mode badge shows what the cockpit has the rig on (LSB / USB / FM).

**The bandscope** is a fast (~30 Hz) colored display split into a panadapter
trace and a scrolling waterfall, with per-frame AGC so signals stay visible as
conditions shift. **Span chips** above the scope — Full / Voice / Low / High —
zoom the view to the part of the passband you care about.

**RF power slider.** Wired to CAT and *follows the rig*: turn the rig's power
knob and the slider tracks it (it won't sit lying at 100%). Your drags win while
you're dragging.

**⊞ Panels** in the header shows and hides the panes under the scope. An entry
that cannot show you anything on your setup is listed but greyed, with the reason
on it — it goes live by itself when your station can render it:

- **Rig Scope Controls** command the *radio's own* panadapter (span sets the
  hardware sweep width, ref sets weak-signal visibility), so they appear only
  while an Icom CI-V or FlexRadio scope is streaming. On the audio bandscope
  there is nothing to command, and the entry says so. The span chips over the
  scope are a different thing: they zoom what is already on screen.
- **DSP Functions** (NB / NR / Notch / COMP / VOX) and **RX DSP Levels** (NR
  level, AGC) only ever offer what your radio reports over CAT. A rig that
  reports none of them has nothing for those panes to hold, and the entries say
  so rather than hiding an empty pane behind a live checkbox.
- **TX Meters** (SWR / ALC / PO / COMP) read on transmit, so the panel is empty
  while you listen. The entry stays tickable and says when it reads.
- **Voice Keyer** hides the F-key message pane if you work with a mic and never
  use it. Unticking it stops a voice message that is playing — the entry says so
  before you tick it. Nothing else changes: PTT, Tune and Stop TX are not panels
  and have no entry, so no layout you save can put them out of reach.

A greyed entry is still a tab stop: it takes keyboard focus and reads its reason
with the panel name. If the list outgrows your window it scrolls inside the menu,
so **Undo last change** and **Reset layout** stay reachable.

<!-- TODO: capture screenshot — the bandscope with the Full / Voice / Low / High span chips -->

## Core workflows

### Get on the air and make a call

1. Set the band and frequency — type it, pick a band-plan channel, or just spin
   the rig's knob and watch the read-back follow.
2. **Push to talk** one of three ways:
   - hold the on-screen **PTT** button,
   - **hold the Space bar** (works unless you're typing in a field),
   - or let the configured rig method key it (CAT, serial RTS/DTR, or VOX) — set
     in [Settings ▸ Rig / CAT](settings-reference.md#rig--cat).
   For hands-free operating, toggle **Lock**.
3. Talk on the rig's microphone. Nexus handles the canned messages, recording,
   scope, and CAT/PTT — the voice path itself is the rig's own mic.
4. The cockpit **unconditionally drops PTT when you navigate away**, so there is
   no stuck-transmitter path.

### Work FM and repeaters

1. Set **Phone mode ▸ FM** in
   [Settings ▸ Rig / CAT](settings-reference.md#rig--cat). The cockpit's mode
   badge switches to FM and the rig is driven to FM.
2. For a repeater, set the **Repeater shift** (simplex / plus / minus — the
   offset is the band standard, e.g. 600 kHz on 2 m, 5 MHz on 70 cm) and the
   **CTCSS (PL) tone** in the same Settings tab.
3. Tune to the repeater's output frequency and operate — Nexus applies the shift
   and access tone through CAT.

### Use the voice keyer

The voice keyer has six F-key slots: **CQ, My Call, Report, QRZ?, 73, Again**.

1. **Record in-app** or **import any WAV** (Nexus resamples and downmixes
   automatically). Choose your recording mic in
   [Settings ▸ Audio](settings-reference.md#audio) — on a digital setup the
   default input is the rig's RX audio, so point "Voice mic (recording)" at your
   actual microphone.
2. Press a slot to play it. Playback keys PTT for the duration; **Esc** aborts.

If you never use it, untick **Voice Keyer** in ⊞ Panels and the pane goes away.
Hiding it while a message is playing stops the message — that is deliberate, and
the menu entry tells you so first.

### Record a QSO

QSO recording streams the rig's RX audio straight to a timestamped WAV on disk,
with crash-safe headers and a 2-hour auto-stop, so a long ragchew or a dropped
session never leaves you with a corrupt file.

## Field Day and logging

The log strip pre-fills **59 / SSB**. Log a contact and — because the draft is
seeded from what you were actually running — it says SSB, never an accidental
"FT8." During [Field Day](contesting-pota.md), the strip becomes an FD entry with
class and section, sharing dupe checking with the other cockpits, and each
contact routes to the event log.

License-class enforcement hard-blocks PTT outside your privileges — see
[Settings ▸ Station](settings-reference.md#station).

## Honest limits

- **No live mic-through-app audio bridge.** You use the rig's mic for live voice;
  Nexus handles canned messages, recording, the scope, and CAT/PTT — not a
  software voice path to the transmitter. This applies to SSB and FM alike.

## Related guides

- [CW](cw.md)
- [Operate — FT8/FT4 digital](operate-digital.md)
- [Field Day & POTA/SOTA](contesting-pota.md)
- [Settings reference](settings-reference.md)
