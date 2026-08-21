# Nexus 1.7.0 — PSK31 and QPSK31, OmniRig, and macOS as a real platform

*2026-08-18*

Three new capabilities, and a lot of repair behind them.

**PSK31 and QPSK31.** The classic narrow-band keyboard mode, both directions, written from
scratch. Open the PSK screen on the Digital rail, tune 14.070, click a warble on the waterfall
and text prints. Transmit is the way PSK31 is actually operated — type and press Enter, use the
F1–F4 macros, or hold the carrier up with continuous TX and type into it. QPSK31 sits behind
the mode selector with the sideband-reverse toggle it needs. Nexus transmits at a modest drive
and the dock reminds you to keep ALC near zero, because an overdriven PSK31 signal splatters
into the operators either side of you.

**OmniRig.** If you already run VE3NEA's OmniRig, Nexus can use it: pick OmniRig in Settings ▸
Radio ▸ Rig & CAT and choose RIG 1 or RIG 2. The radio is set up *in OmniRig*, so Nexus stops
asking for the model and port it does not own. Windows only.

**macOS.** The Mac build shipped at 1.5.0; this is the release that makes it livable. Every
build from 1.5.0 to 1.6.1 was missing the entitlement that lets a signed app open a microphone,
so capture returned silence forever and macOS never even asked — that is fixed, and if your Mac
has been deaf this is why. Installing an update now actually restarts the app. ⌘Q runs the full
shutdown instead of skipping the transmitter unkey and the journal flush. Browser links, Field
Day exports and the share card all did nothing on a Mac and now work; Pounce notifications
appear for the first time; ⌘-click and ⌘1–9 do what the Ctrl versions do elsewhere.

**Rotators — five models could not work at all.** The baud was one app-wide 9600 sent as an
override of the rotator's own driver. Correct for GS-232, wrong for everything else, so SPID
Rot2Prog (600), SPID Rot1Prog (1200), Idiom Press Rotor-EZ, Hy-Gain DCU-1 and Green Heron RT-21
(4800) were told to speak at a rate their controller does not answer, and nothing said why. The
baud now comes from the model. Also: a pointing command that gets no answer is a failure instead
of a silent success, "Allow flip" actually works so an overhead pass no longer wraps the mast,
and a failed park no longer drives the antenna flat into the wind.

**FlexRadio.** A full audit found 103 confirmed problems. The opt-in native path assumed Nexus
was the only program connected to the radio — it would adopt and then delete SmartSDR's own
panadapter, bind another program's audio stream, and ignore your drive setting entirely on
transmit. The everyday Hamlib path had its own: configuring a second radio silently wiped your
Flex address, and a level the radio refused was re-sent fifty times a second for the rest of the
session. Separately, **CW below 10 MHz did not put an SDR into CW at all** — Nexus asks for CW-L
and four profiles, SmartSDR CAT among them, do not offer it under that name.

**An FT8 transmission could land in the wrong slot.** An operator watched his radio transmit in
the odd slot while set to the even one. When a decode takes longer than the fifteen-second
period — a machine bogged down by a virus scan — the late result could re-run the transmit
decision carrying its original slot number, which still passed the even/odd check. It can no
longer key. Latent since 0.13; it takes a badly stalled machine to trigger.

Also: SSTV text stays where you drag it, the ATU button reaches the FT modes, the Log QSO button
stops claiming success for a QSO it refused to log, N3FJP logging carries reports/name/power,
the logbook exports a date range, the docked Band Activity strip tunes on scroll, and WSPR and
FST4W beaconing behaves the way WSJT-X's does.

With thanks to **on8st** for the in-app confirmation dialogs and rig-form validation, and to the
operators whose field reports became fixes here: KR8MER, Tomsk666, kr4fqg, akhepcat,
JoeiinCanada, VE3WEJ, N8GB, mw0cqu, F4MQS, SP6U and KF4YHC.

Full detail in the [CHANGELOG](../CHANGELOG.md).

73 — KD9TAW
