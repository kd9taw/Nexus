*Released 31 August 2026 — everything new since 1.9.2*

**Field Day is now a club system, not a single position.** One computer at the site hosts the
event; every other position joins it — Nexus finds the host on the network, or you type its
address — and from then on every contact you log streams to the host as it happens. Each position
sees the club's running score, section totals, and a live band board showing who is on which band
and mode, so you can look before you move rather than shouting across the field. While you type a
callsign you are told if another tent has already worked that station on this band and mode: a
warning, not a lock, exactly the way N3FJP treats it — your own log's dupes still refuse.

It keeps working when the site does not. A position that loses the network keeps logging and
re-syncs itself when the cable comes back. If the host machine dies, turn hosting on at any other
position and everyone re-joins with nothing lost. The host exports the merged club log as Cabrillo
or ADIF, deduplicated the way the rules actually score it. If you also push to N3FJP or N1MM+,
that carries on untouched alongside.

**Put the club score on the TV.** The host serves a spectator page any browser on the site network
can open — score, rate, the sections board filling in, who is on what band, and the newest contact
as it lands, with one plain-language line so visitors who are not hams can follow what they are
watching. Nothing to install on the TV. There is also a Club Board button in the left rail that
opens the band board as its own window for a second monitor.

**And now Connect goes on a TV too.** Turn on Settings ▸ Appearance ▸ Connect on a TV and your
conditions picture — band conditions, space weather, live openings, the plain-language notes —
serves to any browser on your house network in type you can read across the shack. It is read-only:
there is no control on it that changes anything, and it loads nothing from the internet, so it
still works at a site with no connection. Off until you switch it on, and the setting says plainly
what anyone on your network would see: your callsign, your grid and band conditions. Your log, your
needs board and the frequency you are on are never sent.

**Nexus speaks Japanese.** 日本語に対応しました。 The whole application — every screen, every
setting, every message — alongside English, German, Spanish and French.

**When do the bands settle down?** Everything in Connect told you how conditions are right now and
nothing told you what was coming. The new Kp outlook pane shows NOAA's three-day forecast: the
worst period still ahead, when a storm is expected to start, and when it lifts. Measured hours are
solid bars and the forecast is drawn hollow, so a prediction never reads as a reading.

**A magnetic storm now finds you.** A solar flare already raised an alert wherever you were in the
app, and so did a band coming alive — but a geomagnetic storm only ever appeared in the Space
Weather pane, so you saw it only if you happened to be looking there. A flare is minutes of
absorption; a storm is days of degraded HF, and it is the one that decides whether tonight is worth
sitting down for. Storms now speak up the same way, quietly at G1 and loudly from G2, and they do
not repeat themselves while the index wobbles. Nexus tells you; it never touches your radio.

**Parks on the air, on the map.** POTA activators now show on the Connect map, at the park rather
than roughly in the right square — the spot feed carries each park's own position and Nexus had
been throwing it away. Filled markers are parks you have never logged, hollow ones you already
have, and hovering names the activator, the reference and the frequency. Choosing the POTA/SOTA
view switches the layer on, which is what that view had been promising and not doing.

**Your country file can be updated at last.** Settings ▸ Station ▸ Update country file now works.
The copy built into Nexus was from January 2025; the one it fetches is current, and it tells you
how old yours is. It applies at the next start.

**Memories, from your reports.** The mode list offered eight modes and let you pick exactly one —
whichever the radio was on — so for most stations USB was the only mode you could add. Fixed, and
the tone list had the same fault. Channels imported from CHIRP keep modes and tones Nexus has never
heard of instead of quietly rewriting them. ＋ New now opens a panel pinned at the bottom of the
pane rather than an editor lost somewhere in a long list, and it no longer looks dead when you have
something typed in the search box. Every row has a tick box for deleting several at once, deleting
only ever takes rows you can actually see, and every delete — including the ✕ on a single row —
offers an undo, in the popped-out window too, where the undo it promised had not existed.

**Also fixed:** Tune could interrupt itself with a brief power drop while the carrier was up. A
Winter Field Day RTTY contact reached your club logger labelled FT8. Renaming a Field Day position
did not reach the club board. A ClubLog catch-up could look like a flood and get your address
blocked. Band dropdowns offered bands no radio in the shack can reach. On a FlexRadio there is no
RTTY mode to select, so Nexus now asks for the data mode that actually works. Your own transmissions
no longer follow you to another band, and Erase clears them too. On macOS a fresh install could not
connect to a radio at all. Spanish and French showed doubled text wherever a count was involved.
And the Field Day manual said the event runs 24 hours; ARRL Field Day runs 27 and Winter Field Day
30.
