# Nexus 1.7.5 — Nexus speaks German

*2026-08-20*

**Deutsch.** Nexus can now run in German — the first language it has ever offered. Pick it in
Settings ▸ Appearance ▸ Language, or just run it on a German Windows and it starts in German by
itself. About 4,600 phrases: every settings page, the setup wizard, every dialog and every error
message, and the whole of the operating chrome around them.

Nothing technical is translated, and that is deliberate. Frequencies, signal reports, callsigns,
grid squares, band and mode names, Q-codes, RST, POTA and SOTA references and ADIF field names
read the same in every language, because they are what you put on the air and in your log. In
particular a frequency never picks up a German decimal comma — 14.074 is 14.074 everywhere, and
a test fails the build if a comma ever appears inside a translated number. The transmit controls
themselves — Stop TX, Tune, the TX arm switch, ATU, the TX/RX indicator — stay in English for
now, on purpose: those are the controls that stop a transmission, and they move as their own
reviewed step rather than as part of a bulk translation.

Anything not yet translated falls back to English rather than appearing blank, so a partial
catalog can never break a screen. If you would like to see your own language here, that is now a
matter of one file — `docs/i18n.md` says what a language takes.

**Find a station in a crowded list.** The Stations panel has a search box beside its filter
chips, and it takes the wildcards you would expect: `PA*` for every PA prefix, `ON4*` for every
ON4, or both at once — several terms mean "any of these". `?` fills in exactly one character, and
a plain word like `4FD` still matches anywhere in a call. It narrows whatever the filter chips
are already showing rather than replacing them, the count beside the title now tells you how many
of the total you are looking at, and Esc clears it. The same wildcards work in the Spots search.

**Tune, ATU and RF power reach the PSK and RTTY headers.** Those two cockpits were missing the
controls every other one puts in the same place, so an operator who tuned up in Phone and
switched to PSK found the button gone from where he had just used it. In PSK that is the mode's
one real hazard: set the drive against a Tune carrier, below where ALC starts to move, and your
signal stays clean — overdriven PSK31 splatters into the operators either side of you, and it
looks fine on your own waterfall while it does it. RTTY keys the carrier for the whole over, so
it wants running well under the rig's SSB rating. The ATU button appears only if your rig
actually reports a tuner.

**Two more things that were being asked for.** Nexus now keeps periodic logbook backups in a
`backups/` folder beside your log — at most one a day, only when the log has actually changed,
plus an immediate copy whenever a save is about to make the log *smaller*. And there is a
diagnostic log you can attach to a bug report: `nexus-diag.log`, beside `ALL.TXT`, covering
startup, the CAT and audio device open, updater checks and crashes, with passwords and API keys
masked before anything is written. Until now Nexus wrote nothing at all about its own health, so
"it won't start" reports arrived with nothing to look at.

## Fixes worth naming

- **Virtual COM ports appear in the rig picker on Windows.** If you run SmartSDR CAT, com0com or
  any virtual serial pair, its ports could be missing from the list while your real USB ports
  showed up fine.
- **Saving in Settings no longer ends the contact you are working.**
- **The CW speed slider now reaches a WinKeyer** while you are between overs.
- **A logbook with accented or non-English text in it could load as empty** — and say nothing.
- **Nexus could arrive in FT8 with the transmitter already armed** after leaving a manual mode.
- **Nexus could fail to start with no window, no error and nothing to send.**
- **Frequencies and coordinates typed with a comma decimal separator were read wrong.**
- **The decode panes remember ten times as much**, so you can actually scroll back.
- **OmniRig failing with "requires elevation"** now says what to do about it rather than showing
  a bare `0x800702E4`: OmniRig is set to run as administrator and Nexus is not. Start OmniRig
  yourself and leave it running, or clear that flag on OmniRig.exe.

The full list is in the [CHANGELOG](../CHANGELOG.md).
