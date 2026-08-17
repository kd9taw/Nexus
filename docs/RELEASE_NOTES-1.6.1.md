# Nexus 1.6.1 — two field reports, two fixes

*2026-08-17*

A point release, two days after 1.6.0, for two problems you reported.

**Windows + USB rig interfaces: audio opens again.** If your DE-19, QDX, or similar
interface failed with "the requested stream type is not supported" — a regression since
1.3.0 — this is your fix. A change made for Linux sound cards taught the audio open to
share one device handle when the input and output names match; on Windows those are two
separate one-direction devices that happen to share a name, and Nexus was handing the
output stream your capture device. Sharing is now platform-aware. (#99, #104 — and if you
renamed your Playback device as a workaround, you can rename it back.)

**The waterfall can't freeze and pretend to be live.** Reports of waterfalls stopping
after seconds or minutes: four separate routes into that symptom are closed. A stuck
transmit state now times out instead of freezing the display forever, the TX dark band no
longer applies to RTTY and SSTV (whose minutes-long transmissions made it read as a dead
display), a wedged display fetch recovers after five seconds instead of ending updates for
the session — on the Phone/CW scope too — and an audio-device recovery keeps its warning
banner up until the new device actually delivers samples.

Everything in [1.6.0](RELEASE_NOTES-1.6.0.md) — FT2, the satellite fixes, the scope, the
SSTV composer, the Mac build — is unchanged.

Full detail in the [CHANGELOG](../CHANGELOG.md).

73 — KD9TAW
