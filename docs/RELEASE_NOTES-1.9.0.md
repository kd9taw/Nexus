# Nexus 1.9.0

*Released 25 August 2026 — everything new since 1.8.1*

Two headlines. Nexus speaks Spanish and French now, and on Linux and the Mac it finally talks
to your radio out of the box. The rest came off the air — most of it from one long evening
chasing RI1FJL on Franz Josef Land.

---

## Nexus speaks Spanish and French

Both are complete — every screen, every dialog, all 4,681 phrases, the same coverage German has
had. It follows your computer's language on its own, or you can pick one in Settings.

The language menu now says **English · Deutsch · Español · Français**, which sounds obvious
until you learn it was listing them as "es" and "fr". If you are hunting for your own language
you should not have to know its ISO code to find it.

Frequencies, callsigns, grid squares, signal reports and mode names stay exactly as they are in
every language. That is deliberate and it matters: Spanish and French both write a decimal
comma, and a dial reading 14,074 is an operating fault, not a cosmetic one.

## CAT rig control works out of the box on Linux and macOS

Nexus carries Hamlib — the `rigctld` program it drives your radio through — inside every
download now, on every platform. Nothing to install, no Homebrew, no apt.

Windows has worked that way since the beginning. Linux and the Mac never did, and the AppImage
was the worst of it: an AppImage is the file you download *because* it installs nothing, and it
was the one that could not talk to a radio until you worked out on your own that you needed a
package called `libhamlib-utils` that nothing had ever mentioned. It shipped Hamlib's licence
files and no Hamlib. On the Mac it was `brew install hamlib`, the same story.

If you already installed Hamlib yourself, nothing changes and nothing clashes — Nexus prefers
its own copy and falls back to yours.

## Working a DXpedition

**Clicking a station you are already working no longer throws the contact away.** Set your
message by hand — say you drop back to sending your grid — then click the station again, and
Nexus went back to sending the report. Every click was quietly restarting the QSO from scratch
and working out the message again. It also restamped the contact's start time, so it would have
gone into your log at the time of your last click rather than when you actually started.

Clicking a *decode* still does what it always did: you are pointing at a message and saying
answer that one. Clicking the *station* — a roster row, a station card, a spot — now just gets
you going again and leaves your message alone. If you had been calling long enough for Nexus to
stop, that click still restarts the calling.

**Calling a DXpedition no longer gives up after eight tries.** Carried over from 1.8.1 and worth
repeating because it is the same station that found it: calling somebody who has never come back
is open-ended, the way WSJT-X does it.

## Your ALL.TXT lines up with everyone else's again

If you compare logs with a friend — and people do, when a QSO goes strangely — the times were
useless. Nexus stamped each line with the moment the decoder happened to finish, not the period
the signal was actually in. Three things fell out of that, and a station watching the same QSO
from his own shack found all three:

- A transmit line and a receive line landing on the same second, which cannot happen.
- Times that never sat on a 15-second boundary, so the two files could not be lined up at all.
- One period's decodes split across several timestamps, so half a period looked missing.

Every line now carries its own period, exactly as WSJT-X writes it.

## The SSTV waterfall comes back

Change band while a picture was coming in and the band display stopped — and stayed stopped.
Switching modes and back was the only way to get it returned, and landing on an SSTV frequency
stopped it again.

The screen shows the band until a picture starts arriving, then shows the picture in the same
place. A picture that began and never finished was never cleaned up, so Nexus went on believing
one was still coming and held the display for it, for the rest of the session. Changing band is
the obvious way to cause that, but so is the sending station stopping mid-picture, or the band
going long.

A picture that has run well past the time its own mode takes is now given up on and the
waterfall returns. Nothing is given up early — the allowance is per mode, so a Scottie DX still
gets its four and a half minutes.

## Around the bands

**RTTY lands where the band plan says.** The listed frequencies were the frequency your signal
comes out on, which is what the dial reads on true FSK but not on AFSK, the default. On AFSK
the tones sit about 2.3 kHz below the dial, so the signal landed low — inside the FT4 cluster on
20 m, inside FT8 on 17 and 12, inside JS8 on 15. Exactly the overlaps the band plan exists to
avoid. Both keying methods now land in the same place. Your dial will read about 2.3 kHz higher
than before on AFSK; the signal is what moved back where it belongs.

**RTTY transmits on the frequency you tuned to.** Clicking the waterfall to net onto a station
moved the decoder but not the transmitter, so you answered on a frequency nobody was listening
on.

**A zoomed waterfall stays where you put it.** Picking a numeric span made the display re-centre
on your receive marker every time you clicked, sliding the view sideways with nothing to scroll
it back.

**A directed CQ stays put** instead of lasting one contact. "Clear DX call after logging" was
also wiping your CQ message, so `CQ DX` went back to a bare CQ after a single QSO.

**The needs chip says LoTW,** which is what actually closes a contact for awards. And a
DXpedition marker is no longer counted as something you need from a station — it was a label,
not a reason, and it was already shown elsewhere.

**Both signal reports reach Log4OM** and anything else on the N1MM broadcast. Sent and received
RST were missing from every contact.

**The Tempo roster works on Tempo Deep,** where it had always been empty — which reads like a
quiet band rather than a fault.

**Settings says "receive" where it meant receive.** The label described the opposite of what the
switch did.

## Linux

**Nexus stops asking the system keyring whether it could upload when it has nothing to upload.**
The auto-upload worker checked your credentials every two seconds whether or not a single
contact was waiting, and on Linux each check is a round trip to the keyring daemon. Windows and
macOS were never affected.

## For anyone who has sent me a crash report

Windows builds now publish a symbol map alongside the installer, so a crash address in a report
can be turned into the function it happened in. It changes nothing about how Nexus runs.

---

## Downloads

Windows · Linux AppImage · Linux .deb · Raspberry Pi (bookworm and trixie) · macOS on Apple
Silicon.

If you are running a test build, this one outranks it and the updater will offer it.

73 — KD9TAW
