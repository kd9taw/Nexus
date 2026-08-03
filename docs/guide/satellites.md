# Satellites

The Satellites section answers "which bird can I work, and when?" for *your*
grid. It predicts amateur-satellite passes over your location, keeps your
favorites at the top, plots each pass, lists the working frequencies, and — if
you have a rotator — can auto-track a bird across the sky through a pass.

Satellites is an opt-in section. Turn it on in the first-run wizard or in
[Settings ▸ Features](settings-reference.md#features). It needs your grid set in
[Settings ▸ Station](settings-reference.md#station) to compute passes.

<!-- TODO: capture screenshot — the Satellites pass list with a favorite starred and its polar plot -->

## The tour

**The pass list** shows upcoming passes over your QTH, favorites first. Each row
reads like a plain-language prediction — for example *"ISS in 38 min · 62° · NW→SE
· 9 min"*: time until AOS, maximum elevation, the direction it travels, and how
long it's up.

**The polar plot** draws the pass across the sky (horizon to zenith), so you can
see where to point and how high it climbs.

**Frequencies** for each bird are listed so you know where to listen and where to
transmit.

You can also drop a **Satellite Passes** pane into the [Connect](connect.md) grid
for an at-a-glance next-passes list beside the map, and turn on the
**Satellites (amateur)** map layer to watch the birds move in real time.

<!-- TODO: capture screenshot — the polar plot of a pass with the AOS/LOS direction and max elevation -->

## Core workflows

### Star your favorites

Click the **⭐** on a bird to favorite it. Favorites sort to the top of the pass
list. The ISS is the easiest first target — favorite it and it leads the list.

### Set a pass alarm

Arm an alarm on a pass and Nexus reminds you before AOS so you don't miss it.
(For the loud, repeating "they're on the air" style of alarm, see the
[DXpedition wake-me alarm](dxpeditions.md#set-a-wake-me-alarm) — the same alarm
machinery.)

### Tune around the passband — and get back on the bird

Working a linear bird means chasing a station across the transponder, so turn
the VFO and Nexus follows: it takes your new dial as your position in the
passband and moves your uplink to match (mirrored, if the transponder inverts).
Doppler keeps correcting around wherever you put yourself.

Tune *outside* the passband and that stops — you've left the transponder as far
as Nexus can tell, which is the right call, because the alternative is dragging
somebody's uplink to a passband edge because they QSY'd to 20 m. **Lock on**,
on the **Dial** line under the bird's name, puts you back: it re-runs the
transponder pick you already made, so the radio, the band and the mode all come
with it, and you land in the middle of the passband again.

It is there from the moment you pick a transponder — that pick tunes the radio
straight away, so the dial is live long before you arm a pass, and the way back
is live with it. It sits with the line that names the rig, and it is there
whether the pass is armed or not and whether or not the bird is up. The one
state it is absent in is the one where it would have to guess: with no
transponder picked there is nothing to put you back onto, and choosing one for
you would be choosing your uplink.

### Log the contact without leaving the pass

The log strip sits in the bird's detail column, directly under the Doppler
readout — the same log strip the Phone and CW cockpits use, with the same
callbook lookup, the same recall card and the same prior-contact history. It is
there whether or not a pass is armed, and it stays there after the bird sets, so
you can catch up on a contact once your hands are free.

Type the call and press Enter **twice**. On a call the strip hasn't seen yet the
first Enter runs the callbook lookup and fills the name and QTH; the second one
logs. (Once the name is filled, one Enter logs.) The band, frequency, mode and
time come from where you already are, and the report defaults to the one for
that mode. Working a bird from a rig Nexus isn't connected to? Open **Log a
contact from another radio** in the strip and set the band, frequency, mode and
UTC time by hand.

**It logs an ordinary contact, not a satellite contact.** This is worth being
plain about, because it decides whether a contact can ever earn satellite
credit. LoTW recognises a satellite QSO by two ADIF fields:

- `PROP_MODE=SAT` — the propagation mode.
- `SAT_NAME` — the satellite, spelled the way LoTW spells it (`AO-7`, not
  `AO7`).

**Nexus writes neither, for any contact it logs.** Getting `SAT_NAME` right
means resolving the bird's designator, and ARRL is explicit that a name LoTW
does not recognise gets the data rejected — so a guess is worse than nothing,
and a logged QSO is permanent. That resolution is not built yet. Until it is, if
you want satellite credit you have to add the two fields yourself: set them in
whatever logger you upload from, or add them to the ADIF before you sign it. Any
record that already carries them — a foreign import, or one you repaired — keeps
them: Nexus writes them out on export and reads them back on import untouched.

**It has to be both fields, and that is why Nexus writes neither rather than the
easy one.** `PROP_MODE=SAT` on its own would be trivial to write and it would be
true — but TQSL will not sign a contact whose propagation mode is `SAT` when it
names no satellite ("PROP_MODE = 'SAT' but no SAT_NAME"), just as it will not
sign one naming a satellite it doesn't know. Either way the contact never
reaches LoTW, so it earns nothing at all: not the satellite credit you were
after, and not the DXCC or WAS credit an ordinary untagged upload does earn. A
half-tag costs you the whole QSO. When you add the fields by hand, add both.

#### Three things this strip does not do yet

None of the three is a decision that satellite work should stay this way. They
are the price of dropping the Phone/CW log strip in unchanged rather than
building a satellite-aware one, and each is meant to be closed.

**Your satellite grids land in the wrong place — in Nexus and at ARRL.**
Nexus decides "was this a satellite QSO?" from `PROP_MODE=SAT` in the record.
(LoTW asks for more than Nexus does: it wants that field *and* the satellite's
name.) With nothing writing it, a contact logged here counts toward neither the
**Satellite VUCC** totals on the Awards screen nor the satellite needs board
that ranks which pass is worth chasing.

Where the grid goes instead depends on the band you were listening on, and
neither answer is right:

- **On 70 cm and 23 cm it lands nowhere.** Those bands have no per-band grid
  slot of their own, and the satellite bucket is the only home they had.
- **On a metre band it lands in the wrong bucket.** 2 m is the downlink of every
  U/V bird — the Fox-1 satellites (AO-85, AO-91, AO-92) and AO-7 on mode B — and
  AO-7's mode A comes down on 10 m, so this is the ordinary case, not a corner.
  The grid is counted toward your **terrestrial** VUCC for that band, which is a
  grid ARRL's rules say a satellite contact does not earn, and LoTW files the
  untagged upload the same way. If you chase VUCC, this one matters: add both
  fields before you sign.

Adding the two fields by hand puts the contact where it belongs in both counts:
edit the ADIF with Nexus closed, exactly as below, and the totals pick it up the
next time Nexus starts.

**During Field Day, this strip logs to the ordinary log, not the contest log.**
The Phone and CW strips switch to the Field Day log while a session is running;
this one is not wired to Field Day yet, so a satellite contact made during FD
goes into your general log and scores the club nothing. Until it is wired, log
satellite contacts made during Field Day from the Phone or CW cockpit *while you
are still on the bird*. Catching up afterwards does not work cleanly: the FD log
stamps every contact with the band the radio is on at the moment you type it, so
a 70 cm pass entered later goes into the contest log — and out to N1MM or
N3FJP — on whatever band you have since moved to.

**The recorded MODE comes from your sideband, so it is wrong on a data mode.**
The strip writes `FM`, `CW`, `AM` or `SSB`, chosen from the mode the rig is
commanded to. That is right for voice and CW work. But the Satellites section is
also reachable on the digital tiers, and there the sideband is not the mode: on
FT8, FT4, Q65, JT65, MSK144, WSPR and FST4 every channel commands USB, so the
contact records `MODE=SSB`; on Tempo's three FM simplex channels (2 m, 1.25 m,
70 cm) it records `MODE=FM`. Either way the record names a voice mode for a
contact you made on a data mode.

Until the strip is tier-aware there are two fixes, and only one of them covers
your tier:

- **On FT8 or FT4**, open **Log a contact from another radio** in the strip and
  pick the mode there before you log.
- **On Q65, JT65, MSK144, WSPR, FST4 or Tempo**, that picker has no entry for
  your mode — it offers SSB, FM, AM, CW, RTTY, FT8 and FT4 and nothing else. Log
  the contact, then open the **Logbook**, edit the record and type the mode into
  its **Mode** field, which takes any text.

**If you ran 0.24.0 through 0.27.x, check your log.** In those versions a
contact logged while a transponder was held picked up `PROP_MODE=SAT` and a
`SAT_NAME` taken from the *catalog* name of the bird — "SAUDISAT 1C (SO-50)",
not "SO-50". LoTW does not recognise those, and the hold is only handed back
when the pass ends (a transponder picked without arming a pass is never handed
back at all), so ordinary contacts made afterwards were tagged too. Nexus no
longer writes any of it.

Existing records are left exactly as they are. Nexus will not rewrite contacts
you already logged: some of them really were satellite QSOs that want the name
corrected, and some were terrestrial contacts that want the tag gone, and
nothing in the record tells the two apart — only you know which pass you were
actually on. To find them, your general log is a plain ADIF file
(`~/.config/tempo/log.adi`, or `%APPDATA%\tempo\log.adi` on Windows): search it
for `SAT_NAME`. Fix them there, with Nexus closed — correct the name, or delete
both fields from the record. There is no way to do it from inside Nexus: the
logbook's edit form does not carry these two fields, and an edit that leaves
them blank deliberately *preserves* what is stored, so that an ordinary
busted-call fix cannot silently strip a satellite tag off a record that earned
it.

### Pin the radio a pass uses

On a multi-radio station the readiness rail names the rig a pick routed to, and
the band and mode class it routed on. If two rigs cover the downlink, the pick
goes wherever your routing says — which is usually what you want, and sometimes
not. **🔓 pin this radio** holds the pass on the radio you are on: it stays 🔒
pinned until you click it again, and no pick hands the bird to another rig
meanwhile. It is the same switch as the 🔒 beside the radio selector in the top
bar, put where you are working the pass. Pinning does not re-tune anything — it
decides where the *next* pick lands.

### Auto-track with a rotator

1. Configure your rotator in
   [Settings ▸ Rig / CAT](settings-reference.md#rig--cat) — pick your model and
   its COM port and Nexus runs the control daemon for you. No hardware? Pick the
   **Dummy (testing)** model, or run `rotctld -m 1` and point Nexus at
   `127.0.0.1:4533` to watch it work.
2. **Arm rotor track** on a pass. Nexus slews the rotator to follow the bird
   across the sky through the pass; the compass shows the track, with °T and °M
   side by side, and a STOP control halts it.

The section is read-only until you arm a track — it won't touch your rotator on
its own.

**If the rotator stops answering mid-pass**, Nexus gives up on it rather than
hammering it for the rest of the pass — and gives up on *only* it. The pass
carries on: your transponder stays picked, Doppler keeps correcting the radio,
and the sky dome keeps showing where the bird is so you can turn the antenna
yourself. The track says so plainly (the badge and the readiness rail both read
"rotor stopped answering") and stops showing commanded angles, because it has
stopped commanding anything. The rotor strip in a cockpit header keeps naming
the bird and the ■ that stops the track, beside the dim "ROTOR —" for the mast.

At LOS that pass still sends one stop, in case the controller came back — but it
will **not** park or go to ready, even if you configured one. The pointing was
handed to you, so the antenna stays where you left it.

## Honest limits

- Passes are computed for your grid — **set your Maidenhead locator** first or
  the predictions can't run.
- The bird list is **not everything in orbit** — it is the amateur population:
  satellites with an amateur transmitter on record. Around 430 birds are
  listed and around 367 of those carry current orbital elements.
- Rotor auto-track drives an **az/el** rotator through Hamlib `rotctld`
  (elevation is followed through the pass; an azimuth-only rotator is detected
  automatically and driven in azimuth alone); test it with the Dummy model
  before you trust it on real hardware.

### Where does the bird list come from, and what does a bird's status mean?

The list is **derived, not copied**. It starts from the
[SatNOGS database](https://db.satnogs.org) — the community record of which
satellites carry which transmitters — and keeps every satellite with an
amateur transmitter: one SatNOGS labels *Amateur*, or one transmitting in an
ITU amateur-satellite allocation (the band test is what keeps SO-50, whose
transmitters are all filed as "Unknown"). Orbital elements for those birds are
then assembled from three sources, freshest epoch winning: CelesTrak's
`amateur` group, CelesTrak's `satnogs` group, and the SatNOGS element service.
That last one matters — it is the only source for a bird still catalogued
under a placeholder number, and for birds CelesTrak has no elements for at all.

The list is rebuilt every six hours by the project's mirror, so **a bird going
on or offline reaches you within six hours of SatNOGS recording it** — no app
update needed. Each bird's status rides with it:

- **alive** — in orbit, with something amateur transmitting. These are the
  birds that carry elements and appear on the map, in the schedule and in the
  pass list.
- **alive but silent** — in orbit, but the catalog lists no working amateur
  transmitter any more. The pass geometry would still be real; there is
  nothing to work on it.
- **dead** — reported silent.
- **re-entered** — gone. Kept in the list for six months after re-entry, so a
  favorite that stops working has a row that says why, then dropped.
- **pre-launch** — on record but not yet deployed. Nothing to work yet.

Only *alive* birds carry elements, so a bird in any other state shows in the
list with its status and "no elements" rather than a position. **Your ★ stays
put either way** — a bird that dies never vanishes out from under its star,
and search reaches the whole catalog so you can always find it to unstar it.

### How current are the elements?

Every prediction runs on orbital elements (TLEs), and elements decay: a fresh
set predicts a pass to the second, an old one drifts — and pointing and
Doppler drift with it. Nexus keeps the elements current for you and tells you
plainly when it can't:

- **Where they come from.** Elements refresh in the background a few times a
  day from the project's mirror described above (CelesTrak data courtesy of
  Dr. T.S. Kelso; population and status data from SatNOGS / Libre Space
  Foundation, CC BY-SA 4.0); the mirror exists so a fleet of installs never
  hammers the sources. If the mirror itself is unreachable for a day, Nexus
  falls back to fetching CelesTrak's amateur group directly — a shorter list
  of 97, with every other bird keeping its row marked "no elements" until the
  mirror is back. The Satellites section and Settings ▸ Orbital elements show
  the age, fetch time and source. **Update now** forces a refresh; **Import
  from file** loads a downloaded TLE/keps file — the path for offline shacks
  and brand-new launches no source carries yet.
- **Past 14 days** the age is badged stale, and arming a pass asks first —
  refresh right there, or arm anyway with your eyes open.
- **Past 30 days** SGP4 accuracy is genuinely gone, so anything that would
  move the radio or rotator refuses — naming the bird and the age — rather
  than drive the antenna off a fiction.
- **A pass runs on the elements it armed with** (a pass is minutes long; a
  mid-pass swap would jump the antenna). The readiness rail shows the age of
  the frozen set.
- **Renames don't orphan your stars.** CelesTrak occasionally renames a bird;
  your ★s, alarms and schedule keep working because Nexus remembers the
  catalog number behind each name.
- A bad or empty download never replaces a good cache, and nothing waits on
  the network — Nexus serves the best elements it has and refreshes behind
  you.

## Related guides

- [Connect — map + propagation](connect.md) (Satellite Passes pane, live map layer)
- [Settings reference](settings-reference.md) (rotator setup)
- [DXpeditions](dxpeditions.md) (the same alarm machinery)
