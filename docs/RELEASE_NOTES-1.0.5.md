# Nexus 1.0.5 — your contacts reach your logger, four characters of grid go on the air, and a transmit watchdog that actually runs

*2026-08-08*

Logging to N1MM+, HRD and Log4OM, two FT-mode fixes that change what leaves the antenna,
a transmit-safety fix, live drive control, and your recordings and SSTV pictures moved
somewhere you can find them.

**Everyone should take this one.** If you run a separate logger, contacts were never
reaching it. If you run a six-character grid, your FT8 and FT4 QSOs were stalling at the
other end. Neither showed you an error.

---

## Logging — your contacts were not reaching N1MM+, HRD or Log4OM

**Only Field Day contacts were ever sent.** Nexus speaks the WSJT-X UDP protocol on 2237,
and this is exactly why it was so hard to spot: the connection looked alive. N1MM's WSJT
window filled up with decodes, status arrived, everything read as working. But the one
message a logger actually writes a contact from was only ever sent for Field Day. Every
ordinary QSO went into your own log and nowhere else — no error, nothing missing on
screen, and the FAQ saying the path was supported.

Every logged contact goes out now: FT8 and FT4, phone, CW, RTTY, SSTV, and rows you type
into the Logbook by hand.

Worth knowing if you ever chased this yourself: the **N1MM contact broadcast** in Settings
is a different thing and was never going to help. N1MM does not read those packets back
in — they are for club dashboards and live maps. 2237 is the one it logs from.

---

## FT8 / FT4 — what goes on the air

**A six-character grid was going out, and the message format holds four.** If you had set
a six-character locator in Settings, your calls to another station carried something an
FT8 or FT4 message cannot hold. The other operator's software could not decode it as a
grid at all, so it had nothing to auto-reply to and the contact stalled on their side —
for a reason that looked like nothing whatsoever on yours. Your CQ was always fine, since
that path already trimmed it, so this only bit once you answered somebody.

Nexus sends four characters now, the same as WSJT-X. Your settings and your log keep all
six: a six-character locator is correct in both, and it is only what leaves the antenna
that is cut. Reported by kr4fqg.

**A plain click on the waterfall was moving your transmit frequency too.** The hint under
the waterfall says left-click sets RX, Shift or right sets TX, Ctrl sets both — and that
is what WSJT-X does. A plain left click in Nexus moved both markers, so clicking to listen
to someone also moved you on top of them. The only way to stop it was switching Hold Tx
Freq on, which is a workaround for a bug rather than what that switch is for.

A plain click now moves the green RX marker and leaves TX where you put it, whatever Hold
Tx Freq is set to. Double-clicking a decode to work a station is unchanged. Reported by
akhepcat.

---

## Transmit safety — a station Nexus had given up on stayed armed

When you call a station in FT8 or FT4 and it never answers, Nexus stops calling after
eight overs. It kept the QSO open while it waited, which is what you want. But the TX
watchdog — the six-minute limit that exists to stop an unattended radio — was only checked
at the moment an over was being built, and a held-back over is not built. The clock was
never looked at. So the QSO sat there armed with Enable TX still lit, and if that station
was decoded again later, minutes or hours, Nexus answered it without you touching
anything.

On the shipping defaults the give-up always came first — eight FT8 overs is four minutes
against a six-minute watchdog — so on a called station the watchdog could not fire at all.
It now runs while a station is being held back, so the six minutes you set is the six
minutes you get, and TX disarms when it expires.

Nothing about the message sequence, slot timing or when Nexus decides to stop calling has
changed. If you are simply monitoring with TX armed and waiting for a decode you are
unaffected — the watchdog still does not start until there is something it is holding
back.

---

## Drive control — ⚠️ your Pwr slider will sit somewhere different

**Read this one before you transmit.** Both drive sliders now use a curve instead of a
straight line. The range that actually matters on real hardware — 0 up to just past where
ALC engages — turned out to live in only the bottom 15-20% of the old travel, which made
setting drive against an ALC meter far fiddlier than it needed to be.

**What a saved drive level means has not changed — only how far along the slider you move
to reach it.** Your stored setting is exactly what it was. Check it is where you want it
before you work anyone.

While that was being fixed, two other things went with it. Moving Pwr only affected audio
Nexus had not generated yet, and it generates well ahead — an FT8 over is built and queued
in one go, all thirteen seconds of it — so the level was already baked into everything
waiting to go out. Hold Tune, move the slider, and the rig's ALC sat where it was for
several seconds before catching up, which makes it very easy to overshoot into compression
while chasing a control that has not responded. Drive is now applied to each sample as it
leaves for the sound card, so what you set is what goes out on the next fraction of a
second, including audio already queued — scaled rather than rebuilt, so there is no click
when you move it mid-transmission. And the Settings panel's own Tx Power slider applied
only on release rather than while dragging; both track live now.

Reported by g0fqb, who also found the cause.

---

## Your files — recordings and SSTV pictures moved

Recordings now go to **Documents ▸ Nexus ▸ Recordings**, and received SSTV images to
**Pictures ▸ Nexus SSTV**. Both used to sit in Nexus's own configuration folder, which is
hidden, is not the same place for a second radio, and is not somewhere anyone thinks to
look. Several people concluded recording was simply broken, and they were reasonable to.
Pictures somebody sent you are worth being able to find, open and share without going
through the app.

**Your existing SSTV gallery comes with you.** The first time you start this version the
images and their index are moved across, so the gallery looks exactly as it did — nothing
stranded, nothing to re-import. Recordings you already have are left where they are rather
than moved out from under you; only new ones go to Documents. If Windows cannot tell Nexus
where your Documents or Pictures folders are, it carries on using the old location rather
than guessing.

Alongside that: **you can delete a received SSTV image from inside the app** — hover a
thumbnail, ✕, and it asks first, naming the picture. There was no delete anywhere before,
so the gallery only ever grew. Deleting a file by hand no longer leaves a broken thumbnail
either, and pictures you copy into the folder get picked up. **A recording that cannot be
saved now says so**, naming the path it tried, instead of failing silently into an empty
folder.

---

## Smaller things

**The record button is findable again.** It had been reduced to a bare dot in a box, in a
header that also carries the band picker, tuning strip, Tune and Stop TX — small enough
that it got reported as missing before it got reported as hard to see. It says REC next to
the dot now, and the dot is red at rest. **CW has one at all now** — it never did, so
recording a CW contact meant leaving the cockpit.

**The Needed board no longer says "Digital" for stations that are plainly FT8.** A cluster
spot only gives you a frequency, and that path was asking "digital, voice or CW?" and
showing the answer — but the band plan knows 14.074 is FT8 and 14.080 is FT4, and always
did. Off a known watering hole it still says Digital rather than inventing a mode.

**USB and LSB no longer read as different modes** on the new-band and new-mode badges —
ADIF says they are both SSB. FM and AM stay separate, and FT4 stays separate from FT8.
Old imports carrying a band Nexus could not recognise no longer show a permanent "new
band" badge. Entities were already right: European Russia, Asiatic Russia, Kaliningrad and
Franz Josef Land are four separate DXCC entities and always were.

**The Call Roster shows which station you are actually working**, in the transmit colour,
and it does not fade with age like the other rows. Asked for by m7jyfradio and akhepcat.

**The Tempo dial scrolls.** Hover a digit of the big readout and roll — every other
cockpit already worked that way. And **the wheel sensitivity slider now reaches every
dial**; it only ever affected Phone and CW, so anyone who moved it because a free-spinning
mouse was overshooting got no change and no reason why.

**macOS: CAT could fail even with Hamlib installed.** An app launched from Finder or the
Dock gets a fixed `PATH` that does not include Homebrew or MacPorts, so `rigctld` was
invisible to Nexus while working fine from Terminal. Those directories are checked now.

**The Linux downloads say which Linux they need.** Both PC Linux files require Ubuntu
24.04 or newer (`ldd --version` to check) and nothing said so — on anything older the
`.deb` installs without complaint and then does not start. The AppImage is no help there
either: it carries the application's libraries, not the system C library. The build that
produces them is now pinned rather than following whatever GitHub calls "latest", and the
release refuses to publish a Linux binary needing more than the stated minimum.

---

## Notes

1.0.3 and 1.0.4 were bench builds and were never published — 1.0.5 is the next release
after 1.0.2.

Windows, Linux (.deb + AppImage) and both Raspberry Pi bases (bookworm, trixie).
Windows and the AppImage self-update; the `.deb` packages are apt-managed and notify only.
