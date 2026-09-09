# Nexus 1.8.0

*Released 23 August 2026 — everything new since 1.7.6*

AM is the headline. The rest came from people writing in, and two of them were costing
contacts, so start there.

---

## AM on phone

Pick **AM** in the Phone screen next to AUTO / USB / LSB / FM and the rig goes to AM.

Nexus backs your power down to a quarter of your phone setting when you do. That is not
Nexus being cautious — a rig that makes 100 W PEP on sideband makes about 25 W of carrier
on AM, and if you leave the drive where SSB had it you will flat-top the modulation peaks
and sound bad. It can only ever turn you down, never up past the limit you already set.
It also opens the filter out, because AM is twice as wide as sideband and a sideband filter
chops half your audio off.

You will find AM on the bands where AM actually lives — the windows down on 160, 80 and 40,
and 10 m and up for the 29 MHz crowd and 6 m. It is not offered on 20 m, and that is
deliberate: there is no room for a 6 kHz signal there.

## Two that were costing you contacts

**If a station sent you RR73, you may not have been sending 73 back.**

This one only bit if DXpedition **Hound** mode had been left switched on from some earlier
session. With it on, every ordinary QSO you made finished the way a Fox QSO finishes: the
other station's RR73 ended it, and Nexus went quiet and switched your transmit off without
sending the closing 73.

From your seat it looked like a completed contact. From theirs, you disappeared — so they
sat there sending RR73 at you again and again, wondering where you went.

Hound now switches itself off every time you start Nexus. Turn it on when you sit down to
work a DXpedition, and it will be off again next time. The amber **HOUND** badge shows
while it is on so you always know. Working a real Fox is exactly as it was.

**Nexus now tells you when your radio is turned down to zero.**

A rig with the power at 0 still keys up. The TX light comes on, the meter swings, and from
where you sit the transmission looks completely normal. Nobody hears it. There is nothing
to notice and no reason to blame the radio, so you can burn an evening calling into an
empty band before you think to look at the power knob.

If your transmit is armed and the rig is reporting no power, Nexus now says **NO RF POWER**
and puts it in the log file. It does not touch your power — if you are running a hair above
zero to drive an amplifier, that is your business.

Worth knowing if you run a **Yaesu**: they keep a separate power setting for SSB, DATA, CW
and AM. Set your power on sideband and it does not follow you into DATA. That is how the
operator who reported this ended up transmitting into thin air on a radio that was working
perfectly.

## At the operating position

**Hold Tx stays where you put it.** Same for the red and green markers on the waterfall.
Saving anything in Settings used to quietly put them back the way they were.

**A contact that went quiet can still be logged.** You call CQ, somebody answers, you trade
reports — and then they vanish. Club stations working several people at once do this all
day. Nexus stops calling them so your run keeps moving, which is right, but it used to throw
the contact away too. When the station finally came back with RR73 the Log button told you
there was nothing to log, about a QSO whose reports are sitting in your own ALL.TXT. It
keeps the exchange now, and Log writes it with the right start time. It still will not log
it for you — only you saw them come back.

**You can read your own notes again.** The Comment box and the Notes box both saved fine and
neither was ever shown to you anywhere afterwards. Comment now has its own column in the log,
and a contact with a private note carries a 📝 — hover it to read the note. Which is the
whole point of writing down what you talked about last time.

**The Phone waterfall has frequencies across it now,** so clicking somewhere is a decision
rather than a guess. **The Needed list shows the frequency too** — a rare one on 20 m is a
very different proposition at 14.025 than at 14.310.

**The Needed board honours your New-grid band setting.** If you have grids set to VHF and up,
you were still getting HF grid needs listed on the board.

**Band Activity marks each period once,** not between every single line. It started doing
that after a few hundred decodes had built up.

**A station answering your CQ makes a noise again.** The alert was being suppressed for the
whole time you were calling CQ — which is the one stretch where it is the only thing you
are listening for.

**An unanswered CQ run takes a break** instead of calling into a dead band forever. You set
how many calls and how long it waits.

**You can log a QSL card that came in the post.** **There is a reset to factory defaults**
now, next to the backup that makes it safe to press. **"Hide worked" tells you what it
hides** — worked stations, except the ones that still owe you a confirmation.

## Things that were stopping people getting started

- Nexus starts on a machine with no regional settings configured. It used to take down the
  Operate screen entirely.
- It stops hammering the system keyring. On Fedora this could restart the keyring service
  in a loop for as long as Nexus was open.
- A busy PC no longer produces a CAT failure that was never real — a rig reply interrupted
  at the wrong moment was being read as a dead radio.
- OmniRig gets a moment to start if it was not already running, and the "needs
  administrator" message now points at the fix that actually works.
- Nexus no longer gives up on the rigctld you pointed it at because the machine was busy for
  a second and substitutes its own.
- **Linux:** the AppImage stopped carrying its own copy of a system graphics library, which
  is what kept it from starting on some desktops.

---

## Downloads

Windows · Linux AppImage · Linux .deb · Raspberry Pi (bookworm and trixie) · macOS on
Apple Silicon.

If you are running a test build, this one outranks it and the updater will offer it.

73 — KD9TAW
