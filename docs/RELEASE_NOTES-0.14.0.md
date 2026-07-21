# Nexus 0.14.0 — your rig is yours at launch, a 3-D logbook globe, and Tempo that can't lose a message

*2026-07-21 · every item below operator-verified on the air before release*

The biggest release yet: a fundamental change to how Nexus treats your radio at startup, a new 3-D view of your contact history, on-time FT8 transmit, clean TX audio, and a long list of operator-requested fixes and workflow features.

---

## 📻 Launching Nexus no longer touches your rig — at all

Nexus now opens your radio **read-only**: it reads the rig's actual frequency and mode and displays them, and commands nothing. Park on 40 m LSB for a net, open Nexus to check the bands, and your rig stays exactly where you left it. The first command happens when *you* act — entering a cockpit, clicking a spot, or arming transmit.

Underneath is a new safety latch: every one of the app's transmit paths now asserts the correct mode on the rig immediately before keying, so a transmit can never silently key into the wrong mode. What it means for you: no more launch-time QSYs, no more mode surprises, and the rig's dial you see at startup is the rig's truth, not the app's memory.

## 🌍 The Logbook opens on a spinning globe of everywhere you've worked

Your contacts on a slowly rotating 3-D earth — every worked grid square a glowing dot in its band's color, busier squares brighter, with a hover count. Scroll down and it slides away to give the log the full height; search and the column headers stick below it.

- **Per-band grids (VUCC-style):** a band picker shows one band's squares on their own — your 2 m grids are their own achievement, never pooled with HF.
- It costs nothing when you're not looking: the globe fully unloads when you leave the Logbook, and pauses when scrolled out of view.

## ⏱️ FT8 transmits on time

Your transmission now starts **at the slot boundary**, exactly like WSJT-X. Previously Nexus finished decoding the prior slot before keying — the ~1 second of other stations you could hear at the start of your own over. Decoding now runs in parallel with your transmit.

## 🔊 TX audio is a clean, flat signal

The transmit audio path gained a proper anti-aliased resampler: the FT8/FT4 envelope is now a flat constant-envelope block like WSJT-X's, where it previously carried a periodic amplitude ripple (visible as "beading" in a recording). Less splatter, more of your power in the actual signal.

## 💬 Tempo: a queued message survives anything

- A reply to a station you just decoded now **transmits on the next cycle** — presence tracking follows the same signals you see in the chat.
- **Work keeps Tempo contacts in Tempo** — no more bouncing to FT8 for an FT1 station.
- **Held messages survive restarts**: close Nexus with a message "waiting to send" and it's still queued on relaunch — it goes out when the station is next heard.

## 🗺️ The 3-D Connect globe caught up to the 2-D map

Opening sectors are now **filled and labeled** ("6 m Sporadic-E", "2 m Tropo"), the band-activity heat aura glows like the 2-D map's, and both legends now render on the globe.

## 📋 Logbook workflow

- **Sync QRZ** — fetch your online QRZ logbook (QSOs logged elsewhere + confirmation status) right from the Logbook header.
- **Fetch LoTW** — pull confirmations directly from LoTW with your saved credentials, no manual file download.
- **Import POTA** — import a pota.app hunter/activator export and stamp park references onto your matching QSOs. It never creates or overwrites records.
- **Every column sorts** (Sent, Rcvd, and Park joined the set), and click any callsign to open their QRZ.com page.
- **📢 Spot from anywhere**: a big amber Spot button beside Log on the Phone and CW pages (pre-filled with the call you typed and your frequency), and a per-row Spot in the Logbook to re-spot a logged contact.

## 🎛️ Operating quality-of-life

- **Tuning step remembers itself** — set 10 Hz for CW and it survives mode changes and restarts, per cockpit (FTDX10 report).
- **Spots panel**: a "My privileges" filter shows only spots you may transmit to under your license class (Open class sees everything), and all filters now survive leaving and returning to the view.
- **Classic ↔ Roster layout switching no longer clears your decodes.**
- **Sortable everywhere**: satellite pass schedule, FT8 roster Grid column, Needed board Mode/Zone, Band Activity DT sort, and a fully sortable VHF openings log.
- **Icom IC-7760** joins the rig list (Hamlib CAT), and the **FT-710** setup no longer demands a Silicon Labs driver from a dead link — modern Windows installs it automatically.

## 🔧 Under the hood

- Release pipeline rebuilt: one repo, one history, tag-driven multi-platform builds (Windows, Linux, Raspberry Pi Bookworm + Trixie) with artifact verification — including a guard that the AI CW decoder model can never be silently missing from a build.
- The published source tree is complete and builds from a fresh clone (GPL §6).

---

**⬇️ Downloads** on GitHub Releases (Windows installer, Linux .deb/.AppImage, Raspberry Pi .debs for Bookworm and Trixie) — SHA256SUMS attached. 73!
