Two transmit-path safety fixes lead this release.

- **A rig could get stuck transmitting after a CAT dropout** — reported on a Yaesu's Enhanced USB
  port on Windows. A CAT hiccup while the radio was keyed (an over or a tune) could leave it
  transmitting until you powered it off. Nexus now always sends an unkey through the reopened
  control channel and keeps its stuck-key recovery armed across a reconnect, so a transient CAT
  dropout no longer strands the transmitter. (If the USB device disappears entirely, no software
  can unkey it — a hardware PTT that fails safe, or the rig's Standard port, is the sure guard.)
- **FT8 no longer sends 73 without receiving RR73.** In an ordinary QSO, a DXpedition Fox's
  confirmation addressed to you — from a Fox you'd been chasing on the same band — could be misread
  as your current partner's roger and close the contact early. Fixed.

**New**

- **Opt-in beta updates.** Settings ▸ App updates lets you follow pre-release builds if you'd like
  to help test — off by default, and no one is ever auto-updated onto a beta.
- **The WSJT-X UDP feed can target several destinations at once** — feed a local tool like
  GridTracker and a remote scorer at the same time.

**Also fixed**

- Frequency presets no longer collapse to a single band when a radio has band coverage set.
- The generated-message pane clears after an auto-logged contact (with "Clear DX call after
  logging" on).
- FTDX-101D power no longer dips on transmit.
- The 3-D globe remembers its map layers.
- Added a 60 m FT8/FT4 calling-frequency preset (5.357 MHz).
- The callbook lookup now shows the beam heading from your grid.
- The log editor's time field no longer clips on macOS.
- The Linux (Debian 13 / trixie) "buffer size" startup crash that left the radio engine dead.
- SSTV images no longer import upside down on macOS.

73 — KD9TAW
