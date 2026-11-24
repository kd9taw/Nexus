# Nexus 1.25.1

A safety fix for split on the FT8, FT4 and other digital screens. If you set Split Operation to Rig, please update.

With Split Operation = Rig, 1.25.0 could transmit on VFO B left on another band. After a restart and a band change,
an operator on 12 m found an over had gone out on 10 m. A slow radio such as the Yaesu FTDX10 could also have its first
overs refused, with TX turned off.

This update also fixes a double-click on FT8 and FT4 that could work the station next to the one you aimed at, and a
false "PTT OFF FAILED" on wfview's virtual port over wfview Remote.

## The short version

- **Split Operation = Rig sets VFO B for your band before every transmission** on the digital screens, as WSJT-X
  does: every over in every mode, Tune, the PTT button and a CW ID. A VFO B left on another band, by an earlier
  session, another program or the radio's SPLIT key, can no longer put you on the air there.
- **Nexus turns the radio's split off when it lets go of the radio** on the digital screens, instead of putting an old
  split back.
- **A slow radio keys a moment late instead of being refused.** A command the radio rejects, or never answers, still
  stops TX and says why.
- **A late over stays inside your period** in every mode: what does not fit is not sent, and an over that could not
  start in time is skipped, with TX left on for your next one.
- **Stop TX stops before anything is sent,** even while the radio is still taking the split commands.
- **Split Operation Fake It or None now turns the radio's split off on the digital screens,** so a split left from
  earlier can't put you on the air on another band. The CW and Phone screens' split is yours and is never touched.
- **A double-click works the station you double-clicked** on the Call Roster, Band Activity, Rx Frequency and the
  Stations cards, not the one that moved under the pointer between your two clicks.
- **wfview's virtual port over wfview Remote no longer stops TX with "PTT OFF FAILED"** when the radio answers slowly
  (Native Icom CI-V on macOS or Linux). Nexus now waits up to a second for the radio's answer, as WSJT-X does.
- **A CQ DX, CQ POTA or CQ FD shows and logs the 4-character grid it sends.** With a 6-character grid in Settings,
  the TX line, Band Activity, ALL.TXT and the message to GridTracker or JTAlert showed all six, though only four
  ever went on the air. What goes on the air is unchanged.
- **Tx 6 is sent as you typed it,** as in WSJT-X, until a station answers, you change band or you press Generate Std
  Msgs; then it returns to your call and grid with your CQ word kept. Call CQ sends Tx 6 too. A 6-character grid
  goes out as 4, and a Tx 6 that is not a CQ goes out as free text, both as in WSJT-X. The one difference from WSJT-X:
  nothing goes out under a call other than the one in your Settings, so a Tx 6 or Tx 5 that would go out as another
  station is refused, with the reason, before anything is sent.
- **Tab no longer lands on a control you cannot see** behind the bottom controls, on the FT, CW, Phone,
  RTTY, PSK, JTTY, JS8, SSTV and Tempo screens.
- **RTTY's waterfall keeps a picture and its buttons on a small or scaled screen,** and **APRS keeps its map and
  station list.**
- **The Logbook's Add form no longer scrolls sideways** on a 1366×768 screen.
- **Screen readers name the club event fields** in Settings ▸ Contesting.
- **The Needed board window opens where you left it,** at the same size and on the same monitor.

## What to do after updating

- **If you run FT8 or FT4 with Split Operation = Rig:** after updating, look at your radio's SPLIT indicator once. Start
  an over on your usual band and check that the radio transmits where Nexus shows. If an earlier session left the
  radio in split with VFO B on another band, Nexus now sets VFO B for you before the first over.

## Upgrading from 1.25.0

- **A slow radio may key a moment late** on the first over after a band change, while it takes the split settings.
  The over is shortened to fit your period rather than running into the other station's.
- **On the digital screens, the radio's split is turned off when Nexus lets go of the radio,** for example when you
  close Nexus. Before, an old split was put back.
- **Off the digital screens, your own split is put back as before,** but only when its transmit frequency is on the band
  you are on.
- **A Tx 6 that is not a CQ is now sent as free text,** as WSJT-X sends it. 1.25.0 refused it. A Tx 6 or Tx 5 that
  would go out as another station (QRZ W1XYZ FN42, W9XYZ W1XYZ RR73) is refused, which WSJT-X would send.

## Security

- **A library inside Nexus was updated** for a new advisory (RUSTSEC-2026-0594) about web cookies. Nexus does not use
  the affected code in a way the advisory describes. Nothing to do on your side.

## Credits

- **WSJT-X's own copyright notice** now appears in Nexus's NOTICE file and the README credits, as WSJT-X asks of every
  program built on its work.

## Known issues

The split fixes, Tx 6 and the wfview fix have been tested in software only, not yet on the air. Please tell us how
they work on yours, especially on a slow radio such as the FTDX10.

wfview's virtual port on Windows (com0com and similar pairs) is still treated as a USB cable, so the longer wait does
not apply there yet.

A Halt TX sent from a logger such as JTAlert or GridTracker can still land a moment late if a slow radio
is in the middle of its split commands; Stop TX in Nexus, Nexus Remote and TCI are immediate. This is
being fixed in the next update.

**The full list of changes is in the CHANGELOG.**
