# Nexus 1.2.0 — FT8, FT4 and CW through satellites, and the rig's own key finally shows

*2026-08-10*

Hold a bird and work it on FT4 — the dial stays put, Doppler steps between slots, and
the log comes out right. Phone and CW finally show TX and live meters whenever the
transmitter is actually keyed, including from the radio's own mic or key. Plus WSPR
countries, timeline duplicates, and a hide-worked filter for Band Activity.

**Take this one if you work satellites or operate Phone/CW.** Both headliners were
verified on real Yaesu hardware before this shipped.

---

## Work the birds on FT4, FT8 — and CW

Hold a transponder in the Satellites section and switch to the FT console: the dial now
stays on the bird. It used to bounce off the FT8 watering hole and come back in the wrong
mode — the section change re-homed the frequency and dropped the bird's sideband answer.
Now the pass owns the dial through section changes and FT8↔FT4 flips, and the rig lands
in the right form on the bird's side: DATA for the digital tiers, CW (or CW-R on an
LSB-side bird) for the keyer, your sideband for phone.

Doppler behaves under slot modes: corrections land only in the quiet part of each slot —
at the boundary or after the signal ends — so a receive period is never stepped
mid-decode. CW keeps continuous correction, the way dedicated satellite programs run it,
and the AI CW decoder held copy across a stepped pass on the bench.

The bookkeeping is right too: a QSO logged on a digital tier records the tier's own mode
— FT4, not SSB — alongside the automatic `PROP_MODE=SAT` tag from 1.1.0. And a held bird
is never invisible: the operating cockpits show a SAT chip ("bird holds the dial") with a
release button, even before a pass is tracked. QO-100-class operating — pick the
transponder, no pass needed — works the same way.

One honest note: the band table still ends at 23 cm, so 13 cm/3 cm dials carry no band
label yet. Tuning and logging work by frequency; the band-architecture extension is the
next satellite batch.

## The transmitter shows when it's keyed — however it's keyed

Phone and CW watched a flag only the FT8 transmitter set, so a voice or CW over showed RX
and no meters — even though the SWR/ALC/power readings were being polled the whole time.
Both now follow the real transmit state. And Nexus now asks the rig for its own PTT once
a second while idle, so keying the radio from the radio — mic PTT, a straight key —
shows as TX with live meters within a second. Reported by Tomsk666 (#57); both halves
bench-verified on an FTdx10 and an FT-991.

## Also fixed

- **Leaving the FT screen and coming back no longer wipes your roster and decodes.**
  Checking the Logbook and returning froze the app briefly, cleared the Call Roster and
  Band Activity, and started them over — the return path re-issued the mode switch, and
  the stale-cycle protection that rightly clears everything on a real FT8↔FT4 change
  fired when nothing had changed. Navigate anywhere and back; your decodes stay put.
  Reported with a clean repro by kr4fqg. Band Activity's empty message also now says
  *which* kind of empty — "nothing matches the To-me filter" is not "no decodes yet".
- **WSPR spots name the right country.** A WSPR line reads CALL GRID POWER, and the
  decode feed was parsing it with the FT8 grammar — the power figure looked like a
  signal report and the grid slid into the callsign slot, so TI4JWC's Costa Rica showed
  as Armenia. Reported by graafpeter-web (#55).
- **Old transmitted calls stay in their own time period.** On a busy band your earlier
  CQ lines could resurface under the live period with stale timestamps — a display-only
  re-ingest bug in the decode pane's history. Reported by m7jyfradio (#15).
- **Switching FT8→FT4 on a band with no FT4 calling channel stays put** instead of
  jumping to the top of the band plan — which used to drag a 70 cm station to 80 m.
- **Band Activity grows a −B4 switch**: stack it with any filter chip — CQ-only minus
  worked-before is one click, and it persists across restarts. Asked for from the field.

---

Full details in the CHANGELOG. Windows, Linux and both Raspberry Pi builds:
https://hamradiotools.io/nexus/
