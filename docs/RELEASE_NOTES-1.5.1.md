# Nexus 1.5.1 — Nexus comes to the Mac

*2026-08-16*

One thing, and it's been asked for since the first release: **a native macOS build.**

Apple Silicon (M-series, macOS 12 or later), signed and notarized — it opens with a
double-click, no Gatekeeper wrestling — and it self-updates the same way the Windows and
AppImage builds do. Everything in [1.5.0](RELEASE_NOTES-1.5.0.md) is in it, meteor scatter
and all.

Two Mac-specific notes. CAT control talks to your radio through Hamlib, which on the Mac
comes from Homebrew: `brew install hamlib` once, and Nexus finds it — including the checks
that landed in 1.5.0 so a half-installed Hamlib gets skipped for one that actually runs.
And if you're on an Intel Mac, the app builds from source out of the box as of 1.5.0; the
prebuilt DMG is Apple Silicon only, because the modem's Fortran toolchain is single-arch.

This is the first macOS release. It has passed signing, notarization, and a
launch-and-stay-alive check on a clean machine, but it has far fewer hours on real shacks
than the Windows build — if something's off, say so on GitHub and it'll get fixed fast.

Thanks to ON8ST, whose macOS groundwork in 1.5.0 made this build possible.

Full detail in the [CHANGELOG](../CHANGELOG.md).

73 — KD9TAW
