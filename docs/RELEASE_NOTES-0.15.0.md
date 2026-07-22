# Nexus 0.15.0 — two ways QSOs were quietly going missing, panels you can delete, and DXKeeper

*2026-07-21*

Two of the fixes below are the kind you'd never have noticed. If you upload to LoTW, or you
hunt parks, **please read the first section** — it may explain something you've already seen
and blamed on the other end.

---

## 🛠️ Two silent ways contacts were being lost

### A QSO that LoTW rejected was marked "sent" and never tried again

Nexus signs and uploads through TQSL. TQSL is run in a mode where a record it refuses is
skipped **silently** — it doesn't say which one, it just processes the rest. Nexus was reading
"some records were suppressed" as *success*, and stamping that result across the whole batch.

So a rejected QSO was marked delivered, dropped off the "needs uploading" list permanently,
and never retried — **while never having reached LoTW at all.** If you've ever found a contact
missing from LoTW that Nexus insisted it had uploaded, this was probably why.

Now a suppressed record is treated as a rejection, so those QSOs stay in the queue and go out
on the next attempt. Re-offering one LoTW already has costs nothing — it just deduplicates.
Losing one is permanent.

**This was never specific to any mode.** It could swallow any record TQSL didn't like.

### Park references never reached HRDLog — or any other logger looking for them

Nexus wrote POTA references using the older `SIG`/`SIG_INFO` fields. That convention is shared
with WWFF and special-event stations, which is exactly why ADIF later added dedicated
`POTA_REF`/`MY_POTA_REF` fields — and loggers that read the dedicated ones saw no park at all.

Now both go out, so DXKeeper, HRDLog, QRZ, ClubLog and eQSL all see the reference. Nothing
about your existing log changes; new exports simply carry both.

---

## 🪟 Panels you can actually remove

The Operate cockpit gets **`⊞ Panels`** in its header. Untick a panel and it's *gone* — no
placeholder, no extra window — and the decode lists and roster grow into the space. It stays
gone after a restart.

Removable: the waterfall, Band Activity, Call Roster, Rx Frequency, Stations, and Tx Messages.

This is for the operator who reads the spectrum off the radio's own display and doesn't want it
duplicated on screen. Popping the waterfall out to its own window already worked — but that
still leaves it *running*. Removing it actually stops the work: the panel unmounts and its
120 ms spectrum poll stops with it.

**Undo last change** and **Reset layout** are in the same menu, so there's no arrangement you
can get stuck in. Layouts are per-screen, so a popped-out Operate window keeps its own.

Transmit controls — TX On, Tune, Stop TX, the frequency and mode — are not removable, in any
combination.

## 📖 DXKeeper (DXLab Suite)

Each logged QSO can now go straight into DXKeeper over its TCP Network Service.
**Settings ▸ Integrations ▸ DXKeeper.** Turn on *Configuration ▸ Defaults ▸ Network Service*
in DXKeeper first.

One thing that trips everybody up: the box asks for the **Base Port** (default 52000), which is
what DXKeeper's own panel shows you — but DXKeeper actually listens on **base + 1**. Nexus adds
the 1 for you, and the hint shows the real port so there's no guesswork.

Leave *"Let DXKeeper do the uploads"* **off** unless you want two copies of everything: Nexus
already uploads to LoTW, eQSL, ClubLog and QRZ. (If you have *Auto upload* ticked for Club Log
or QRZ inside DXKeeper, it will upload regardless — untick it there.)

---

## 🏷️ FT1 is now TempoFast · DX1 is now TempoDeep

The two native Nexus protocols have proper names. This is a rename only — **the on-air
waveforms are unchanged**, so a station you worked before this release is unaffected, and the
protocol names never appeared in a transmission in the first place.

Your logbook shows `TempoFast` / `TempoDeep` going forward.

**And they can now be uploaded.** A TempoFast QSO previously wrote a mode value that isn't in
the ADIF standard, so TQSL rejected the record outright — meaning a TempoFast contact could not
have been confirmed anywhere. They now upload as **`MODE=MFSK` with `SUBMODE=TEMPOFAST`**, which
LoTW accepts as a digital-mode contact. Your own logbook still records TempoFast, so you keep
the distinction from TempoDeep.

> **If you make a TempoFast contact:** ask the other operator to log it as **MFSK**. LoTW
> matches on band and mode *group*, so if their side logs a mode LoTW doesn't recognise, their
> upload is dropped and there's no match — no matter what we send.

---

## ✍️ Logging and setup

- **State and Country are now editable** in *Log this QSO*, beside QTH. They were always filled
  in from the QRZ lookup and saved with the contact — you just couldn't see or correct them, so
  fixing a misheard state meant logging first and editing afterwards.
- **POTA/SOTA spots sort** by workable-now, activator, reference, band or mode — and the Sort,
  Band, Program and Mode filters now survive leaving the page and coming back.
- **Band-edge tones moved to Settings ▸ Rig**, where they belong. The cue always fired on phone
  and CW as well as digital; it was just filed in the wrong place.

## 🔧 Under the hood

- Groundwork for running two radios at once: a complete audit of the modem's internal state
  (615 items) with a build check that fails if a future update introduces state nobody has
  accounted for. Two radios sharing that state would produce contacts attributed to the wrong
  band — well-formed, plausible, and wrong.
- Closed a latent memory-corruption path in the FT8 decoder's cross-cycle recovery. Not
  reachable in normal operation, but one wrong call away.

---

**⬇️ Downloads** on GitHub Releases — Windows installer, Linux `.deb`/AppImage, and Raspberry Pi
`.deb`s for Bookworm and Trixie. SHA256SUMS attached. 73!
