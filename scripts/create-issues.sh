#!/usr/bin/env bash
# Retire the private next-release backlog into GitHub Issues — ONE TIME, on approval.
#
#   ./scripts/create-issues.sh                 # dry run: print every issue, create nothing
#   ./scripts/create-issues.sh --wave 1        # dry run, first wave only
#   ./scripts/create-issues.sh --wave 1 --yes  # actually create the first wave
#   ./scripts/create-issues.sh --wave all --yes
#
# WITHOUT --yes NOTHING IS CREATED. That is the whole safety model: the default run is a
# review surface, and the maintainer approves the list before a single issue exists.
#
# This script only ever calls `gh issue create` and `gh issue list`. It never comments,
# never closes, never edits, never labels an existing issue. If you want any of that, do
# it by hand — an automated tracker write is not something this repo does.
#
# The list came from a full read of the 2,067-line backlog; the classification (SHIPPED /
# DUPLICATE / OPEN / DROP) and the evidence for each verdict live in the private triage
# manifest, not here. Only the OPEN items are below.
#
# Re-running is safe: an issue whose exact title already exists (open or closed) is
# skipped, so a partial run can be finished without creating duplicates.
set -euo pipefail

REPO_SLUG="kd9taw/nexus"

bold() { printf '\n\033[1m%s\033[0m\n' "$*"; }
ok()   { printf '  \033[32m✓\033[0m %s\n' "$*"; }
skip() { printf '  \033[33m•\033[0m %s\n' "$*"; }
die()  { printf '\n\033[31m✗ %s\033[0m\n' "$*" >&2; exit 1; }

CONFIRM=0
WAVE=all
while [ $# -gt 0 ]; do
  case "$1" in
    --yes) CONFIRM=1 ;;
    --wave) shift; WAVE="${1:-}" ;;
    --wave=*) WAVE="${1#*=}" ;;
    -h|--help) sed -n '2,9p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) die "unknown option: $1" ;;
  esac
  shift
done
case "$WAVE" in 1|2|all) ;; *) die "--wave must be 1, 2 or all (got '$WAVE')" ;; esac

command -v gh >/dev/null 2>&1 || die "gh CLI not found"

# Existing titles, so a re-run cannot duplicate. Fetched once. In dry-run mode a failure
# here is not fatal — printing the list must work with no network and no auth.
EXISTING=""
if gh auth status >/dev/null 2>&1; then
  EXISTING="$(gh issue list --repo "$REPO_SLUG" --state all --limit 300 \
    --json title --jq '.[].title' 2>/dev/null || true)"
elif [ "$CONFIRM" = 1 ]; then
  die "gh is not authenticated — run 'gh auth login' first"
else
  skip "gh not authenticated: cannot check for existing issues (dry run continues)"
fi

CREATED=0; SKIPPED=0; PLANNED=0

issue() {
  local id="$1" wave="$2" labels="$3" title="$4" body
  body="$(cat)"

  # Match the tracker's existing convention: every issue already on the repo carries a
  # [bug]/[feat] prefix, and a half-prefixed tracker stops being scannable in a list view.
  # The GitHub label is still applied — this is what shows in notifications and search.
  # Prefixing BEFORE the duplicate check below is deliberate, so a re-run compares the
  # title that was actually created.
  case "$labels" in
    *bug*)           title="[bug] $title" ;;
    *documentation*) title="[docs] $title" ;;
    *enhancement*)   title="[feat] $title" ;;
  esac

  if [ "$WAVE" != all ] && [ "$wave" != "$WAVE" ]; then return 0; fi

  if [ -n "$EXISTING" ] && printf '%s\n' "$EXISTING" | grep -Fxq "$title"; then
    skip "$id  already exists: $title"
    SKIPPED=$((SKIPPED + 1))
    return 0
  fi

  if [ "$CONFIRM" = 1 ]; then
    gh issue create --repo "$REPO_SLUG" --title "$title" --label "$labels" --body "$body" \
      >/dev/null || die "$id: gh issue create failed"
    ok "$id  created [$labels]  $title"
    CREATED=$((CREATED + 1))
  else
    PLANNED=$((PLANNED + 1))
    printf '\n\033[1m── %s  (wave %s, label: %s)\033[0m\n' "$id" "$wave" "$labels"
    printf '\033[1m%s\033[0m\n\n' "$title"
    printf '%s\n' "$body" | sed 's/^/    /'
  fi
}

if [ "$CONFIRM" = 1 ]; then
  bold "Creating issues on $REPO_SLUG (wave: $WAVE)"
else
  bold "DRY RUN — nothing will be created (wave: $WAVE). Add --yes to create."
fi

# ─────────────────────────────────────────────────────────────────────────────
# WAVE 1 — the thirteen to open first
# ─────────────────────────────────────────────────────────────────────────────

issue O1 1 bug \
  "SSTV gallery: no way to delete an image, and deleting the file leaves a broken thumbnail" <<'BODY'
Two defects that compound each other.

**There is no delete command anywhere.** Grepping `crates/`, `src-tauri/src` and `ui/src` for an
SSTV or gallery delete/remove command returns nothing. Once an image decodes it is permanent as far
as the app is concerned; the gallery only ever grows.

**The startup seed never checks the files still exist.** `src-tauri/src/lib.rs` reads
`gallery.json` from the gallery directory and hands the entries straight to `load_sstv_gallery` —
no `stat`, no filter, no reconciliation against the directory. Delete the `.bmp` files by hand and
every entry survives in the index and renders as a thumbnail over a missing file.

They compound: with no in-app delete, deleting by hand is the only way to clear the gallery, and
that is precisely the path that leaves it broken.

### What to build
- Delete from inside the app — per image, and a way to clear a selection or the lot. One action
  writes the file removal *and* the `gallery.json` update so the two cannot drift.
- Reconcile the index against the directory on load and after any delete: drop entries whose file
  is gone, and consider adopting `.bmp` files present on disk but absent from the index (the
  inverse drift, which a hand-copied file already causes today).

### Decisions
- **Deleting a received image is irreversible** — a decoded SSTV picture is the only copy of
  something someone sent you. Confirm before deleting, and decide whether "remove from gallery" and
  "delete the file" should be distinct actions. The Logbook's purge dialog (type-the-word
  confirmation) is the in-repo precedent for the destructive end.
- Failure UX for an entry whose file vanishes while the app is running: silently drop it, or show
  it as missing? Silently dropping is friendlier but hides a real disk problem.
BODY

issue O2 1 bug \
  "Audio recording writes no file, and a failed write says nothing" <<'BODY'
Reported: "recording is not writing the file."

### Where the code says the files go
Both are 12 kHz mono WAV under the settings directory (`src-tauri/src/lib.rs`):
- `voice_dir()` → `<settings dir>/voice/slot<N>.wav`, one per voice-keyer slot, overwritten on
  re-record and removed on clear.
- `recordings_dir()` → `<settings dir>/recordings/qso-<ms>.wav`, or `qso-<CALL>-<ms>.wav` when a
  call is known. The millisecond stamp exists so a stop-then-start inside one second cannot clobber
  the previous file.

### ⚠️ Do not start from the original evidence — it was from the wrong directory
The first report noted both directories absent under a roaming-profile `tempo` folder. That folder
held only a months-old `settings.json` listing no radios, so it is not what the running build
reads. **"The directory does not exist" is not evidence the write fails.** First establish where
the running app actually resolves `settings_path()` on the affected machine, then look there. It
may be writing correctly the whole time.

### If it *is* failing, where to look
- `start_voice_recording` and the stop path — is the buffer taken and dropped without a write?
  `cancel_voice_recording` is "stop, take and drop the buffer", and the voice-keyer hide path calls
  it, so a UI teardown racing the stop could discard a take that was never written.
- `std::fs::create_dir_all` — both writers create the directory first and a failure is swallowed.
- `tempo_core::wavfile::write_wav_i16`'s return value is discarded at the call site.

**Worth fixing regardless of the cause: nothing surfaces a write error to the operator.** A
recording that fails silently is the same honesty defect as a feature that reports success while
doing nothing.
BODY

issue O5 1 enhancement \
  "Multi-operator: set the operator, and export one ADIF per operator" <<'BODY'
Asked for by a POTA operator running one computer with two operators: the ability to set the
operator and do a multi-operator export in the ADIF. "Double the contacts, double the fun."

**The data model already supports this.** `crates/tempo-core/src/logbook.rs` carries `operator` and
`station_callsign` — "who operated / which station logged it (multi-op and club logs depend on the
distinction)". Both are written on export, parsed on import, and a record arriving with its own
`STATION_CALLSIGN` from an imported multi-op log is already handled.

**What is missing is the whole workflow.** Settings has only `mycall` — there is no operator concept
above the record. Nothing sets `operator` when a QSO is logged, there is no way to switch operator
mid-session, and there is no per-operator export. The field is plumbed and permanently empty.

### Why it matters
POTA requires each operator to submit their own log. Two ops sharing one rig and one laptop have
exactly two options today and both are bad: log everything under one call and lose the second op's
credit, or stop and hand-edit the ADIF afterwards.

### What to build
1. **An operator selector, prominent and fast.** Two people swap the mic every few QSOs; this
   cannot live three clicks deep in Settings. The current operator must always be visible, because
   a wrong operator is silent and only discovered at submission.
2. **Set `OPERATOR` on every logged QSO** from that selection; keep `STATION_CALLSIGN` as the
   station's call — that is the distinction ADIF exists to express and the one POTA scoring reads.
3. **Export split by operator** — one ADIF per operator plus the combined file, with names obvious
   at a glance, because these get uploaded from a phone in a car park.
4. **An operator roster, not free text.** Typing a call per QSO reintroduces the error this fixes.

### Decisions before building
- Where does the selector live — the cockpit log strip (fastest, costs vertical space in a strip
  already under pressure) or a header control (always visible, further from the hand)?
- Does the operator persist across a restart, or reset? A park runs across a lunch break, and a
  stale operator after a restart silently mislabels a run.
- Should the park reference be per-operator too? Two ops at one park share it — but two ops working
  different references from one camp is a real scenario.
- Does it extend to Field Day, where multi-op is the norm? That argues for building it generically.
BODY

issue O9 1 bug \
  "Tempo chat: chunk reassembly is keyed by message id alone, so two stations can merge" <<'BODY'
`Reassembler.buffers` in `crates/tempo-core/src/text.rs` is a `HashMap<char, Partial>` keyed on the
message id, and ids cycle `A..Z`. Two stations transmitting message `B` in the same window have
their chunks merged into one garbled message, and a stale partial `B` can merge with a new `B`
26 messages later.

Invisible with one peer on a quiet band; a genuine fault on a busy 6 m opening with several Tempo
stations. The module already documents it as a known accepted limit.

### ⚠️ The obvious fix was tried and is wrong — read the source comment first
Keying on `(sender, id)` does not work: `accept()` records `from` on the **first chunk of an id and
deliberately ignores it afterwards**, because the talker context drifts between a message's chunks
on a live band, so routing on it would *split* a message that should assemble.

**The real fix is a sender id in the frame**, which means it depends on the header-packing work —
that item is already reworking the 3-byte header and is where the bits come from. Do not open this
one on its own expecting a small change.
BODY

issue O10 1 enhancement \
  "FT modes: stop calling a station that has gone silent" <<'BODY'
Ruling: keep calling as long as the station is still decoding — it is out there — and stop only
when it has NOT been decoded for about 4 cycles. Re-arm only by re-clicking their contact or decode.
The existing overs-cap behaviour is explicitly not wanted, because it stops you calling a station
you can plainly hear.

**⚠️ This is FT TX/sequencing and is gated: it needs a direct maintainer yes before any code.**
WSJT-X behaviour is a compatibility contract and on-air correctness cannot be proven in CI.

### Today's mechanism counts the wrong thing
`directed_max_calls` → `Station::call_cap`, enforced in `QsoStation::tx_capped()`. But `tx_count`
increments only in `after_tx()` and resets in exactly one automatic place — when the partner
advances our state. So it means "overs since the partner last advanced us", **not** "cycles since I
heard them". Decoding the DX does nothing; a decode matching no transition arm falls through and the
counter keeps climbing. (Also: 8 overs is 16 slots ≈ **4 minutes** on FT8, not 2 — we transmit every
other slot.)

### Three real defects found while scoping it, independent of the feature
1. **No latch.** The cap only withholds the over. QSO mode stays armed, Enable-TX stays lit, and if
   the DX later advances the state the counter zeroes and **we key up again with no operator click**.
   Worse, the wall-clock watchdog lives inside the transmitting branch, so a capped station never
   times out — it can sit armed and spontaneously transmit minutes later.
2. **No UI.** `directedMaxCalls` exists only in a test fixture — absent from `types.ts` and the
   settings panel. Changing it means hand-editing `settings.json`.
3. **A bug the new rule fixes:** in `AwaitRoger` there is no grid arm, so a DX who did not copy your
   report and re-sends their grid never advances the state — and Nexus stops calling a station it is
   decoding perfectly every cycle.

### Shape of the build
Track the last slot the DX was heard in (from the decode observation pass, keyed on the **slot
number** — that pass can run twice per slot — and seeded from the click context so a stale spot
cannot trip it instantly). Latch when `slot - last_heard` exceeds the configured cycles. **Clear the
latch wherever a QSO is (re)started, including the WSJT-X-UDP reply path — that one line's absence
would leave a re-click silently transmitting nothing.** Surface it as the existing "stalled" state.

**Do not repurpose `tx_count`.** It is load-bearing for auto-log (the proof the closing RR73 went
out) and for CQ resume; resetting it on a plain decode would silently un-arm auto-logging.

### WSJT-X deviation, deliberate
Stock WSJT-X has no per-station give-up at all — only the 6-minute TX watchdog. Nexus already
deviates. This rule touches only whether an over is emitted, never the message sequence, state
machine, slot timing or message selection, and it is strictly in the transmit-*less* direction: it
can never transmit where WSJT-X would not, only stop earlier.
BODY

issue O12 1 bug \
  "Memory channels, watchlist, chase sets and alarms live in browser storage and are not backed up" <<'BODY'
An audit of what survives an upgrade found that `settings.json` is completely safe — the struct
carries `#[serde(rename_all = "camelCase", default)]` at the **struct level**, so every field is
optional on load, an older file can never break, and no existing value is dropped.

**The risk is entirely in the other store, and it holds real operator data.**

37 keys live in WebView2 `localStorage`: invisible to the operator, not beside `settings.json`, not
covered by any backup of it, and **not per-profile**. Most are cosmetic and belong there. These are
not:

- `nexus.memory.bank.v1` / `.v2` — the radio **memory channels**. UI-only; there is no Rust side.
- `nexus.watchlist` — the callsigns being watched.
- `nexus.sats.chasing` / `nexus.dxped.chasing` — the chase sets that drive the schedule, the
  best-pass picks and the alarms.
- `nexus.sats.alarms` / `nexus.dxped.alarms` and their `.fired` twins — armed wake-me alarms.
- `nexus.profiles`, `nexus.navOrder`, `nexus.connect.config`, the detached-waterfall panel state.
- `nexus-ui-scale-mode` / `nexus-ui-scale-cap` — **accessibility settings.** Losing these is worse
  than cosmetic for an operator who needs them.

### How they actually get lost
1. **Changing the Tauri identifier orphans everything.** It is still `com.kd9taw.tempo` while the
   product is Nexus. Any tidy-up to `com.kd9taw.nexus` silently wipes all 37 keys on every installed
   machine, with no error and no migration. ⭐ **Do not rename it without shipping a migration first.**
2. Uninstall-then-reinstall (rather than upgrade in place) can clear the WebView2 data folder.
3. `localStorage` is origin-scoped — changing the asset protocol or origin loses the lot.
4. There is no migration story at all: `memory.bank.v1` *and* `.v2` both exist, because a schema
   change was already handled by minting a new key and orphaning the old one.

### What to do, ranked
1. **Move the operator data into `settings.json` or sibling journals** — memory bank, watchlist,
   chase sets, alarms, profiles. Durable, per-profile, backed up with everything else, survives a
   WebView2 reset. This is the one that matters.
2. Leave the cosmetics in `localStorage`; that is what it is for.
3. **Pin the identifier** with a comment and a test asserting its value, so the landmine in (1)
   cannot be stepped on by a well-meaning rename.
4. Add settings export/import, so a backup is possible at all.
BODY

issue O15 1 bug \
  "Every Russian station is badged as a new entity — entity slots key on a free-text country" <<'BODY'
`ui/src/features/callHistory.ts` keys entity slots on the **COUNTRY string** carried by the record.
A real 11k-QSO log holds "Russia" (×153), "Asiatic Russia" (×60) and "European Russia" (×2) as three
separate strings, so the same entity occupies three slots and any Russian station can show a false
new-entity badge.

This is one of a family of false-need classes found by censusing a real log against the needs
engine; it is the worst of them because it fires constantly on a common entity.

**The fix is a design call, not a patch:** the frontend has no DXCC resolution of its own. Either
put `cty.dat` on the frontend, or expose a Tauri command that resolves an entity from a callsign and
key the slots on the resolved entity id instead of the stored text.

Related but separate: an exact-call exception in `cty.dat` can override the stored country entirely
(RI0SP is European Russia whatever the record says), which is another reason the stored string
cannot be the key.
BODY

issue O16 1 bug \
  "The needed board compares raw mode tokens, so FT4 against a stored MFSK reads as a new mode" <<'BODY'
`callHistory.ts` compares **raw mode tokens**. A QSO stored as `MFSK` (which is what many loggers
and imports write for FT4) does not match a live `FT4` decode, so the station is badged as a new
mode when it is not.

The backend already has a mode normalizer — the frontend has none exposed. Expose the existing one
rather than writing a second table; two mode tables in two languages is the exact drift that caused
the sort-by-need ranking bug (three TS mirrors of one backend ordering).

Part of the same false-need family as the entity-by-country-string issue.
BODY

issue O17 1 bug \
  "Re-importing the same QSO under a different mode spelling stores it twice" <<'BODY'
`dedup_key` in `crates/tempo-core/src/logbook.rs` builds its key from the **raw mode string**:

    (call.to_ascii_uppercase(), band.to_ascii_lowercase(), mode.to_ascii_uppercase(), when_unix)

while `reconcile::key` uses the mode **class**. So the same QSO re-imported with a different but
equivalent mode spelling (`FT4` vs `MFSK`, `PSK31` vs `BPSK31`, a submode vs its parent) passes
dedup and is stored a second time.

Two routines that answer "is this the same contact?" disagree, and only one of them guards the
import path.

Fix by giving both the same notion of mode identity — normalize in `dedup_key`, or have both call
one shared key builder. Add the failing case as a test first: import a file, import it again with
the mode respelled, assert the log length is unchanged.
BODY

issue O19 1 bug \
  "An imported QSO with an unparseable BAND and FREQ 0 is a permanent \"new band\"" <<'BODY'
Two facts combine into a permanent false badge:

1. `from_band_token` and `from_label` disagree on `-fm` suffixes, so some band strings parse in one
   path and not the other.
2. **`freq_mhz` is never used as a band fallback** — if the BAND token does not parse, nothing else
   is tried.

And every imported row in a real 11k-QSO log carries `FREQ=0.000000`, so there is no frequency to
fall back to even if the fallback existed. Net effect: **an unparseable BAND is a permanent
NewBand** for that contact, forever, on every poll.

Fix: one band-resolution path used by both callers, with the frequency as an explicit fallback when
the token does not parse — and a decision about what to do when neither is usable (today it silently
becomes "needed", which is the wrong default; unknown should not read as new).

Part of the same false-need family as the entity and mode issues.
BODY

issue O22 1 bug \
  "The N1MM/N3FJP broadcast hardcodes radio number 1" <<'BODY'
`crates/tempo-net/src/n1mm.rs` emits a literal `<radionr>1</radionr>` in every broadcast.

For a single-radio station that is correct by accident. For any multi-radio station — which Nexus
supports, including simultaneous radios — every packet claims radio 1 regardless of which rig made
the contact, so an N1MM or N3FJP instance consuming the feed attributes the whole session to one
radio.

Small, self-contained: carry the radio id through to the broadcast and emit it. Worth a test that
pins the field against a two-radio fixture, because a wrong value here is silent — the receiving
logger accepts it happily.
BODY

issue O25 1 enhancement \
  "Guard every rig model in the catalog against the bundled Hamlib" <<'BODY'
A rig model shipped in the catalog that the bundled Hamlib cannot load produces a daemon that
refuses to start, and nothing in the build catches it. That has happened: a catalog entry whose
number was real in Hamlib's `riglist.h` but whose backend was not compiled into the shipped library.

**The verification that missed it is instructive.** A check ran `strings libhamlib-4.dll | grep`
for the backend name, found matches, and concluded the backend shipped. It does not follow — source
strings can be present while the backend is not registered in the build. The honest check is one
command: start `rigctld -m <n>` and see whether it comes up.

### What to build
Every model number in `crates/tempo-audio/src/rigmodels.rs` must be loadable by the Hamlib we ship.
`rigctl -m <n> --dump-caps` per entry is a slow but decisive sweep, and
`scripts/gen-hamlib-serial-speeds.mjs` already dumps caps for every model — its fixture could carry
a `loadable` flag at no extra cost, with a test asserting every catalog entry is flagged loadable.

**The guard generalises**: it catches any future entry whose number is wrong or whose backend is
absent, which is a class of failure that is completely silent until an operator selects it.
BODY

issue O28 1 bug \
  "A band change commands the wrong dial first — the band plan is tier-scoped, not mode-scoped" <<'BODY'
Reported: "When I click on spots or use the dropdown, it goes to some type of base frequency, then
quickly changes to what it was supposed to be. For example I was on SSTV, switched to 10 m, it went
to the FT8 frequency, then quickly changed to the configured SSTV frequency."

It looks cosmetic and is not: it is a **double retune**. The rig is actually commanded to the wrong
dial and then commanded again. On a slow-serial rig that is two CAT round-trips per band change, and
anyone watching (or a spot click) sees the wrong frequency briefly.

### Root cause of the FIRST write, confirmed by reading
`Engine::band_plan()` builds from `bandplan::band_plan_for(tier)` and applies the operator's working
frequency overrides **only when `mode_name` is non-empty** — and `mode_name` is set solely for the
FT8 and FT4 tiers:

    let mode_name = match tier { Tier::Ft8 => "FT8", Tier::Ft4 => "FT4", _ => "" };

So in SSTV, Phone, CW and RTTY the band dropdown is fed the **digital (FT8) plan carrying no working
frequency overrides**. Picking 10 m therefore commands the FT8 dial. (The top bar's own doc comment
already admits it is fed the digital plan.) The mode-aware channel list elsewhere handles SSTV and
RTTY correctly, so the two disagree.

Also confirmed: `mode_home` has arms for Digital, Phone, CW and RTTY but **none for SSTV**, and SSTV
rides the Phone operating mode — so its "home" is the phone segment start, not the SSTV calling
channel.

### A second, independent contributor — narrowed, not proven
`Engine::set_frequency` calls `set_active_radio()` *before* applying the requested dial, and that
handoff restores the incoming radio's persisted `last_dial_mhz` / `last_band`. It early-returns when
the radio is already active, so it fires only on a real handoff — consistent with the report coming
from a dual-radio station and with it not being universally reproducible.

### ⚠️ How to fix it, and how not to
Making `band_plan()` **mode-aware** is likely the real fix and removes the double write at its
source. Do **not** hide the intermediate state in the UI, and do not patch either site before the
ordering is actually observed: instrument the tune path (log every dial write with its caller) and
reproduce. This is the band-routing and tune path, TX-adjacent, and guessing here has cost a release
before.
BODY

# ─────────────────────────────────────────────────────────────────────────────
# WAVE 2
# ─────────────────────────────────────────────────────────────────────────────

issue O3 2 enhancement \
  "The app data folder is still called \"tempo\" — moving it is a migration, not a rename" <<'BODY'
The application data directory and the Tauri identifier (`com.kd9taw.tempo`) still carry the
pre-rename project name, on a product whose every other surface says Nexus.

### Why this is not a rename
That directory holds everything the operator owns and cannot regenerate: `settings.json` (rig
profiles, routing rules, per-radio uplink consents), the logbook, the Field Day log, conversations,
the TLE snapshot cache, voice-keyer slots and recordings.

A build that changes the identifier without migrating **silently starts a new empty profile**: the
operator opens it and their log, their radios and their voice keyer are gone, while the old data
sits intact one folder away. That is the worst possible first impression, and it is exactly what
"just rename it" produces. The same rename also wipes every browser-storage key (see the operator-data
issue) — memory channels, watchlist, chase sets, alarms, accessibility scale.

### The actual job
Detect the old directory; move or copy it; prove the move atomic-or-idempotent (a half-migrated
profile must not be reachable); handle **both** directories existing (a downgrade then upgrade);
leave the old one in place until the new one is proven readable; and say plainly in the release
notes what moved.

`shared_data_dir()` and the existing FCC/TLE cache paths are the precedent for a per-file migration.
There is no precedent for moving the whole root.

**Do it as its own release with its own testing pass. Never fold it into a feature batch.**
BODY

issue O4 2 enhancement \
  "APRS map has no street basemap" <<'BODY'
Asked: is there a way to change the APRS map to a street map?

**Today: no, and it is not a hidden setting.** `ui/src/components/MapView.tsx` is Canvas2D with
d3-geo — no tiles, no WebGL. The world is bundled GeoJSON (country outlines plus CQ zones).
Grepping `ui/src` for `tileLayer`, `{z}/{x}/{y}` or a tile host returns nothing. Every map in Nexus
— Connect, band map, satellites, APRS — is that one component.

**Why APRS is the case that actually needs it.** For HF propagation a coastline map is correct: you
care about paths and grids, not roads. APRS is the opposite — it plots a mobile station's track, and
"which road is he on" is the whole question. At the zoom an APRS operator wants, our map is blank.

### The realistic options
- **❌ Google Maps.** Their terms require the official Maps SDK/JS API and forbid using their tiles
  in a generic renderer; it also needs an API key with billing attached, per install. Not viable for
  a free, offline-capable desktop app. This should be said plainly rather than left open.
- **✅ OSM raster tiles** via a provider (MapTiler / Thunderforest / Stadia) or self-hosted.
  ⚠️ OSM's own tile servers prohibit bulk app use — a shipped app pointing at them is a policy
  violation and gets blocked. A provider means an API key, and attribution is mandatory and must be
  visible.
- **✅ Vector tiles** (MapLibre GL) — better looking, offline-capable via MBTiles, but a real
  dependency and a WebGL surface next to a Canvas2D one.
- **✅ Offline MBTiles the operator supplies.** No key, no policy problem, no network; suits a field
  laptop and matches this app's offline leanings.

### Decisions before anyone builds
1. **Which basemap source, and who pays for the key** — us (shipped, rate-limited, our bill) or the
   operator (a field in Settings). This is the load-bearing question.
2. **Tiles only on the APRS map, or everywhere?** A tile basemap under the HF propagation map is
   noise; under APRS it is the point. Suggested: APRS plus satellite ground tracks, opt-in.
3. **Offline behaviour.** Nexus is used at field sites with no network. The vector map must remain
   the fallback, not a blank grey grid.
4. **Attribution surface** — provider credit visible on the map, plus a NOTICE entry (the
   OSS-integration checklist applies).
BODY

issue O6 2 enhancement \
  "Push logs to World Radio League" <<'BODY'
Asked: integrate Nexus with WRL and push logs to it.

**This is the easy direction and almost all of it already exists** — adding a service here is
filling in a template, not building a mechanism.

### The proven pattern to copy
- **Per-QSO push at log time:** `push_to_hrd` and `push_to_n1mm` in `crates/tempo-app/src/engine.rs`.
  A `push_to_wrl` sits beside them.
- **⭐ Store-and-forward with retry already exists:** `take_pending_uploads` / `requeue_upload` and
  the `PendingUpload` queue. This is the part that matters for a POTA or SOTA operator — contacts
  made with no signal queue and go up when the phone finds a bar. Any new connector rides this
  rather than inventing its own retry.
- **Toggle shape:** `Settings::<service>_upload: bool`, driven by `set_upload_toggles`.
- **Credentials go in the OS keychain, never in settings** — the rule is documented at the eQSL and
  QRZ credential sites.

### ⚠️ The one trap, and it has bitten before
**The push must happen in the backend worker, not the UI.** A UI-side push meant a contact logged
while that view was closed — or logged and then navigated away from — never went anywhere, silently.
Everything routes through `log_qso` → the worker.

### Unknowns to establish before any estimate
1. Does WRL have a documented upload API at all, and does it accept ADIF? If it is browser-upload
   only, this becomes "export a WRL-shaped file" — a different and smaller feature, still worth
   doing, but do not promise sync.
2. Auth model — API key, OAuth, or username/password. OAuth would be the first in this codebase and
   is materially bigger than a key.
3. Per-QSO or batch? The funnel does per-QSO with retry; a batch API needs a flush policy, not a new
   mechanism.
4. Duplicate handling on their end — if a re-push creates a second entry rather than updating, the
   retry logic becomes dangerous and needs an idempotency key or an upload-state stamp.

**Sizing:** ADIF over a keyed HTTP endpoint is small. OAuth or a bespoke format is a different
conversation. Answer question 1 first — it decides whether this is a connector or an exporter.
BODY

issue O7 2 enhancement \
  "Inbound log sync: pull QSOs in from other loggers" <<'BODY'
Asked: Nexus syncs logs *out* well — what about inbound syncing from other programs, or a universal
way other programs could sync in?

### What already exists — more than it looks
- **QRZ is already a genuine two-way connector** and is the working template: full pull, delta pull
  with an overlap window, and a response parser. Anything built for another service should follow
  that shape rather than invent one.
- **LoTW and eQSL pull too**, but only *confirmations* — they match against QSOs we already hold.
  That is a different job from "here are contacts you do not have".
- **⭐ A universal path half-exists already.** `crates/tempo-app/src/station.rs` watches the shared
  `log.adi` for freshness: if another writer appends to the same ADIF, Nexus folds those records in.
  It was built for a second Nexus instance, but **any program writing that file gets picked up
  today.** Worth documenting as the zero-effort answer before building anything — for a logger that
  can write ADIF to a chosen path it may already be the whole feature.
- No World Radio League connector, and no watch-a-folder-of-ADIFs path.

### ⚠️ The hard part is identity, and it recently shipped a bug
Two programs do not agree on a QSO's timestamp — one logs when you hit Enter, another when the
exchange started — so **the exact-second key cannot match across programs.** That is precisely why
`reconcile::merge_and_add` exists with its fuzzy `(call, band, mode-class, UTC-day ±1)` key, and
precisely why using that matcher for *our own* file caused a mis-pairing bug: one QSO deleted,
another duplicated, a confirmation attached to the wrong contact.

The fuzzy matcher is right for this job and wrong for the other one — **and this feature must use it
deliberately, not by accident.** Read `crates/tempo-core/src/reconcile.rs` and its history first.

### The decisions, in order
1. **What is authoritative on a conflict?** Same QSO, different RST or comment, in two systems.
   Nexus wins / remote wins / newest-modified wins / ask. Silently picking one is how an operator
   loses an edit they made deliberately.
2. **Universal mechanism** — a watched ADIF folder (drop-in, works with any logger that can export,
   no API, no credentials, probably the highest value per line of code); a local HTTP endpoint other
   programs POST ADIF to; or per-service connectors (best experience, most work, and each one is a
   permanent maintenance commitment against someone else's API).
3. **World Radio League specifically** — establish whether their API supports *reading* a log and
   what auth it needs before promising it. If it is upload-only this becomes a different feature.
4. **Never import a QSO into a hole it will fall out of.** Anything inbound must survive the same
   round trip as our own records: time-known, mode spelling, `OPERATOR`/`STATION_CALLSIGN`, and the
   passthrough that preserves unmodelled ADIF fields.
5. **Dry-run first.** Show what would be added, updated and skipped before writing anything. With a
   thousand operators and irreplaceable logs, a sync that silently adds 400 duplicates is worse than
   no sync at all.

Suggested first slice: size the watched-ADIF-folder option. It is small, service-agnostic, and may
satisfy the "universal way other programs could sync in" half of the ask on its own.
BODY

issue O8 2 enhancement \
  "Tempo chat: pack the frame header (+20% throughput, and it is where the sender id goes)" <<'BODY'
**Measured today** in `crates/tempo-core/src/text.rs`: `FREETEXT_MAX = 13`, `HEADER = 3`
(`id` + `seq` + `tot`), so `PAYLOAD = 10` characters per chunk and `MAX_CHUNKS = 9` — a **90-character
ceiling**. One frame per own-parity slot is 8 s per frame at TempoFast's 4 s T/R, plus a leading
identify frame carrying no text. Real cost: `hello` is 16 s, a full 90-character message is about
**80 s** — roughly 40–70 characters per minute.

**23% of every frame is overhead**, and it is three ASCII characters. One character from a wider
alphabet (or a base-N pack) recovers 2 characters per frame: **+20% throughput, and the ceiling goes
90 → 108.** The 9-chunk cap exists *only* because `seq` and `tot` are single decimal digits.

**⭐ It also carries the fix for the reassembly-merge bug.** Chunk sets are keyed on the message id
alone, so two stations using the same id in one window merge into a garbled message. Routing on the
observed sender does not work (the talker context drifts mid-message, which would split messages
that should assemble) — the fix needs a **sender id in the frame**, and this is the work that frees
the bits for it. Do the two together.

⚠️ **This is an on-air format change** — both ends must update together. Do it while it is a handful
of stations; it gets harder with every new user. Bump the protocol version.
BODY

issue O11 2 enhancement \
  "Auto-update: let the operator turn off the background download" <<'BODY'
**Default stays on** — download quietly, then offer to install, unchanged for everyone who does not
touch it. The opt-out is for **low-bandwidth, metered or satellite connections**, where pulling a
release unannounced is genuinely rude.

**Why it matters:** the Windows artifact is about 240 MB, and `ui/src/useSelfUpdate.ts` starts that
download the moment a check finds a version — deliberately, so that pressing install is instant.
That reasoning is right on home broadband and wrong on a metered hotspot or a field laptop tethered
to a phone, where a quarter of a gigabyte can arrive with no warning and no way to say no.

### The build (small, UI-side)
- New setting `update_auto_download: bool`, **default true**. Station-wide, not per-radio — it is a
  property of the connection, not the rig.
- When false, stop at the `'available'` phase instead of falling through to `'downloading'`. The
  banner then offers **"Download (240 MB)"** rather than "Install and restart", and pressing it runs
  the same download path already there. **Show the size in the button** — the whole point is
  informed consent, and the content length is already captured.
- Settings ▸ Updates, worded for the actual situation: "Nexus downloads new versions in the
  background so installing is instant. Turn this off on a metered or slow connection — you'll be
  told a version is ready and can start the download yourself."
- The install gate is untouched: it already refuses while transmitting, tuning, in a QSO or running
  CQ, and that is orthogonal to when bytes move.

⚠️ Keep the shipped failure behaviour: a background *check* that fails stays silent, while an
*install* failure surfaces. An opt-out must not turn a quiet check into a nag.
BODY

issue O13 2 enhancement \
  "Silence Doctor: tell the operator why they are hearing nothing" <<'BODY'
A pane plus a "why?" drawer plus a line in the status bar — no new nav section — that answers the
question an operator asks when nothing decodes: is it me, my noise floor, or the band?

Design rulings already taken:
- **Trigger automatically after 8 silent FT8 periods** (about 2 minutes), scaled per mode.
- **Judge noise relative to the operator's own history first.** Absolute P.372 calibration is a
  later unlock, not the entry price.
- **Ask the QTH noise category once**, default Residential, and **always name it in the verdict** —
  a verdict that hides its assumption is not a verdict.

It validates against a real logged incident, which is why it was ranked ahead of the other queued
programme work: the pass condition is checkable rather than aesthetic.
BODY

issue O14 2 enhancement \
  "Delivery ledger: record what each QSL service actually said, per QSO" <<'BODY'
An aggregate row per QSO with a per-award verdict on expand, plus a requeue policy — hybrid
(dependency invalidation, transient backoff, and a daily sweep).

**⚠️ The structural gap this closes.** `LogNeeds::add` takes a bare `confirmed: bool`, so the
confirmation **source is collapsed before the needs engine ever sees it** — a paper card and an eQSL
match are indistinguishable there. That is the same class as the missing mode axis, and the same
per-QSO work fixes both:

- ⭐ **Per-QSO confirmations give the mode axis for free.** There is no `confirmed_mode` set today
  (`confirmed_band` is mode-agnostic) yet the UI mode-gates Confirm — the model and the gate
  disagree, and no amount of UI work reconciles them.
- The same change fixes the per-award/per-source under-count in the elite DX awards matrix.

**Paper cards: tiers 1 and 2 are in** — verify `QslRcvd::Paper` is honoured end to end, plus a
one-tap "card received". Tier 3 (bureau CSV) is out of scope.

**A competitive finding worth keeping:** "everyone uses a boolean" is **false** — DXKeeper has six
status letters per service. The real gap is that nobody records *what the service said*,
distinguishes rejected from not-sent, or requeues a cohort.
BODY

issue O18 2 bug \
  "ADIF import drops SUBMODE and never reads APP_TEMPO_MODE" <<'BODY'
The ADIF importer discards `SUBMODE` and never reads the application-specific mode field that Nexus
itself writes on export.

**Harmless today** — a census of a real 11k-QSO log found zero affected rows — but it poisons future
imports: a WSJT-X or LoTW export carries `MODE=MFSK` with `SUBMODE=FT4`, and dropping the submode
throws away the only precise mode information in the record. Our own round trip loses information
too: what we write on export, we do not read back on import.

Fix: parse `SUBMODE`, and read `APP_TEMPO_MODE` when present, into whatever the record's mode
identity ends up being. Do it alongside the mode-normalization work — they are the same question
asked at different layers.
BODY

issue O20 2 enhancement \
  "Coverage map: shade confirmed grids apart from merely worked" <<'BODY'
`ui/src/coverage.ts` exposes only `workedGridSet()`, so the "My coverage (worked)" layer and the 3-D
globe cannot tell a worked square from an award-confirmed one.

The chaser's actual question is **"what do I still need *confirmed*"**, and the current single-shade
map cannot answer it. It ties directly into the VUCC/DXCC award story.

**Half of it is already written and recoverable rather than rewritable.** A parked branch has
`confirmedGridSet(log)` — the subset where any QSO carries `awardConfirmed` — with tests, about a
dozen lines. It was never wired to a map, so nothing user-visible was ever lost:

    git show wip/cockpit-panels-navorder:ui/src/coverage.ts

**The real work is the map side**: a second shade or fill on the coverage layer and on the globe
points, plus a legend. Small, self-contained, no transmit path.
BODY

issue O21 2 bug \
  "Three or more radios: every window spawn-fails a rigctld for every other enabled radio" <<'BODY'
In simultaneous-radios mode, every window tries to open a monitor connection to every *other*
enabled radio. The want-set has no notion of "another window already owns that radio", and
`open_monitor` refuses a daemon it did not launch — so each window spawn-fails a rigctld for each
foreign radio: 2 doomed cycles at two radios, 6 at three, plus permanently red "no CAT" pills.
Quadratic in the radio count.

**Partly mitigated already, and it matters which half.** A retry backoff shipped
(`retry_after_ms`, `crates/tempo-audio/src/service.rs`) and it kills the *single unreachable radio*
spawn loop — a radio that is enabled but powered off no longer respawns a daemon every ~850 ms.
Whether the cross-window claim gap remains has not been proven either way; it needs a real
three-radio setup to observe. **Reproduce before fixing.**

⚠️ **Do not weaken the monitor/handoff design to fix this** — the persistent per-radio CAT pool and
the instant handoff it enables are load-bearing for dual-radio operation.

### Riders for the same pass
- The port and audio conflict validators report only the **first** conflict, so a second collision
  stays hidden until the first is fixed.
- The `--profile r<id>` refusal logs to stderr only, which is invisible outside a console.
BODY

issue O23 2 bug \
  "DXpedition page parsing: removing a tag inserts no boundary, so spots-only rows can stop resolving" <<'BODY'
`strip_tags` in `crates/propagation/src/live/dxped.rs` removes markup character by character and
**inserts no boundary where a tag was**. The callsign cell parses today only because the live page
happens to put a newline inside one of its spans; without that, `TY5FR[spots]` and every spots-only
row stops resolving into a callsign.

So the parser is correct by luck, not by construction, and the failure mode when the page changes is
silent — rows quietly stop producing entries rather than erroring.

**The one-line fix (push a space where a `<` opens a tag) touches shared parsing for EVERY cell**,
including band and mode text, so it can change how those parse too. Do it as its own small task with
the fixtures re-validated against a full live page — not as a rider on unrelated work.
BODY

issue O24 2 enhancement \
  "Logbook write path: make \"recover before rewrite\" impossible to get wrong" <<'BODY'
Three consecutive adversarial reviews of this area each caught a defect the previous author shipped
past themselves:

| defect | how it got through |
|---|---|
| an ADIF import discarded the confirmations it carried | dedup treated a matching row as "nothing new" |
| the merge turned import into a full-log rewriter that deleted another instance's QSOs | the one call site legitimately exempt while append-only stopped being append-only |
| `reconcile_disk` mispaired by UTC day: one QSO deleted, one duplicated, a confirmation on the wrong contact | our own file re-identified with the matcher built for fuzzy external reports |

Each fix was correct, and each stays. **The pattern is the problem: every one was a local repair to
a shared routine whose contract lives in a doc comment.**

### The shape, as measured
- **16 `save_log` sites** in `crates/tempo-app/src/station.rs`. Fifteen must call
  `recover_external_appends()` first and one (`clear_logbook`) must not. **Nothing enforces which** —
  the rule is prose, and one of the three bugs above exists because that prose was silently falsified
  by an unrelated change.
- **`save_log` is a whole-file `rename()`.** There is no cross-process lock: two instances importing
  simultaneously can interleave read → merge → rename, and the later rename wins. The recovery
  narrows the window; it does not make the write atomic. Pre-existing, all 16 sites.
- **The staleness fingerprint is restamped by `save_log` but not by the append branch**, so the gate
  misses on every subsequent call.
- **One matcher serves two jobs with opposite requirements**: fuzzy `(call, band, mode-class, UTC-day
  ±1)` is right for a LoTW or eQSL report whose times legitimately differ, and wrong for
  re-identifying rows we wrote ourselves and know exactly.

### Questions worth answering before writing any code
1. Can the recover-before-rewrite rule be made **unrepresentable to get wrong** — a type, or a single
   funnel owning load → mutate → persist — rather than a comment fifteen sites must remember?
2. Should disk reconciliation and report reconciliation be **separate functions with separate keys**,
   given they have opposite correctness requirements?
3. Is whole-file `rename()` still the right persistence model at ~26,000 QSOs, or does an
   append-and-compact log serve better? Note the current model's one real virtue: it is crash-safe.
4. What is the honest answer on **concurrent instances**? Today there is no guard and the docs admit
   it. Either make it safe or make it refuse — the current middle is what keeps biting.

**Not a rewrite for its own sake.** This is about whether the next change to this path can be made
safely by someone who has not read all sixteen call sites.
BODY

issue O26 2 enhancement \
  "SSTV: a band view that becomes the picture while a signal decodes" <<'BODY'
A tall band view as the left column, the full-resolution live decode to its right, received images
underneath.

⭐ **The left column shows spectrum when idle and pixels while decoding.** That reconciles two asks
that pull opposite ways — "a waterfall, albeit small, to understand what's on the band right now"
needs spectrum; "as the band is coming in with the signal, actually show the image in the band as it
decodes" needs pixels. Pixels *replacing* the band was chosen over an overlay or a side-by-side
split. So: nothing decoding ⇒ an ordinary waterfall over the SSTV passband; VIS detected ⇒ the same
column paints decoded scan lines; signal ends ⇒ back to spectrum. One surface, two states.

**One waterfall row = one SSTV scan line.** The waterfall already has frequency on X and time on Y,
appending a row at the bottom and scrolling. Locking the row cadence to the mode's scan-line rate
(Scottie 1 is about 0.43 s per line) makes the picture align with the signal's arrival **by
construction**, with no timestamp plumbing — line N is simply row N.

⚠️ **The cost was stated and accepted: with pixels drawn where the spectrum was, you cannot see the
signal — including whether you are mistuned — for the ~110 s of a decode.** Two things blunt that,
both cheap and both required:
- The idle state above means the band is visible whenever nothing is decoding.
- **Surface the mistuning number.** The decoder already computes it — the observed leader offset
  from 1900 Hz, carried on the VIS-detected event and currently used only for diagnostics. Show it
  as a readout ("tuning +12 Hz") so the information the pixels hide is stated outright.

**The live progressive decode already works — do not rebuild it.** The decoder emits a per-scan-line
event, the progress DTO carries the preview buffer and the line counts, and `SstvView.tsx` paints it
to a canvas. What is missing is only the band view; the right-hand pane keeps that existing
full-resolution canvas.

**Reuse `ui/src/components/Waterfall.tsx`** (palettes, scaling and the retained-buffer scroll are all
solved) by adding a pixel-row source mode. Do not fork it into a second waterfall.
BODY

issue O27 2 enhancement \
  "CM108 HID PTT for Digirig Lite and modded dongles" <<'BODY'
Nexus does not implement CM108 HID keying. That is stated honestly in the interface detection today
(a Digirig Lite is named and told to use VOX or CAT rather than being offered a serial line it does
not have), so nothing is silently broken — this is a missing capability, not a defect.

**Only worth building if Digirig *Lite* or modded-dongle support is actually wanted.** Neither the
Digirig Mobile nor any RIGblaster needs it; both key correctly today.

⚠️ **The Hamlib shortcut does not work on Windows.** The bundled `libhamlib-4.dll` carries the
`cm108_*` symbols but imports no HID entry points — its CM108 path is POSIX `open`/`write` behind a
Linux hidraw guard. So `-P CM108` on Windows is a **silent no-op**, which is the worst possible
shape: a selectable PTT method that never transmits.

### If it is built, build it native
- `crates/tempo-audio/src/cm108.rs`: Linux via `std::fs` on `/dev/hidrawN` (zero new dependencies);
  Windows via the `windows-sys` dependency already present, with HID features. `hidapi` would be a
  **new C build dependency** and must clear the OSS-integration checklist first.
- HID enumeration for a picker (there is none today; cpal exposes no USB identity, so audio↔HID
  correlation stays name-only), a udev rule in the `.deb`, and settings/UI plumbing.

**Non-negotiable, decided before any backend code: the unsupported-platform arm returns `Err`, never
`Ok(())`.** A PTT method that reports success and never keys is exactly the failure this whole area
exists to avoid.

**TX/PTT path — gated.** Needs a maintainer yes before starting.
BODY

issue O29 2 documentation \
  "docs/rigs/yaesu.md lists three CW keyers and omits the serial keyline" <<'BODY'
`docs/rigs/yaesu.md` advertises three CW keying methods and does not mention the serial keyline at
all, which is a supported and shipped path — and the one an operator with a USB-to-serial keying
cable needs.

Pre-existing; flagged while fixing an FTX-1 keyline baud-rate bug. Small, self-contained docs fix;
fold into the next docs pass.
BODY

issue O30 2 documentation \
  "The Tempo product brief still shows pre-rename screenshots" <<'BODY'
`docs/Tempo-Product-Brief.md` and its HTML twin show pre-Nexus-rename UI, from captures several
months old, on a 1.0 product whose every other surface says Nexus.

Either refresh the captures or retire the document. Also stale on the same pass: the POTA/SOTA,
satellites and Connect panels on the site are still from a much older release — the satellite
console and the Connect globe in particular look nothing like they do now.

Refreshing needs real captures from a running build, so it is a docs pass with a screenshot step,
not a text edit.
BODY

issue O31 2 enhancement \
  "Pull POTA logs back into Nexus" <<'BODY'
Part of the unified-logbook batch. Manual QSO entry and confirmation pull-back from QRZ, LoTW and
eQSL have shipped; this and settings profiles are what remain.

POTA holds the authoritative record of an activation once it is processed — hunter confirmations,
park credit, and contacts the activator's own log may have missed or mis-keyed. Pulling that back
closes the loop the same way the QSL-service confirmation pulls do.

**Follow the existing shape, do not invent one.** The QRZ connector is the working two-way template
(full pull, delta pull with an overlap window, response parser), and anything inbound must survive
the same round trip as our own records — time-known, mode spelling, operator and station callsign,
and the passthrough that preserves unmodelled ADIF fields.

⚠️ The matching problem from the inbound-sync issue applies here in full: two systems do not agree
on a QSO's exact timestamp, so the exact-second key cannot be used across programs — and the fuzzy
matcher must be chosen deliberately, not by accident.
BODY

issue O32 2 enhancement \
  "Settings profiles: save, recall and transfer a station setup" <<'BODY'
The last of the unified-logbook batch. A named, saveable station setup that can be recalled and
moved to another machine.

Concretely useful in three cases that come up repeatedly: a portable setup versus a home setup on
one laptop; recovering after a reinstall; and helping another operator by handing them a working
configuration instead of a screenshot of a settings pane.

### Two constraints that shape it
- **Credentials must not travel in the file.** They live in the OS keychain by design; an exported
  profile that carries them turns a convenience into a credential leak. Export the fact that a
  connector is configured, not its secret.
- **Rig profiles are per-radio and the flat active mirror is derived.** Anything that writes settings
  in bulk has to sequence the writes correctly — the setup wizard exists partly because an
  out-of-order settings write clobbered a license class. Follow that ordering rather than writing a
  new bulk-apply path.

Depends on nothing else; the shared data directory it would live beside already ships.
BODY

# ─────────────────────────────────────────────────────────────────────────────

if [ "$CONFIRM" = 1 ]; then
  bold "Done: $CREATED created, $SKIPPED already existed."
else
  bold "$PLANNED issue(s) would be created. Nothing was created."
  printf '  Re-run with --yes to create them, or --wave 1 --yes for the first wave only.\n\n'
fi
