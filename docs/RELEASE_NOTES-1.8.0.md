# Nexus 1.8.0 — AM on phone, and the ones you told me about

*2026-08-23*

AM is the headline, and it is the reason this is 1.8 rather than another patch. Everything else
came from somebody reporting it, and two of those are worth reading even if you skip the rest:
one had Nexus going quiet at the end of a contact without telling anybody, and one is a warning
about your radio that nothing in the app used to say out loud.

**AM on the Phone screen.** Pick AM beside AUTO/USB/LSB/FM and the rig is commanded to AM with a
6 kHz filter, because AM is double-sideband and an SSB-width filter cuts half of it off. Power
drops to a quarter of your phone setting: a rig making 100 W PEP on SSB makes about 25 W of
carrier on AM, the carrier is always there, and SSB drive clips the modulation peaks. That
ceiling can only ever lower your power, never raise it past the cap you already set. AM is
offered on the bands where it is actually worked — the windows below 10 MHz, and 10 m and up —
so you will not find the button on 20 m.

**A station that sends you RR73 gets your 73 back.** If DXpedition "Hound" mode had been left
switched on, every ordinary contact quietly inherited the Fox rule: the QSO ended on the other
station's RR73, Nexus sent no parting 73, and Enable-Tx switched off before it could. From your
chair it looked like a normal contact. From theirs you simply vanished, and they sat there
repeating RR73 at you. That is right against a real Fox, where a parting 73 lands as QRM in the
Fox's own segment, and wrong against everybody else. Hound is a per-DXpedition mode now: off
again at every launch, on when you turn it on for the DXpedition, with the amber HOUND badge
marking the session. Working a real Fox is unchanged.

**Nexus tells you when the radio is armed to transmit at 0% power.** A rig at zero still keys,
still shows TX, and still looks like a perfectly normal over from where you are sitting. It is
silent only to the station you are calling, so there is nothing to notice and no reason to
suspect the radio. The status lane says NO RF POWER while that is true, and it goes in the
diagnostic log. Nothing is changed for you — the power is not raised, not clamped, and no
transmission is held back. Worth knowing on a Yaesu especially, which keeps a separate power
level for SSB, DATA, CW and AM, so a level you set in one mode does not follow the rig into
another. That is exactly how one operator spent an evening on a radio that was keying perfectly
and putting out nothing.

**Hold Tx, and the waterfall's RX and TX markers, survive a settings save.** Pressing Hold Tx or
dragging a marker changed the setting, but any later save from the Settings window posted an
older copy back over it and then stored that, so it looked as though it had never saved at all.
None of the three is editable in Settings; they were only travelling in the form, so a save could
only ever undo them. Restoring a backup still sets all three from the backup.

**A contact the run gave up on can still be logged.** If a station answered you, exchanged
reports and then went quiet — a club station working several people at once does this routinely
— Nexus stopped calling them so your CQ run kept moving, which is right. But it threw the contact
away, so when the station finally came back with RR73 the Log button said there was nothing to
log about a QSO whose reports are sitting in your own ALL.TXT. The exchange is kept now, and Log
writes it with the contact's own start time. Nothing is logged for you that wasn't before — only
you saw them come back, so it stays your call.

**The log table shows your Comment, and marks contacts carrying a private Note.** Both could be
typed and saved, and neither was ever shown again, so the only way to read a note was to open a
contact you had no way of knowing held one. The Comment has its own column now, and a contact
with a private Note carries a 📝 you can hover for the full text.

**The Phone waterfall has frequencies on it,** so a click is not a guess, and **the Needed list
shows the frequency** rather than only the band — a rare one on 20 m is a different decision at
14.025 than at 14.310.

**The Needed board respects your New-grid band choice.** With New grid set to VHF+, HF grid needs
were still listed there. The choice reached the roster and the decode rows when it was added, but
not the board, which had stopped sharing that code path earlier so that turning the CW or Phone
features off would not hide needs. Both hold now.

**The Band Activity list marks each period once again, not every decode.** Once about three
hundred decodes had built up, the dim time-and-band bar that separates one T/R period from the
next started appearing between every single line — it was comparing each decode against one from
the far end of the buffer instead of the row above it.

**A station answering your CQ makes a sound again.** The alert was being held back for the whole
time you were calling CQ, which is the one stretch where it is the only thing you are listening
for. It stays quiet once you are into the exchange, as it always should have.

**An unanswered CQ run takes a breather** instead of holding the frequency forever — how many
calls, and how long it waits, are both yours to set. **You can record a QSL card that arrived in
the post.** **There is a reset to factory defaults,** beside the backup that makes it safe to
use. **"Hide worked" explains itself** — it hides stations you have worked except those that
still owe you a confirmation.

And the ones that were stopping people cold: **Nexus starts properly on a machine with no
regional settings** (a `C` locale used to take down the Operate screen); **it stops interrogating
the system keyring every few seconds**, which on Fedora could restart the keyring daemon in a
loop for as long as Nexus was open; **a busy PC no longer produces a phantom CAT failure** when a
rig reply is interrupted; **OmniRig gets time to start** if it was not already running, and its
"needs administrator" message now points at the fix that works; and on Linux **the AppImage no
longer carries its own copy of a system graphics library**, which is what kept it from starting
on some desktops.

---

Six installers as always — Windows, Linux AppImage and .deb, both Raspberry Pi builds, and macOS
on Apple Silicon. If you are on a tester build, this outranks it and the updater will offer it.

73 — KD9TAW
