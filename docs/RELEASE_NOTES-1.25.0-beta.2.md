# Nexus 1.25.0 beta 2

The second beta of 1.25.0 fixes what beta 1's testers reported and closes a transmit-safety gap in split and
satellite operating. Everything in beta 1 is still here; the full 1.25.0 notes are linked at the end.

## What's new since beta 1

- **A sound card that opens but sends no audio is now named and reopened.** Sometimes a radio's USB audio opens
  without an error and then sends nothing, which left the scope reading "Scope data unavailable" and the FT
  waterfall blank until Nexus was restarted. Nexus now shows **NO RECEIVE AUDIO**, reopens the sound card by
  itself (never while you're transmitting) and writes to the diagnostic log when the card opened and when its
  first audio arrived.
- **PTT NOT ACCEPTED now tells you what to do and why.** The red line on the TX strip leads with the advice
  ("check PTT method & port"), hovering it shows the radio's or Hamlib's actual answer, and the diagnostic log
  records the reason and your PTT method. A key the radio refused no longer freezes the scope. A key the radio
  never confirmed still drops the over, as before.
- **The ⋯ pop-up (Memories) on the Phone and CW screens stays on screen.** With the Dashboard on or in a narrower
  window it opened off the left edge; it now opens to the right when there's room.
- **Split and satellites: transmit safety.** A satellite's uplink is checked against your license when you pick
  the pass, on the receiver and mode it will actually transmit with. A split the license check refused is never
  written to the radio, and after the radio leaves split Nexus judges the frequency it really transmits on.
- **QRZ logbook sync keeps its place** when you save Settings during a sync, and if QRZ ever stops moving to the
  next page, Nexus starts the download again from the beginning and says so.
- **Linux AppImage:** links, files and folders opened from Nexus open normally in your browser and file manager.
- **The IOTA island list is in every build.** In beta 1 the Linux x86_64 .deb and AppImage shipped without it.

## Reporting a problem

Open an issue at https://github.com/kd9taw/Nexus/issues and say you're on 1.25.0-beta.2, which computer and radio,
and what you did just before.

The full release notes for 1.25.0 (everything beta 1 brought) are at
https://github.com/kd9taw/Nexus/blob/main/docs/RELEASE_NOTES-1.25.0.md
