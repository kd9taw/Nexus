*Released 29 August 2026 — everything new since 1.9.0*

**Nexus can run your linear.** Put an SPE Expert (1.3K-FA, 1.5K-FA, 2K-FA) or an Elecraft
KPA500/KPA1500 on its own serial port and Nexus reads it: power out, SWR at the antenna and before
the tuner, volts, current, PA temperature, and the amplifier's own alarms. Every operating screen
also gains a small strip — Standby/Operate, band down and up, power out — so you can put the amp in
line or move it a band without leaving the screen you are working on. If you have no amplifier,
nothing is added anywhere. Nexus will never switch your amplifier off; that command does not exist
in the code, and both controls are refused while you are transmitting.

**Three things where Nexus was quietly doing nothing.** Six of the ten sound-card formats were
refused outright — no audio at all, looking exactly like a dead band or a bad cable. Soundcard CW
keyed the rig and radiated nothing on the common mic-fed wiring, because it asked for plain SSB
instead of a data mode. And a rig could key the transmitter the moment Nexus started, with PTT on
serial RTS. All three are fixed.

**Two that were legal rather than cosmetic.** RTTY's privilege check never learned about 1.9.0's
move to transmit on the frequency you tuned to, so it could be out by 1.9 kHz — near a band edge
that is the difference between legal and not. And the split check judged your *receive* dial, which
was wrong in both directions: a transmit lock while your transmit frequency was perfectly legal —
the everyday way DX is worked — and a clean key-up with the transmit VFO parked somewhere you may
not use.

**Split grew up.** CW and Operate have a real split control now; they only ever *displayed* that
split was on. Nexus can also follow the radio's own split, if you turn it on and your rig can report
it without being disturbed.

**Audio, broadly.** Windows break-up fixed, and it was worse on battery. A log that could grow to
294 MB. Receive-only no longer opens your sound card's playback side at all. On the Mac, a card that
only captures is no longer missing from the transmit list, and two identical radios can finally be
told apart.

**Tune.** It could drop out repeatedly and defeat an automatic tuner mid-hunt. It can also key at
its own power now — and only ever turns the rig *down*, never up.

**Things you will see straight away.** A zero-beat light in the CW cockpit that tells you which way
and how far off pitch you are. Dropdowns were white-on-white on Linux and unreadable. Small print
was rendering at full size in twenty places, and about eighty surfaces could not follow the theme.
The SWR and ALC meters had no warning band at all — warn and hot were both red.

**Asked for, and now there.** PSK has a log strip. A callsign card in FT8. RTTY starts receiving
when you open it. Split contacts log both frequencies. LoTW confirmations are no longer recorded as
paper QSL cards.

⚠️ **One thing to check if you key CW through your sound card.** That path now uses a data mode
instead of plain SSB, which is what gets the tone to the transmitter rather than the mic socket. If
your interface feeds the rig's **mic jack** and soundcard CW was working for you, tick **plain SSB
for data modes** on that radio in Settings and it will work again — the same switch FT8 has always
used.

This release also carries everything from 1.9.1, whose build failed before it published, so nobody
ever received it.

The full list is in the [CHANGELOG](../CHANGELOG.md). Bugs and requests go to
[GitHub Issues](https://github.com/kd9taw/Nexus/issues) — that is where they get picked up fastest,
and a diagnostic log attached to one is the shortest path to a fix.

73, KD9TAW
