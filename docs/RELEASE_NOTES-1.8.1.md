# Nexus 1.8.1

*Released 23 August 2026 — everything new since 1.8.0*

Four fixes, all of them things that showed up on the air rather than in testing. Three came
out of one evening chasing RI1FJL on Franz Josef Land while he was working a pileup.

---

## Calling a DXpedition no longer gives up after eight tries

If you picked a station and called it, Nexus stopped after eight unanswered overs and then sat
there silent until you clicked it again. In a pileup that is exactly when you least want it to
stop — eight calls is nothing when a hundred people are calling the same station.

There is a real limit behind that, but it was written for a different problem: a station that
*answers* you and then goes quiet mid-contact, which is worth abandoning so your CQ run can move
on. Club stations working several people at once do it all the time. That part is unchanged.

Calling somebody who has never come back is open-ended now, the way WSJT-X does it. The transmit
watchdog still stops you eventually, same as it always has.

## Your reply no longer echoes the DX's shortened callsign

When a station is answering several callers at once it sends its own call in FT8's abbreviated
form — `<RI1FJL>` instead of `RI1FJL` — to make room in the message. Nexus copied that straight
back into its own replies, so overs went out addressed to the short form.

It sends the plain call now. Compound calls like `KH8/W1AW` still go out abbreviated, because
the protocol genuinely has no room for them any other way.

## A Fox is understood without turning the DXpedition setting on

A DXpedition running Fox mode packs two replies into a single transmission — confirming one
station and reporting to another at the same time. Nexus could only read that while the Hound
setting was switched on, and switching it on also stopped the closing 73 going out on every
ordinary contact you made. One switch, two unrelated jobs.

Reading the Fox no longer depends on it. It applies whenever you are working someone, and the
Hound setting keeps only its real job: no parting 73 to a Fox, where a 73 would land as QRM in
the DXpedition's own segment.

## The Phone waterfall gives the voice more of the panel

The dial marker sits on the suppressed carrier, so on USB your voice always sits to the *right*
of it. That is correct and it is how every rig works — but a third of the display was being held
empty on the other side so the marker would read as a line rather than a border, and that empty
third made the dial look like it was in the wrong place.

The gap is much smaller now and the signal is wider. Nothing about tuning or clicking changed.

---

## Downloads

Windows · Linux AppImage · Linux .deb · Raspberry Pi (bookworm and trixie) · macOS on
Apple Silicon.

73 — KD9TAW
