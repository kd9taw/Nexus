*Released 1 September 2026 — everything new since 1.10.0*

**If you're on a Mac, this release gets your radio back.** 1.10.0 could not talk to a rig at
all on a Mac without Homebrew — Test CAT tried every speed and heard nothing, whatever the
radio. The rig-control tools Nexus ships were looking for a helper file at a Homebrew path
that doesn't exist on most Macs, so they never got as far as the radio; your rig and cable
were always fine. Fixed three ways so it stays fixed: the shipped tools now carry everything
they need, the release checks now catch this class before it ships, and if a bundled tool
ever fails to start, Nexus falls back to a Hamlib already on your machine instead of going
dark. Reported by three macOS stations within a day of each other — thank you.

**A rig no longer keys the moment Nexus starts when PTT is on RTS.** On radios whose
control backend expects hardware flow control — the TS-2000 it was reported on, and most
Kenwoods and several Yaesus — choosing RTS as your PTT method let Windows hold the RTS line
up for the whole session. On a Digirig-style interface that line is the key, so the
transmitter keyed at launch and nothing would release it. Nexus now runs the port without
hardware flow control whenever RTS is your key line: it starts low and follows PTT. If your
radio's own CAT menu has RTS flow control switched on, turn it off to match. Reported by
vk6mo.

**World Radio League joins the connectors.** Paste your WRL API key in Settings ▸ Logging &
Connectors and every logged contact flows to your WRL logbook as you work it — same as LoTW,
eQSL, QRZ and the rest, with a per-contact push button in the Logbook for anything older.
Bringing an existing log? The Export for WRL button writes an ADIF shaped for their bulk
importer, which is the right road for thousands of contacts.

**Voice memories from a script.** The rig-sharing port now answers `\send_voice_mem` and
`\stop_voice_mem` — the same spellings Hamlib's own tools use — so an external script can
trigger your rig's DVS playback (Yaesu PB and friends) while Nexus stays connected. The
radio keys itself for the playback, exactly like pressing the memory button on the front
panel. Asked for by an FT-991A station.

**The phantom "is calling you" is gone.** Finishing a contact could pop "so-and-so is
calling you" seconds later — about the station you had just worked — and switching bands
could replay an old caller before a single new decode. The alert now only ever speaks for a
decode that just arrived. A station genuinely calling you alerts exactly as before.

**Spots headings point at the station, not the country.** Every United States spot used to
show the same bearing — the country's reference point in Kansas — even though most spots
carry the station's own grid. A spot with a grid now shows the true heading to that
station; only a genuinely grid-less spot keeps the approximate "~" country heading.

**Smaller fixes that were getting in the way.** A popped-out waterfall could come back
after a restart as "popped out" with no window anywhere — no more. The Connect map now
remembers which layers you had on, and their opacities, across launches. eQSL works for
accounts with more than one QTH profile (there's a QTH Nickname field beside your eQSL
sign-in). On Linux, the GStreamer warning at startup is gone and the sound-card capture
gets real scheduling slack, so a busy desktop stops costing you torn FT8 symbols. And the
TV wall display is now the full Connect view — the real map, every layer — not a summary
page.

**Two new switches.** The LoTW "Confirm" tier on the Needed board and decode chips can be
turned off if you chase new ones rather than confirmations — it stays on by default. And
signal reports can ride your log comments the way WSJT-X writes them ("FT8  Sent: -05
Rcvd: -12") — off by default, one switch in Settings ▸ Digital.
