# Nexus 1.4.0 — A scope that shows you the signal

*2026-08-15*

The CW and Phone scope has been rebuilt around one complaint from an operator running an
FTdx10 beside it: *"I see big vertical spikes where the voice is; on Nexus it seems like it's
all smoothed out without the aggressive peaks."* He was right, and it was not a limitation —
it was four separate faults stacked on top of each other. Signals now rise and fall with how
strong they actually are, carriers draw as lines instead of blocks, and CW keying is visible
as keying.

**Take this one if you use the CW or Phone scope, if your radio goes into DATA when you open
POTA/SOTA, or if your rig transmits the moment Nexus starts.**

---

## The scope

Four things were wrong, and each of them flattened the picture in a different way.

**A signal could not get taller.** The scope took its top of scale from the loudest thing on
screen and refitted it every row — so a signal 40 dB out of the noise drew at exactly the same
height as one 12 dB out. It was already at the top; there was nowhere to go. The scale above
the noise is now fixed, the way a rig's is, and height means strength again.

**The noise floor was drawn near the top.** It took its bottom of scale from the quietest bins
in view, and on an audio scope those are the far side of your radio's own filter — some 40 dB
below the band noise. So the noise itself rendered at the top of the panel with nothing left to
rise above it. That is the "whole spectrum is up" half of the report.

**A carrier was a block, not a line.** Every scope in Nexus mapped analysed frequencies onto
display columns in a way that let neighbouring columns claim the same data, so one carrier was
painted into two or three columns at identical height. Carriers now land in one column with
real shoulders either side.

**And there was not much detail to draw.** The CW cockpit shows about 800 Hz, but the scope was
computing across the whole receiver passband and throwing most of it away. It now asks for its
detail across the span it is actually showing — five times finer on that window, for the same
amount of data.

There is also a **resolution button** beside the palette controls, and it is worth a minute if
you work CW. At the default the scope looks at a longer slice of audio than a dit lasts at
25 WPM, so keying cannot be seen as keying no matter how good the display is. One click the
other way and it can. Sharper costs response, faster costs detail, and no setting is best at
both — which is why it is a button rather than a decision made for you.

The trace hold that made CW draw a solid bar is gone too: it held each peak for four tenths of
a second, right for speech and far too long for a dit.

## Your radio

**POTA / SOTA no longer puts your rig into DATA.** Reported on an FT-991A and seen again on a
Flex 6400 — opening the hunting board flipped the radio from SSB into DATA-USB, on every band,
since 1.0.0. A hunting board is not a mode: you work a park on whatever the activator is
running. It no longer touches the radio at all. Clicking HUNT still moves you to the spot.

**If your radio transmits the moment Nexus starts**, Settings ▸ Radio ▸ Rig & CAT now has
**Interface keys RTS on the CAT port**. Tick it. A one-cable interface — a Digirig and most
like it — keys the radio from the same serial line CAT travels on, and that line has to be held
down or the rig reads it as a mic key. Nexus cannot tell such a cable from a radio that needs
that line for its own handshake, because they report themselves to Windows identically, so this
is the one thing it has to be told. Leave it off otherwise; the default is what you have today.

**4 m is in the SSB and CW band pickers**, not only in the FT dropdown, for anyone licensed
outside the US. And **your RX filter stays where you put it** when you change frequency inside a
band.

## Logging

**CW contacts carry the other station's grid.** The grid is filled in behind the scenes from
your callbook, and that lookup only ever ran for a callsign you typed by hand — which in CW you
almost never do, because the decoder fills it in for you. So every CW contact logged without a
grid, a state or a country.

**A DXpedition worked with a bracketed callsign logs as itself.** When a call will not fit in an
FT8 message the protocol wraps it in angle brackets, and those were reaching the log — so the
contact read as a different station, and counted for nothing on the awards board, silently.

**A station repeating their report gets answered.** If you send your final roger and the other
operator does not copy it, they send their report again — and Nexus used to go quiet at exactly
that moment and move on, which read as the contact being abandoned. It now answers, as WSJT-X
does.

## Also

The **waterfall runs downward by default now** — newest line at the top, history sliding down,
the way most rigs draw it. If you preferred it the other way it is one button in the waterfall
header and it stays clicked.

The **Call Roster** gained **Calling** and **State** columns, an **ATU button** runs your
radio's own tuner from the Phone, CW and SSTV headers, and hints in text boxes stop looking like
settings you have already entered.

Full detail, as always, in the [CHANGELOG](../CHANGELOG.md).

73 — KD9TAW
