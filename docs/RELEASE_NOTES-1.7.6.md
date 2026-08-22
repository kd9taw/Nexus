# Nexus 1.7.6 — the things you reported

*2026-08-21*

No new mode this time, and no headline. Everything below came from somebody filing it — most of
it in the week after 1.7.5 went out — and the batch is worth taking because three of these were
costing people their setup or their contacts.

**Your hidden panes and disabled modes survive an upgrade.** If you had turned off the modes you
do not operate, or hidden panels with the ⊞ menu, an upgrade could hand them all back to you.
Your settings came through fine, which is what made it confusing: those two choices were never
in your settings. They were in browser storage, so anything that reset it took them and nothing
else. They now live beside your settings, per profile, and they go into the backup with
everything else. A popped-out panel keeps its own layout per window, which is right — that is
chrome for a second window, not a setup you spent time on.

**Special-event callsigns are CQs again, so double-click works on them.** A call like `II7MGBR`
or `EN3SUKR` does not fit the standard callsign shape, so FT8 sends it in a form that carries no
grid — and only the compound kind, the ones with a `/`, were being read as real CQs. The rest
came through as free text: no CQ chip, and a double-click did nothing, while the Work button in
the Stations list started the QSO perfectly well. Same call, two different answers depending on
where you clicked. Both work now.

**The Call Roster shows the full CQ.** A station calling `CQ DX` looked exactly like one calling
a general CQ, so you would click to work him and find out from the Band Activity pane that he
wants DX and is going to ignore you. `CQ DX`, `CQ POTA`, `CQ NA` and the rest now show in the
roster itself, in a colour you can pick out while scanning, and hovering says what it means for
answering him.

**Switching from CW to the FT screen left the rig on the CW frequency.** The mode changed to
DIGU correctly; the dial did not move. It only happened if you passed through Tempo on the way,
which is why it looked intermittent — Tempo is a digital mode and asserts the rig mode, but it
keeps its own band picker's frequency, and it was being counted as though it had already landed
the dial for you. The FT screen then thought there was nothing left to do.

**The filter width the radio actually took is checked now, not assumed.** Some rigs accept a mode
change with a filter width, answer "done", and quietly keep the filter they had — so FT8 ended up
listening through a 6 kHz SSB filter with nothing on screen saying so. Nexus reads the width back
after a mode change and re-asserts it once if the radio kept its own; if the second attempt is
ignored too, it tells you the actual width rather than implying it worked. A rig that rounds to
the nearest filter it owns is left alone — asking for 3 kHz and getting 2.7 is the radio doing
its job.

**A manual notch you can place, and a depth for the speech processor.** The Notch button was
driving the radio's *automatic* notch, the one that hunts a carrier down by itself, which is not
what most operators mean by the word. The manual notch — the one you park on a whistle by ear —
is there now, with a frequency slider to put it where the whistle is. COMP gained the control it
was missing: how hard the compressor works. Each shows up only if your radio reports it, so
nothing grows a slider with nothing behind it.

**The dial is marked on a native RF panadapter.** If your radio streams its own spectrum (Icom
CI-V, FlexRadio), the tuned frequency has a line and a DIAL label on it. It is drawn where the
dial genuinely is: on a rig in FIXED scope mode the span is a band segment and the VFO sits
wherever you put it, so the line lands off-centre — and if the dial is outside the displayed
window there is no line at all, rather than one pinned to the edge saying something untrue.

**A hardware keyer learns your speed before you send anything.** With a WinKeyer, the speed
slider only reached the keyer once you had sent a character — so after launch the paddle ran at
whatever speed the keyer itself was set to, and moving the slider did nothing until you typed.
The keyer is opened and told your speed as soon as it can key.

**OmniRig stops being told to check its serial port.** If Test CAT could not read a frequency
over OmniRig, the message advised checking the serial port, baud rate and CI-V — none of which
Nexus uses on an OmniRig connection, as that same Settings page says. It points at what matters
now: which OmniRig slot, and whether OmniRig's own window shows the radio online.

**PSK and Tempo have manual chapters.** PSK31/QPSK31 shipped four releases ago with no chapter
and Tempo never had one — the tour, the workflow and the honest limits for both are in the manual
now. A test fails the build if a shipped mode has no chapter, so the next one cannot reach a
release undocumented.

The full list is in the [CHANGELOG](../CHANGELOG.md). Bugs and requests go to
[GitHub Issues](https://github.com/kd9taw/Nexus/issues) — that is where they get picked up
fastest, and a diagnostic log attached to one is the shortest path to a fix.

73, KD9TAW
