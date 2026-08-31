// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Every operator-visible
// string comes from the catalog (`i18n/en.ts`); a hardcoded one fails CI. What does NOT come from
// the catalog is LOG_EXAMPLES and PARK_PROGRAMS below, plus everything this strip prints about a
// contact — the callsign, RST, grid, band, mode and frequency are wire values, and a locale that
// "translated" one would have broken an example of a wire format. See the invariant-token rule in
// `i18n/index.ts`.

import { useEffect, useMemo, useRef, useState } from 'react'
import type { AppSnapshot, FieldDayStatus, LoggedQso } from '../types'
import { t } from '../i18n'
import { fdLogManual, getLog, logQso, lookupPark, lookupParkLive, qrzLookup, resolveEntity, searchParks, setCwPeerInfo, type Park } from '../api'
import { bandKey, callHistory, entitySlots, isNewEntity, modeKey } from '../features/callHistory'
import { ARRL_SECTIONS_BY_DIVISION } from '../features/arrlSections'
import { isValidLoggedGrid } from '../grid'
import { RecallPanel } from './RecallPanel'
import { pushToast, withErrorToast } from '../toast'

// Valid ARRL/RAC Field Day section codes, derived once from the canonical section universe —
// the same source the Settings section picker validates against. A manual FD contact must carry
// a REAL section: FieldDayLog::sections() scores DISTINCT section strings, so a blank logged as
// the literal '?' would inflate the section multiplier (a fake score).
const FD_SECTION_CODES = new Set(
  ARRL_SECTIONS_BY_DIVISION.flatMap((d) => d.sections).map((s) => s.code),
)

// Manual-override band picker for logging a contact made on a radio NOT connected to Nexus
// (V/UHF especially). One ordered table drives BOTH directions so band and freq can never
// disagree ("2m band / 14.2 MHz"): picking a band fills its representative in-band frequency,
// typing a frequency snaps the band to the plan it lands in. Spans 160m through 23 cm; the
// VHF/UHF edges intentionally match radioprog.bandOfMhz (which itself covers only those 5
// bands, hence the fuller table here).
const LOG_BANDS: { band: string; rep: number; lo: number; hi: number }[] = [
  { band: '160m', rep: 1.84, lo: 1.8, hi: 2.0 },
  { band: '80m', rep: 3.573, lo: 3.5, hi: 4.0 },
  { band: '60m', rep: 5.3715, lo: 5.3, hi: 5.45 },
  { band: '40m', rep: 7.074, lo: 7.0, hi: 7.3 },
  { band: '30m', rep: 10.136, lo: 10.1, hi: 10.15 },
  { band: '20m', rep: 14.074, lo: 14.0, hi: 14.35 },
  { band: '17m', rep: 18.1, lo: 18.068, hi: 18.168 },
  { band: '15m', rep: 21.074, lo: 21.0, hi: 21.45 },
  { band: '12m', rep: 24.915, lo: 24.89, hi: 24.99 },
  { band: '10m', rep: 28.074, lo: 28.0, hi: 29.7 },
  { band: '6m', rep: 50.313, lo: 50.0, hi: 54.0 },
  { band: '2m', rep: 146.52, lo: 144.0, hi: 148.0 },
  { band: '1.25m', rep: 223.5, lo: 219.0, hi: 225.0 },
  { band: '33cm', rep: 902.1, lo: 902.0, hi: 928.0 },
  { band: '70cm', rep: 446.0, lo: 420.0, hi: 450.0 },
  { band: '23cm', rep: 1296.1, lo: 1240.0, hi: 1300.0 },
]

/** The ham band whose plan contains `mhz`, or '' when it falls outside every band above. */
function bandForMhz(mhz: number): string {
  return LOG_BANDS.find((b) => mhz >= b.lo && mhz <= b.hi)?.band ?? ''
}

/** MODE values for the manual log override — every one a member of the CLOSED ADIF Mode
 * enumeration, so TQSL accepts the record. USB/LSB are deliberately ABSENT: they are ADIF
 * SUBMODEs, not Modes, and writing `<MODE>USB` gets the whole QSO rejected on LoTW upload —
 * the same closed-enumeration trap that rejected a bare `<MODE>TempoFast` (see
 * logbook.rs adif_submode). SSB is the generic phone Mode; FM/AM cover the rest of phone.
 *
 * PSK31 and QPSK31 are the keyboard-mode pair (#159 — the PSK cockpit had no log path at
 * all, and this list was the second of that report's three defects: even the manual
 * override could not name the mode the operator was running). Both are MODE values, not
 * SUBMODEs — `adif_submode` deliberately maps neither, and the logbook's own round-trip
 * writes `<MODE:5>PSK31` bare — so they stand beside RTTY here rather than needing the
 * TempoFast-style parent cascade. BPSK31 is not listed: it is the same waveform under a
 * logger's spelling, and both the importer and `modeKey` fold it to PSK31, so offering it
 * would let one mode be logged under two names.
 * The cockpits only ever pass SSB/FM/CW/PSK31/QPSK31 as the default, all present here. */
const LOG_MODES = ['SSB', 'FM', 'AM', 'CW', 'RTTY', 'PSK31', 'QPSK31', 'FT8', 'FT4'] as const

/**
 * The invariant example values this strip shows in empty fields, gathered so the guard can prove
 * they never became catalog entries. Every one is drawn from a technical namespace — a callsign,
 * a Field Day class, an ARRL section code, the signal-report format's name, a unit, a POTA park
 * and a SOTA summit reference — so it is the same characters in every language.
 */
const LOG_EXAMPLES = {
  call: 'W1AW',
  fdClass: '1D',
  fdSection: 'WI',
  rst: 'RST',
  freqUnit: 'MHz',
  potaRef: 'K-1234',
  sotaRef: 'W7A/MN-001',
} as const

/**
 * On-the-air programs for the park/summit picker. The ADIF SIG value and the label the operator
 * reads are the SAME token here, which is why this is one list and not a value/label pair.
 */
const PARK_PROGRAMS = ['POTA', 'SOTA'] as const

/** Now, split into a UTC date (YYYY-MM-DD) + time (HH:MM) for the override's inputs. */
function utcNowParts(): { date: string; time: string } {
  const iso = new Date().toISOString()
  return { date: iso.slice(0, 10), time: iso.slice(11, 16) }
}

/** Parse the UTC date + time inputs to Unix seconds, or null when either is malformed.
 * Amateur logs are UTC: parsing via Date.UTC keeps the browser's local zone out of it — a
 * naive `datetime-local` read as UTC would silently log hours off. */
function utcPartsToUnix(date: string, time: string): number | null {
  const d = /^(\d{4})-(\d{2})-(\d{2})$/.exec(date)
  const t = /^(\d{1,2}):(\d{2})$/.exec(time)
  if (!d || !t) return null
  const ms = Date.UTC(+d[1], +d[2] - 1, +d[3], +t[1], +t[2], 0)
  return Number.isFinite(ms) ? Math.floor(ms / 1000) : null
}

interface Props {
  snap: AppSnapshot
  /** ADIF mode logged ('CW' / 'SSB'). */
  mode: string
  /** Default signal report for this mode ('599' / '59'). */
  defaultRst: string
  /**
   * WHAT THE TWO STATIONS PASS EACH OTHER here — the one thing this strip's
   * three consumers (Phone, CW, Satellites) actually differ on, and therefore
   * the whole of the prop surface that keeps them sharing one component instead
   * of forking it. Required, not defaulted: a fourth consumer must state what
   * its contacts are made of rather than inherit somebody else's.
   *
   *  · 'terrestrial' — Phone and CW. Their operators hunt POTA/SOTA activators,
   *    so the park/summit reference of the station worked is part of the
   *    exchange and the picker + directory search render.
   *  · 'satellite' — the Satellites section. There is no park on a bird, so the
   *    park row is simply NOT ASKED FOR: nothing to hide, nothing to suppress,
   *    and no vertical space taken in a column shared with the sky dome. What
   *    IS passed through a bird is the grid, so the Grid field is asked for
   *    here and only here — beside the reports, in the exchange row.
   *
   * Grid in the terrestrial exchanges is an OPEN QUESTION, not a settled no: a
   * wrong callbook square is uncorrectable in Phone and CW today, and those are
   * the cockpits that meet the rovers and /P activations whose square is wrong
   * most often (2 m/6 m and the VHF contests are grid-for-grid too). It is left
   * out here because it costs the Phone and CW strips a wrapped line each and
   * the operator's instruction for this change was "keep focused on sat" — the
   * height, not the merit, is what is unresolved. See CHANGELOG › Unreleased.
   */
  exchange: 'terrestrial' | 'satellite'
  /** Spot the typed call to the DX cluster: renders a 📢 Spot button beside Log that
   * hands the CURRENT call field up (the host opens its SpotDialog seeded with it +
   * the dial). Phone/CW ask (operator 2026-07-21); omitted = no button (FT8 has its
   * own roster-side spot affordance). */
  onSpot?: (call: string) => void
  /** Open the Logbook filtered to a callsign — handed straight to the recall card below, whose
   *  previous-contact rows become clickable when it is present (#192). Omitted = inert rows,
   *  which is what a build with the Logbook section switched off gets. */
  onOpenLogbook?: (call: string) => void
  /** Click-to-work handoff from the Needed board: the callsign to prefill + focus RST.
   * `ts` changes per click so re-working the same call refires the prefill. */
  pendingWork?: { call: string; ts: number } | null
  /** Called once the prefill has been applied, so the parent can clear it. */
  onConsumeWork?: () => void
  /** CW copilot LIVE fill: the best-guess worked call (confirmed chip if any, else the top
   * decoded candidate) + the decoder's running read of their RST/name, updated every poll.
   * LogEntry pre-fills each field as the QSO unfolds. `confirmed` (a chip click) always wins;
   * an unconfirmed best-guess never clobbers a call the operator has typed over it. */
  cwLive?: {
    call: string | null
    rst: string | null
    name: string | null
    confirmed: boolean
  } | null
  /**
   * When provided, the component enters FD mode: contacts go to fdLogManual()
   * instead of the general logbook.  The `mode` prop determines the FD mode
   * code ('CW' in CwCockpit, 'PH' in PhoneCockpit).
   */
  fieldDay?: FieldDayStatus | null
  /**
   * The FD mode code to pass to fdLogManual: the SCORING CLASS this position logs under.
   *
   * ⚠️ 'DIG' IS ONE OF THE THREE, and leaving it out was a silent scoring error. Field Day
   * scores CW, phone and digital as three classes, and the engine's `log_mode_at`
   * (tempo-core/src/fieldday.rs) already handles 'DIG' specially — it stamps the actual on-air
   * submode behind the class so exports emit the real mode. A digital position picking a
   * station up by hand (the FD cockpit's decode monitor prefills the Call field) was the first
   * consumer with a DIG contact to log; before this it had to send 'PH', which credits the
   * wrong class and dupes against the wrong cell.
   */
  fdMode?: 'CW' | 'PH' | 'DIG'
  /**
   * FD ONLY, AND 'DIG' ONLY — the mode that was actually ON THE AIR behind the digital
   * scoring class (an ADIF name: 'RTTY', 'PSK31', 'QPSK31').
   *
   * ⚠️ REQUIRED FOR A KEYBOARD-MODE POSITION. `fdMode` is the scoring class, and 'DIG' covers
   * every digital mode there is; the engine fills the missing detail from
   * `FieldDayLog::current_submode`, which tracks the FT tier ALONE (`adif_mode_for_tier` has
   * no RTTY or PSK variant). So an RTTY or PSK contact logged as bare 'DIG' is stamped "FT8" —
   * the wrong mode in the ADIF export, Cabrillo "DG" where ARRL wants "RY", and a mode Winter
   * Field Day bans outright on a QSO that was perfectly legal RTTY.
   *
   * Ignored unless `fdMode` is 'DIG': CW and PH ARE their on-air mode and carry no submode.
   */
  fdSubmode?: string
  /**
   * FD ONLY — the in-progress exchange, mirrored out on every change so a host can paint
   * boards from the DRAFT: the FD cockpit's band × mode grid lights the cells where the call
   * being typed is already worked, and its sections checklist glows the section as it settles,
   * before anything is logged.
   *
   * The values arrive normalized exactly as the dupe verdicts below normalize them — trimmed
   * and uppercased — so a board and the dupe check can never disagree about the key. `cls` is
   * the FD exchange class ('3A'), not the CW/PH mode class (that one is `fdMode`).
   *
   * Omitted by every consumer that logs the ordinary way (Phone, CW, Satellites): nothing is
   * called and this strip behaves exactly as it did.
   */
  onFdDraftChange?: (draft: { call: string; cls: string; section: string }) => void
  /**
   * FD ONLY — land the caret in the callsign field when this strip MOUNTS.
   *
   * ⚠️ OPT-IN, AND THAT IS SAFETY RATHER THAN TASTE. The Field Day cockpit wants it: the strip
   * IS that screen, so the first contact of a run is typed rather than clicked. The Phone and
   * CW cockpits must NOT have it, because a focused text field disarms their window Space
   * handler — Phone's Space keyup is a member of its stop-line census — and a strip that
   * grabbed focus on mount would kill push-to-talk from the moment the cockpit appeared, for
   * the whole of Field Day, in a shipped cockpit. Default false: every host that does not ask
   * behaves exactly as it did.
   *
   * `reset()`'s refocus after a logged contact is a different thing and is NOT gated: that one
   * follows an action the operator took in this strip.
   */
  autoFocusCall?: boolean
  /**
   * Does this strip render its OWN "Log this QSO" heading? Default true — today's
   * behaviour, unchanged, for every host that does not say otherwise.
   *
   * A host that already names the surface passes false: the Phone and CW cockpits frame
   * this component in a CockpitPaneFrame whose head reads "LOG" two lines above, and is
   * the frame's accessible name, so the heading said the same thing twice and cost ~30px
   * off the top of a strip whose Log button was already below the fold at the default
   * window. SatellitesView hosts it in a plain `.sats-log` div with NO title of its own,
   * which is why this is a prop and not a deletion — deleting the heading outright would
   * leave that surface untitled. LogEntry.density.test.tsx pins both halves.
   * (The Field Day variant below has never had a heading, on the same reasoning.)
   */
  titled?: boolean
}

/**
 * Shared rich log strip for the CW + Phone cockpits.
 *
 * When `fieldDay` is active (non-null) the strip shows Class + Section inputs
 * and routes the log through fdLogManual() — a dupe rejection shows the error
 * toast. A "FD" chip on the strip signals contacts go to the FD log, not the
 * general logbook.
 *
 * When fieldDay is null/undefined, behaviour is the standard logbook path.
 */
export function LogEntry({
  snap,
  mode,
  defaultRst,
  exchange,
  onSpot,
  onOpenLogbook,
  pendingWork,
  onConsumeWork,
  cwLive,
  fieldDay,
  fdMode,
  fdSubmode,
  onFdDraftChange,
  autoFocusCall = false,
  titled = true,
}: Props) {
  const fdActive = fieldDay != null
  // Does this cockpit's exchange carry a park/summit reference? See `exchange`.
  const asksForPark = exchange === 'terrestrial'
  // …and a grid square? A bird's exchange is grid-for-grid, so the field is asked
  // for there. NOT in Phone/CW — see `exchange` for why that is an open question
  // and not a settled no. The FD layout renders neither field.
  const asksForGrid = exchange === 'satellite' && !fdActive

  const [logCall, setLogCall] = useState('')
  const [logRstSent, setLogRstSent] = useState(defaultRst)
  const [logRstRcvd, setLogRstRcvd] = useState(defaultRst)
  const [logName, setLogName] = useState('')
  const [logQth, setLogQth] = useState('')
  const [logComment, setLogComment] = useState('')
  const [logNotes, setLogNotes] = useState('')
  // Autofill stash (written to the record, not separately edited): grid/state/country.
  const [logGrid, setLogGrid] = useState('')
  const [logState, setLogState] = useState('')
  const [logCountry, setLogCountry] = useState('')
  // Callbook profile photo (display-only, not written to the log). Cleared when the call changes.
  const [logImage, setLogImage] = useState<string | null>(null)
  // The callbook's exact coordinates for this call (display-only, like logImage, and
  // cleared with it when the call changes). Absent whenever the callbook vouched for no
  // real position — the card then falls back to the locator.
  const [logCoords, setLogCoords] = useState<{ lat: number; lon: number } | null>(null)
  // POTA/SOTA park of the station worked (ota.their_*). Prefilled from a hunted spot; editable.
  const [logParkProgram, setLogParkProgram] = useState('POTA')
  const [logParkRef, setLogParkRef] = useState('')
  // Local park-directory suggestions (POTA only) as the operator types the reference.
  const [parkHits, setParkHits] = useState<Park[]>([])
  const [parkPicked, setParkPicked] = useState(false)
  // Auto-loaded details for a complete park reference (name/location/grid), local-first then live.
  const [parkDetail, setParkDetail] = useState<Park | null>(null)
  const [parkDetailLive, setParkDetailLive] = useState(false)
  const [qrzBusy, setQrzBusy] = useState(false)
  const [allLog, setAllLog] = useState<LoggedQso[]>([])
  // Opt-in manual override (standard-log path only) for a contact made on a radio NOT
  // connected to Nexus — e.g. a V/UHF rig. CLOSED = log the live rig + now, byte-identical
  // to before this existed. OPEN = the operator sets band / freq / mode / UTC time by hand,
  // each field pre-filled from the current live/now value at the moment it's opened.
  const [overrideOpen, setOverrideOpen] = useState(false)
  const [ovDate, setOvDate] = useState('')
  const [ovTime, setOvTime] = useState('')
  const [ovBand, setOvBand] = useState('')
  const [ovFreq, setOvFreq] = useState('')
  const [ovMode, setOvMode] = useState('')
  const rstRef = useRef<HTMLInputElement>(null)
  // The callsign field (FD + standard layouts share this ref — only one is mounted
  // at a time), so a completed log can snap focus back for the next contact.
  const callInputRef = useRef<HTMLInputElement>(null)

  // FD-specific: class + section, defaulting from the last entry / fieldDay status.
  const [fdClass, setFdClass] = useState(() => fieldDay?.myClass ?? '')
  const [fdSection, setFdSection] = useState('')

  // Pre-fill class/section from the last logged FD contact — but ONLY when the
  // log actually GREW. `fieldDay` is a fresh object every 300 ms snapshot poll;
  // keying the effect on it overwrote whatever the operator was TYPING with the
  // last contact's exchange (the digital sequencer logging in the background
  // made the fields jump mid-keystroke).
  const fdLogLen = fieldDay?.log?.length ?? 0
  const fdSeenLen = useRef(fdLogLen)
  useEffect(() => {
    if (fdActive && fieldDay && fdLogLen > fdSeenLen.current) {
      const lastEntry = fieldDay.log?.[fdLogLen - 1]
      if (lastEntry) {
        setFdClass(lastEntry.class)
        setFdSection(lastEntry.section)
      }
    }
    fdSeenLen.current = fdLogLen
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [fdActive, fdLogLen])

  // FD: land focus in the callsign field the moment this strip MOUNTS, so the first contact of
  // a run is typed rather than clicked — the same field `reset()` snaps back to after every
  // logged contact, just from the first one too. preventScroll for reset()'s reason: this focus
  // readies the field, it must never scroll the cockpit to the log.
  // Mount only (empty deps, the flags read once): if Field Day starts while this strip is
  // already on screen, focus stays wherever the operator put it.
  // ⚠️ ONLY WHEN THE HOST ASKED (`autoFocusCall`) — see that prop. Focusing on `fdActive`
  // alone reached the Phone and CW cockpits too, and a focused text field disarms Phone's
  // window Space handler, which is on its stop-line census.
  useEffect(() => {
    if (fdActive && autoFocusCall) callInputRef.current?.focus({ preventScroll: true })
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  // Live mirror of the typed call so a slow lookup can tell if the operator has since
  // changed the call (drop the stale result rather than fill the wrong call's data).
  const logCallRef = useRef(logCall)
  logCallRef.current = logCall

  // Live busy flag the debounced auto-lookup timer reads at FIRE time — the `qrzBusy` state is
  // captured stale in the timer's closure, so a blur/button lookup already in flight wouldn't be
  // seen. Kept in sync with `qrzBusy` inside `lookup`.
  const qrzBusyRef = useRef(false)
  // True only when the LAST call change came from the operator TYPING (set in the call input's
  // onChange), false for machine fills (CW decoder, click-to-work). The debounced auto-lookup
  // fires on human edits only, so the shared CW cockpit never spams QRZ on decoder candidate flap.
  const humanCallEditRef = useRef(false)
  /**
   * A call that arrived from the app rather than the keyboard, and is SETTLED enough to look up.
   *
   * ⚠️ WHY THIS EXISTS (operator, 2026-08-15: CW contacts logging with no grid). Callbook
   * enrichment — grid, state, country — only ever ran for calls the operator TYPED, because the
   * debounced auto-lookup is gated on `humanCallEditRef` and both machine fills clear it. In the
   * CW cockpit the call is never typed: the decoder fills it, and focus goes straight to RST, so
   * the on-blur lookup never fires either. Nothing else populates `logGrid` — the CW/Phone strips
   * have no Grid box at all (see `asksForGrid`), it is a write-only autofill stash — so every CW
   * QSO logged with `grid: null`. Same defect on a clicked spot, which is also a machine fill.
   *
   * It is a SEPARATE ref rather than just setting `humanCallEditRef = true` because the two facts
   * are different and one of them is about typing: `humanCallEditRef` still means "the operator is
   * editing this field", which is what stops a half-typed call being looked up. This one means
   * "this call is final" — a clicked spot, or a CONFIRMED decoder call. An unconfirmed best guess
   * is deliberately excluded: the decoder walks K9 → K9A → K9ABC, and looking each one up would
   * spend the operator's QRZ quota on calls that were never worked.
   */
  const settledCallRef = useRef(false)

  // The call whose identity (name/QTH/grid/state/country) the enrichment fields currently hold.
  // Clear them when the call changes to a DIFFERENT one, so a previous callsign's data never
  // bleeds onto another call's recall card — and so onCallBlur re-looks-up the new call.
  const enrichedForRef = useRef('')
  // The call for which an Enter-triggered QRZ lookup was already ATTEMPTED (success OR miss/no
  // callbook), so a second Enter logs instead of re-looping the lookup on an unresolvable call.
  const triedLookupRef = useRef('')
  useEffect(() => {
    const c = logCall.trim().toUpperCase()
    if (enrichedForRef.current && c !== enrichedForRef.current) {
      enrichedForRef.current = ''
      triedLookupRef.current = ''
      setLogName('')
      setLogQth('')
      setLogGrid('')
      setLogState('')
      setLogCountry('')
      setLogImage(null)
      setLogCoords(null)
      setLogParkRef('') // the park was for the previous call
      // The wiped name may have been the CW decoder's copy — un-latch so it can refill for the
      // new call (declared below; the effect callback runs after render, so it's initialized).
      cwNameFilled.current = false
    }
  }, [logCall])

  // Prefill the park field from a hunted spot (snap.hunt = the activator's program+ref). Keyed on
  // the reference string so it fires when a NEW park is hunted, not on every snapshot poll.
  //
  // Deliberately NOT gated on `asksForPark`. It latches in every exchange, and what stops a
  // latch reaching a satellite record is the ONE guard on `ota` at the commit — one seam,
  // one mechanism, mutable and therefore testable. A second guard here would make that one
  // untestable (remove it and the record still comes out clean, because the latch was never
  // taken) which is how a load-bearing fix quietly dies. The two effects below carry their
  // own guards for their own reason: they reach the NETWORK.
  useEffect(() => {
    const h = snap.hunt
    if (!h?.reference) return
    const baseCall = (c: string) => c.trim().toUpperCase().split('/').pop() ?? ''
    const callMatches = logCall.trim() !== '' && baseCall(h.call) === baseCall(logCall)
    const ref = logParkRef.trim().toUpperCase()
    const huntRef = h.reference.trim().toUpperCase()
    if (callMatches || logCall.trim() === '') {
      // Prefill (or keep) the hunted park — but only when the field is empty or still holds the
      // prefill, so a manual override survives. `parkPicked` suppresses the search dropdown.
      if (ref === '' || ref === huntRef) {
        setParkPicked(true)
        setLogParkProgram(h.program)
        setLogParkRef(h.reference)
      }
    } else if (ref === huntRef) {
      // Call changed to a NON-matching one: the engine's auto-tag won't apply the park to this
      // call, so drop the prefill rather than SHOW a park that won't be logged.
      setLogParkRef('')
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [snap.hunt?.reference, snap.hunt?.program, logCall])

  // Search the local park directory as the operator types a POTA reference (debounced).
  // `asksForPark` for the same reason as the lookup below: an exchange with no park row
  // runs no park machinery, and this one reaches the disk. It was already unreachable in
  // the satellite exchange — but only by the ACCIDENT that the prefill above sets
  // `parkPicked` in the same commit, so this effect always finds the flag and consumes it
  // instead of searching. That is not a guard, it is a coincidence between two effects, and
  // no single change to either can be tested for. Stated instead of relied on. (Which is
  // also why no test here reddens on removing this line alone: `parkPicked` still catches
  // it. The guard's value is that the property no longer depends on that.)
  useEffect(() => {
    if (!asksForPark) {
      setParkHits([])
      return
    }
    if (parkPicked) {
      setParkPicked(false)
      return
    }
    const q = logParkRef.trim()
    if (logParkProgram !== 'POTA' || q.length < 2) {
      setParkHits([])
      return
    }
    const id = setTimeout(() => {
      void searchParks(q, 8)
        .then(setParkHits)
        .catch(() => setParkHits([]))
    }, 180)
    return () => clearTimeout(id)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [logParkRef, logParkProgram, asksForPark])

  // Auto-load a park's details the moment a COMPLETE valid POTA reference is entered (like HRD):
  // instant offline lookup first, then the live POTA directory if it's not in the local list. Purely
  // informational — never blocks typing or logging, and swallows all errors (offline = no chip).
  // `asksForPark` first: a pending hunt still PREFILLS logParkRef in every exchange
  // (the engine tags by callsign whichever strip logs), so without this the satellite
  // exchange would fire a local lookup and then a live POTA-directory fetch for a chip
  // it never renders.
  useEffect(() => {
    const ref = logParkRef.trim().toUpperCase()
    if (!asksForPark || logParkProgram !== 'POTA' || !/^[A-Z0-9]{1,4}-\d{4,5}$/.test(ref)) {
      setParkDetail(null)
      return
    }
    let cancelled = false
    const id = setTimeout(() => {
      void lookupPark(ref)
        .then((local) => {
          if (cancelled) return
          if (local) {
            setParkDetail(local)
            setParkDetailLive(false)
            return
          }
          // Not in the local list (empty/stale) → try the live POTA directory (also brings coords).
          return lookupParkLive(ref).then((live) => {
            if (!cancelled) {
              setParkDetail(live)
              setParkDetailLive(true)
            }
          })
        })
        .catch(() => {
          if (!cancelled) setParkDetail(null)
        })
    }, 250)
    return () => {
      cancelled = true
      clearTimeout(id)
    }
  }, [logParkRef, logParkProgram, asksForPark])

  const refreshLog = () => void getLog().then(setAllLog).catch(() => {})
  useEffect(() => {
    // In FD mode we don't use the general logbook for dupe checking, so skip the fetch.
    if (fdActive) return
    refreshLog()
  }, [fdActive])

  // Click-to-work prefill: land the call + drop focus on RST so the operator types the report
  // and hits Enter. Keyed on `ts` to refire on re-click of the same call.
  useEffect(() => {
    if (!pendingWork) return
    setLogCall(pendingWork.call.toUpperCase())
    humanCallEditRef.current = false // a clicked spot is not a human keystroke…
    settledCallRef.current = true // …but it IS a final call, so it still gets enriched
    // preventScroll: focusing the RST readies it for the report, but must NOT scroll the log
    // into view — the operator works from the decode feed/roster scrolled up, and a click
    // snapping the window down to the log every time is the reported bug.
    rstRef.current?.focus({ preventScroll: true })
    rstRef.current?.select()
    onConsumeWork?.()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pendingWork?.ts])

  // CW copilot LIVE fill: as the QSO is decoded, fill blank log fields for the CONFIRMED
  // worked station — the call the moment it's set, then RST + name once each as they arrive.
  // Blanks-only + once-per-field-per-station, so it fills through the QSO without ever
  // clobbering what the operator typed.
  const cwFilledFor = useRef<string | null>(null)
  const cwRstFilled = useRef(false)
  const cwNameFilled = useRef(false)
  useEffect(() => {
    if (!cwLive?.call) return
    const up = cwLive.call.toUpperCase()
    const { rst, name } = cwLive
    if (cwFilledFor.current !== up) {
      // A confirmed chip click always lands. An unconfirmed best-guess fills only if the
      // operator hasn't typed their own call over our previous auto-fill (don't clobber).
      const overridden =
        !cwLive.confirmed &&
        logCallRef.current.trim() !== '' &&
        logCallRef.current.toUpperCase() !== (cwFilledFor.current ?? '')
      if (!overridden) {
        setLogCall(up)
        humanCallEditRef.current = false // decoder fill, not a keystroke…
        // …and enrich it once the decoder is SURE. A best guess still changes character by
        // character, and each change would be a callbook request for a call nobody worked.
        settledCallRef.current = cwLive.confirmed === true
        cwFilledFor.current = up
        cwRstFilled.current = false
        cwNameFilled.current = false
      }
    }
    if (cwFilledFor.current && !cwRstFilled.current && rst) {
      // The decoder's read of `rst` is the report we RECEIVED from them.
      setLogRstRcvd((v) => (v.trim() === '' || v === defaultRst ? rst : v))
      cwRstFilled.current = true
    }
    if (cwFilledFor.current && !cwNameFilled.current && name) {
      setLogName((v) => (v.trim() ? v : name))
      cwNameFilled.current = true
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [cwLive?.call, cwLive?.rst, cwLive?.name])

  const hist = useMemo(
    // `mode` is this cockpit's LOG mode; the scope flag mirrors the engine's worked-band sets
    // so the Dupe badge and the B4 chips can never disagree about what a dupe is.
    () => callHistory(allLog, logCall, snap.radio.band, mode, snap.b4MatchMode ?? false),
    [allLog, logCall, snap.radio.band, mode, snap.b4MatchMode],
  )

  // The award identity for the badges: cty.dat resolved from the CALL (a local
  // in-process table — no network), never the QRZ country string. Keying on the
  // QRZ spelling made NEW ONE fire on every German/Russian contact forever,
  // because "Germany" can never match the "Fed. Rep. of Germany" already in the
  // log. `logCountry` remains the display text and the legacy fallback.
  const [logEntity, setLogEntity] = useState<string | null>(null)
  useEffect(() => {
    const call = logCall.trim()
    if (!call) {
      setLogEntity(null)
      return
    }
    let stale = false
    void resolveEntity(call)
      .then((e) => {
        if (!stale) setLogEntity(e)
      })
      .catch(() => {})
    return () => {
      stale = true
    }
  }, [logCall])
  const entityForBadge = logEntity ?? logCountry
  const newEntity = useMemo(() => isNewEntity(allLog, entityForBadge), [allLog, entityForBadge])

  // DXCC-Challenge axis: the entity is already worked, but is THIS band (or mode) a new
  // slot for it? Only meaningful once the entity is in the log (an ATNO is owned by
  // `newEntity` above; a blank/unresolved entity yields workedEver=false and falls through
  // to the plain "not in your log" line). Bands/modes are entity-wide; band wins over mode.
  const slots = useMemo(() => entitySlots(allLog, entityForBadge), [allLog, entityForBadge])
  // Both sides go through the same key functions — the live band/mode and the stored ones. A raw
  // string compare here was half of the false-badge family: USB against a log of LSB read as a
  // mode never worked, and a stored band token that named no band matched nothing forever.
  // `bandUnknown` suppresses the band badge outright: a contact whose band cannot be recovered
  // means we do not know whether this band is new, and a guess in that state is wrong for as long
  // as the row exists.
  const liveBand = bandKey({ band: snap.radio.band, freqMhz: snap.radio.dialMhz })
  const newBandSlot =
    slots.workedEver &&
    !slots.bandUnknown &&
    liveBand !== null &&
    !slots.bandsWorked.includes(liveBand)
  const newModeSlot =
    slots.workedEver && !newBandSlot && !slots.modesWorked.includes(modeKey(mode))

  // QRZ callbook autofill — fills ONLY blank fields, never clobbers operator input. The
  // explicit button toasts; the on-blur auto-lookup is silent on failure so an operator
  // without QRZ configured isn't nagged on every Tab.
  const lookup = async (silent: boolean) => {
    if (qrzBusyRef.current) return
    const call = logCall.trim()
    if (!call) return
    qrzBusyRef.current = true
    setQrzBusy(true)
    const r = silent
      ? await qrzLookup(call).catch(() => null)
      : await withErrorToast(() => qrzLookup(call), t('callbook.lookupFailed'))
    qrzBusyRef.current = false
    setQrzBusy(false)
    if (!r) return
    if (logCallRef.current.trim().toUpperCase() !== call.toUpperCase()) return
    // Prefer the operator's QRZ nickname over their full name when they set one —
    // hams want to be greeted by it (operator input is still never clobbered).
    const preferredName = r.nickname || r.name
    if (preferredName) setLogName((v) => (v.trim() ? v : preferredName))
    if (r.qth) setLogQth((v) => (v.trim() ? v : r.qth ?? ''))
    // The callbook's square only lands if it IS a square. QRZ answers this field with
    // whatever the operator put in their profile — an 8-character extended locator
    // (probed live: FN31PR99, which `isValidLoggedGrid` takes), but also a rover's
    // "EN52/EN53" or free text, which it does not. Filling the field with a value the
    // strip refuses is how a callbook came to disable the Log button over a square the
    // operator never typed. Blanks-only is unchanged: a typed square still wins.
    // …AND ONLY WHERE THE BOX EXISTS. The reason to filter is that a value the strip
    // will then REFUSE must not arrive on its own; that reason lives entirely in
    // `asksForGrid`. Applying it in the terrestrial exchanges too changed what their
    // records carry — a callbook grid that is not a 4/6/8 locator stopped reaching
    // the record — for cockpits with no Grid box, no `gridBlocked` and nothing to
    // strand. Those strips were explicitly out of scope for this change, so they keep
    // taking the callbook's answer exactly as they did.
    if (r.grid && (!asksForGrid || isValidLoggedGrid(r.grid)))
      setLogGrid((v) => (v.trim() ? v : r.grid ?? ''))
    if (r.state) setLogState((v) => (v.trim() ? v : r.state ?? ''))
    if (r.country) setLogCountry((v) => (v.trim() ? v : r.country ?? ''))
    setLogImage(r.image ?? null) // display-only; no operator value to preserve
    // Same: display-only, and only when the callbook vouched for a REAL position (the
    // backend refuses QRZ's grid-derived and DXCC-centroid fallbacks).
    setLogCoords(r.lat != null && r.lon != null ? { lat: r.lat, lon: r.lon } : null)
    enrichedForRef.current = call.toUpperCase()
    // Feed the worked station's name/state to the engine for the {HISNAME}/{HISSTATE} CW-macro
    // tokens (keyed to the call so a stale lookup can't key the wrong name).
    void setCwPeerInfo(call, preferredName ?? '', r.state ?? '')
    if (!silent) {
      const detail = [r.name, r.grid && t('callbook.detail.grid', { grid: r.grid }), r.state]
        .filter(Boolean)
        .join(' · ')
      const vals = { call: r.call, detail: detail || r.country || t('callbook.detail.found') }
      pushToast(r.grid ? t('callbook.result', vals) : t('callbook.resultNoGrid', vals), 'info')
    }
  }

  const onCallBlur = () => {
    if (!fdActive && !qrzBusy && logCall.trim().length >= 3 && !logName.trim()) void lookup(true)
  }

  // Auto-look-up name/QTH shortly after the operator stops typing a call (no Tab needed), so they
  // can greet by name mid-ragchew. Debounced; skips FD, in-flight, and already-enriched calls.
  useEffect(() => {
    const c = logCall.trim()
    const cu = c.toUpperCase()
    // A typed call, or a machine fill that has settled (see `settledCallRef`) — and not already
    // enriched. The machine-fill half is what puts a grid on a CW contact.
    const worthLookingUp = humanCallEditRef.current || settledCallRef.current
    if (fdActive || !worthLookingUp || c.length < 3 || enrichedForRef.current === cu) return
    const t = setTimeout(() => {
      // Re-check at FIRE time via refs: an onCallBlur/QRZ-button lookup may have already enriched
      // this call or still be in flight — either way, don't fire a duplicate QRZ request.
      if (enrichedForRef.current !== cu && !qrzBusyRef.current) void lookup(true)
    }, 700)
    return () => clearTimeout(t)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [logCall, fdActive])

  // Open the override, seeding every field from the LIVE rig + NOW so an unedited-but-open
  // override still logs today's values. Re-seeds on each open (the cockpit stays mounted
  // across nav, so a once-at-mount default would go stale).
  const openOverride = () => {
    const { date, time } = utcNowParts()
    setOvDate(date)
    setOvTime(time)
    setOvBand(snap.radio.band)
    setOvFreq(String(snap.radio.dialMhz))
    setOvMode(mode)
    setOverrideOpen(true)
  }

  // Keep band ↔ freq consistent. Picking a band fills its representative in-band frequency;
  // typing a frequency snaps the band to the plan it lands in (leaving the band untouched when
  // the frequency is off-plan — the operator still sees a non-blocking warning).
  const onPickOvBand = (b: string) => {
    setOvBand(b)
    const rep = LOG_BANDS.find((x) => x.band === b)?.rep
    if (rep != null) setOvFreq(String(rep))
  }
  const onEditOvFreq = (v: string) => {
    setOvFreq(v)
    const b = bandForMhz(Number(v))
    if (b) setOvBand(b)
  }

  const reset = () => {
    setLogCall('')
    setLogRstSent(defaultRst)
    setLogRstRcvd(defaultRst)
    setLogName('')
    setLogQth('')
    setLogComment('')
    setLogNotes('')
    setLogGrid('')
    setLogState('')
    setLogCountry('')
    setLogImage(null)
    setLogCoords(null)
    setLogParkRef('')
    void setCwPeerInfo('', '', '') // clear the {HISNAME}/{HISSTATE} tokens for the next contact
    // When the other-radio override is open, refresh its UTC time to now for the next contact
    // (a run of live V/UHF contacts each get the current time, never a silently-reused stale
    // one) while KEEPING band/freq/mode — like fdClass/fdSection, so they aren't re-entered.
    if (overrideOpen) {
      const { date, time } = utcNowParts()
      setOvDate(date)
      setOvTime(time)
    }
    // Keep fdClass/fdSection across resets for speed in FD runs.
    // Snap focus back to the callsign field so the operator can type the next
    // contact immediately (rapid logging / a Field Day run), like WSJT-X/N1MM.
    // rAF so it lands after the cleared value re-renders. Only the mounted layout's
    // input (FD or standard) holds the ref, so this always hits the visible field.
    // preventScroll for the same reason as the click-to-work RST focus above:
    // this focus readies the field, it must not snap the cockpit to the log.
    requestAnimationFrame(() => callInputRef.current?.focus({ preventScroll: true }))
  }

  // FD exchange gate: class + section are MANDATORY exchange elements. Never substitute '?' for a
  // blank (it would fake a section multiplier) — the manual log is blocked until class is present
  // AND section is a real ARRL/RAC code (validated against the same universe as Settings).
  const fdExchangeOk =
    fdClass.trim() !== '' && FD_SECTION_CODES.has(fdSection.trim().toUpperCase())

  // GRID GATE. A blank grid is normal and logs fine — most HF contacts have none.
  // A grid that is NOT a Maidenhead locator is refused at the commit, the same
  // shape as the two gates above and below it (the FD section code, the override
  // frequency), and for the same reason: a QSO record is permanent, it cannot be
  // repaired after upload, and a bad square is not a missing credit but a WRONG
  // one (it counts toward a VUCC grid the operator never worked). Refused rather
  // than silently dropped — dropping a value the operator can still see on screen
  // makes the screen lie about what was written — and the reason renders beside
  // the disabled button so it is two keystrokes to clear, not a dead end.
  //
  // `isValidLoggedGrid` (ui/src/grid.ts) takes 4, 6 or 8 characters — every length
  // ADIF's GRIDSQUARE carries, so every square that uploads cleanly. It is the
  // record-side ruling, distinct from `isValidGrid`'s own-station 4/6 (which still
  // gates the setup wizard and the programming workbench, untouched).
  //
  // ARMED ONLY BY THE FIELD THAT IS ON SCREEN. `asksForGrid` is already false in
  // Phone/CW and on the FD path, so no layout without a Grid input can be held by
  // a value it does not render — which is why the FD Log button below needs no
  // `gridBlocked` term rather than being quietly missing one.
  const gridBlocked = asksForGrid && logGrid.trim() !== '' && !isValidLoggedGrid(logGrid)

  // The band / freq / mode this standard-log QSO will record: the hand-entered override when
  // it's open, else the live rig (byte-identical to before). When the override IS open it is
  // all-or-nothing — band, freq, mode AND time come from it together. A blank/garbage freq does
  // NOT silently fall back to the live rig for band+freq while keeping the override mode+time:
  // that logs a mismatched record (e.g. live 20m/14.2 MHz tagged FM at a past UTC time). Instead
  // logging is BLOCKED until the freq is valid or the override is closed (`overrideBlocked`).
  const ovFreqNum = Number(ovFreq)
  const ovFreqOk = overrideOpen && Number.isFinite(ovFreqNum) && ovFreqNum > 0
  const overrideBlocked = overrideOpen && !ovFreqOk
  const effBand = overrideOpen ? ovBand : snap.radio.band
  const effFreqMhz = overrideOpen ? ovFreqNum : snap.radio.dialMhz
  const effMode = overrideOpen ? ovMode : mode

  const logIt = async () => {
    const call = logCall.trim().toUpperCase()
    if (!call) return
    if (overrideBlocked) {
      pushToast(t('logEntry.override.blocked'), 'error')
      return
    }
    if (gridBlocked) {
      pushToast(t('logEntry.grid.blockedToast', { grid: logGrid.trim() }), 'error')
      return
    }

    if (fdActive) {
      // FD path: fdLogManual rejects on band+mode dupe. Class + section are MANDATORY — never log
      // a blank as the literal '?' (it would inflate the section multiplier), so bail until the
      // exchange is complete and the section is a real ARRL/RAC code.
      if (!fdExchangeOk) return
      const cls = fdClass.trim().toUpperCase()
      const sec = fdSection.trim().toUpperCase()
      const fmode = fdMode ?? 'PH'
      // The on-air mode behind the class, for 'DIG' alone — see `fdSubmode`. Sent only with
      // that class so a CW or phone contact can never acquire one it has no meaning for.
      const fsub = fmode === 'DIG' ? fdSubmode : undefined
      const r = await withErrorToast(
        () => fdLogManual(call, cls, sec, fmode, fsub),
        t('logEntry.fd.failed'),
      )
      if (r) {
        pushToast(
          t('logEntry.fd.logged', { call, class: cls, section: sec, mode: fmode }),
          'success',
        )
        reset()
      }
      return
    }

    // Standard logbook path.
    const rstSent = logRstSent.trim() || defaultRst
    const rstRcvd = logRstRcvd.trim() || defaultRst
    // Override open → the hand-entered UTC instant (falling back to now if a field was
    // cleared to something unparseable); closed → now, exactly as before.
    const whenUnix = overrideOpen
      ? (utcPartsToUnix(ovDate, ovTime) ?? Math.floor(Date.now() / 1000))
      : Math.floor(Date.now() / 1000)
    const rec: LoggedQso = {
      call,
      grid: logGrid.trim() || null,
      country: logCountry.trim() || null,
      state: logState.trim() || null,
      band: effBand,
      freqMhz: effFreqMhz,
      mode: effMode,
      rstSent,
      rstRcvd,
      name: logName.trim() || null,
      qth: logQth.trim() || null,
      comment: logComment.trim() || null,
      notes: logNotes.trim() || null,
      whenUnix,
      confirmed: false,
      awardConfirmed: false,
      // `asksForPark` FIRST: an exchange with no park row sends no park reference, by any
      // path. The test below it is "differs from the pending hunt", which is precisely the
      // shape a STALE latch passes — end the hunt or switch parks and the ref left in state
      // no longer matches, so it reads as an explicit entry and rides onto the record as
      // SIG/SIG_INFO. In a cockpit with a park row that is visible and editable; in one
      // without, a satellite contact would be filed as a park contact with nothing on
      // screen to reveal or clear it.
      //
      // Only send an EXPLICIT park. A hunt-PREFILLED ref (still equal to the pending hunt) is left
      // to the engine's callsign-matched auto-tag — which also clears the pend — so a prefill can
      // never ride onto a non-matching call, and the hunt tags exactly the right QSO once.
      ota:
        asksForPark &&
        logParkRef.trim() &&
        logParkRef.trim().toUpperCase() !== (snap.hunt?.reference ?? '').trim().toUpperCase()
          ? { theirProgram: logParkProgram, theirRef: logParkRef.trim().toUpperCase() }
          : undefined,
    }
    const r = await withErrorToast(() => logQso(rec), t('logEntry.logFailed'))
    if (r) {
      pushToast(t('logEntry.logged', { call, mode: effMode }), 'success')
      reset()
      refreshLog()
    }
  }

  const onEnter = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter') void logIt()
  }

  // Enter in the CALL field: on a fresh call (not yet enriched, no name typed) do the QRZ lookup
  // first — like Tab — so a single Enter pulls the callbook; once enriched, Enter logs as usual.
  const onCallEnter = (e: React.KeyboardEvent) => {
    if (e.key !== 'Enter') return
    const call = logCall.trim()
    const cu = call.toUpperCase()
    if (
      call &&
      !logName.trim() &&
      !qrzBusy &&
      enrichedForRef.current !== cu &&
      triedLookupRef.current !== cu
    ) {
      triedLookupRef.current = cu // mark attempted so the next Enter logs even if the lookup misses
      e.preventDefault()
      void lookup(false)
    } else {
      void logIt()
    }
  }

  // ---- FD variant render ----
  // Call/Class/Section are enlarged and stacked under clear captions so a logger
  // sitting beside the operator can read the exchange at a glance — using the
  // vertical room at the bottom of the strip. Scoped to .log-entry-fd, so the
  // normal (non-FD) log strip keeps its compact single-row layout.
  //
  // While-typing dupe verdict, zero IPC: the full own log already rides every
  // snapshot and the club-sync block ships club-ONLY keys, so both checks are
  // plain lookups on data in hand. OWN dupe = the hard block fdLogManual will
  // refuse (same key: call, band, mode class). CLUB dupe = another position
  // already worked them — N3FJP semantics, a WARNING only; logging proceeds
  // and the host keeps both rows.
  const fdTypedCall = logCall.trim().toUpperCase()
  const fdModeClass = fdMode ?? 'PH'
  const fdOwnDupe =
    fdActive &&
    fdTypedCall !== '' &&
    (fieldDay?.log ?? []).some(
      (q) =>
        q.call.toUpperCase() === fdTypedCall &&
        q.band === snap.radio.band &&
        (q.mode ?? '') === fdModeClass,
    )
  const fdClubDupe =
    fdActive &&
    !fdOwnDupe &&
    fdTypedCall !== '' &&
    (fieldDay?.club?.dupes ?? []).some(
      ([c, b, m]) => c === fdTypedCall && b === snap.radio.band && m === fdModeClass,
    )
  // …and the same draft, mirrored out to the host's boards (see `onFdDraftChange`). It reuses
  // `fdTypedCall` rather than re-normalizing so the cell a board paints is the key this strip
  // would log. Deliberately NOT keyed on the callback's identity: a host that passes an inline
  // arrow re-renders on every 300 ms snapshot, and firing on that would repaint the boards on
  // ticks where the operator typed nothing.
  useEffect(() => {
    if (!fdActive) return
    onFdDraftChange?.({
      call: fdTypedCall,
      cls: fdClass.trim().toUpperCase(),
      section: fdSection.trim().toUpperCase(),
    })
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [fdActive, fdTypedCall, fdClass, fdSection])
  if (fdActive) {
    return (
      <div className="log-entry log-entry-fd">
        <div className="le-fd-header">
          <span className="le-fd-chip">{t('logEntry.fd.chip')}</span>
          <span className="le-fd-mode">{fdMode ?? 'PH'}</span>
          <span className="le-fd-hint">{t('logEntry.fd.hint', { band: snap.radio.band })}</span>
        </div>

        <div className="le-fd-big">
          <label className="le-fd-field le-fd-field-call">
            <span className="le-fd-cap">{t('logEntry.fd.call.label')}</span>
            <input
              ref={callInputRef}
              className="settings-input mono le-fd-input le-fd-input-call"
              value={logCall}
              // SPACES ARE DROPPED, not kept: no callsign contains one, and the space bar is
              // the phone position's push-to-talk. An operator who reaches for it out of habit
              // while the caret is in this field would otherwise log "K1 ABC" verbatim —
              // `logIt` only trims the ends. Costs nothing on any other path (a pasted call
              // with a stray space comes out clean).
              onChange={(e) => setLogCall(e.target.value.replace(/\s+/g, '').toUpperCase())}
              onKeyDown={onEnter}
              placeholder={LOG_EXAMPLES.call}
              autoComplete="off"
              spellCheck={false}
            />
          </label>
          <label className="le-fd-field">
            <span className="le-fd-cap">{t('logEntry.fd.class.label')}</span>
            <input
              className="settings-input mono le-fd-input le-fd-input-code"
              value={fdClass}
              onChange={(e) => setFdClass(e.target.value.toUpperCase())}
              onKeyDown={onEnter}
              placeholder={LOG_EXAMPLES.fdClass}
              autoComplete="off"
              spellCheck={false}
              title={t('logEntry.fd.class.title')}
            />
          </label>
          <label className="le-fd-field">
            <span className="le-fd-cap">{t('logEntry.fd.section.label')}</span>
            <input
              className="settings-input mono le-fd-input le-fd-input-code"
              value={fdSection}
              onChange={(e) => setFdSection(e.target.value.toUpperCase())}
              onKeyDown={onEnter}
              placeholder={LOG_EXAMPLES.fdSection}
              autoComplete="off"
              spellCheck={false}
              title={t('logEntry.fd.section.title')}
            />
          </label>
          {/* No `gridBlocked` term, and that is not an omission: `asksForGrid`
              is false whenever `fdActive` is, so `logIt`'s grid guard — which
              sits above the FD branch — is provably inert on this path. A layout
              with no Grid input can never be held by grid state. */}
          <button
            type="button"
            className="le-log-btn le-fd-log-btn"
            onClick={logIt}
            disabled={!logCall.trim() || !fdExchangeOk}
          >
            {t('logEntry.fd.log')}
          </button>
          <button
            type="button"
            className="le-qrz"
            onClick={reset}
            title={t('logEntry.clear.title')}
          >
            {t('logEntry.clear.label')}
          </button>
        </div>

        {/* THE VERDICT SLOT — always present, empty or not.

            The dock this strip lives in (FD cockpit) is bottom-anchored, so anything that makes
            the strip taller moves the fields UP, and these three verdicts appear and vanish PER
            KEYSTROKE right under the fingers that are typing. Reserving the height here rather
            than on the strip is the difference between a reservation and a coincidence: a floor
            on `.log-entry-fd` has to be re-derived every time the header row or the field row
            changes height, and the one this shipped with (8.5em = 119px against a 117px natural
            strip) reserved two pixels of an eleven-pixel line — i.e. nothing. The wrapper is
            zero-height in every other host, so Phone and CW are unchanged. */}
        <div className="le-fd-verdicts">
        {logCall.trim() !== '' && !fdExchangeOk && (
          <div className="le-fd-hint" role="alert">
            {fdClass.trim() === ''
              ? t('logEntry.fd.needClass')
              : t('logEntry.fd.badSection', { section: fdSection.trim() || '—' })}
          </div>
        )}
        {fdOwnDupe && (
          <div className="le-fd-hint" role="alert">
            {t('logEntry.fd.dupe.own', {
              call: fdTypedCall,
              band: snap.radio.band,
              mode: fdModeClass,
            })}
          </div>
        )}
        {fdClubDupe && (
          <div className="le-fd-hint" role="status">
            {t('logEntry.fd.dupe.club', {
              call: fdTypedCall,
              band: snap.radio.band,
              mode: fdModeClass,
            })}
          </div>
        )}
        </div>
      </div>
    )
  }

  // ---- Standard logbook variant render ----
  // A pending POTA/SOTA hunt (from a spot click) auto-tags the matching QSO by callsign at log
  // time — surface it so the operator SEES the park will be recorded (edit/manual entry is on the
  // dedicated park field). Matches when the logged call equals the hunted activator.
  const hunt = snap.hunt
  const huntMatches =
    hunt != null &&
    logCall.trim() !== '' &&
    hunt.call.trim().toUpperCase().split('/')[0] === logCall.trim().toUpperCase().split('/')[0]

  return (
    <div className="log-entry">
      {titled && <h2>{t('logEntry.title')}</h2>}

      {hunt && (
        <div className={`le-hunt-chip${huntMatches ? ' match' : ''}`} title={t('logEntry.hunt.title')}>
          🌲 {hunt.program} {hunt.reference}
          <span className="le-hunt-for"> · {hunt.call}</span>
          {!huntMatches && logCall.trim() !== '' && (
            <span className="le-hunt-warn"> {t('logEntry.hunt.mismatch')}</span>
          )}
        </div>
      )}

      <div className="le-row">
        <input
          ref={callInputRef}
          className="settings-input mono le-call"
          value={logCall}
          onChange={(e) => {
            humanCallEditRef.current = true
            setLogCall(e.target.value.toUpperCase())
          }}
          onBlur={onCallBlur}
          onKeyDown={onCallEnter}
          placeholder={t('logEntry.call.placeholder')}
          autoComplete="off"
          spellCheck={false}
        />
        {/* "Lookup", not "QRZ": the command tries QRZ first and falls through to HamQTH, so the
            old label named only half of what the button does. It is also styled as a quiet
            utility (.le-lookup) and is now the only action in this row — see .le-actions below. */}
        <button
          type="button"
          className="le-qrz le-lookup"
          onClick={() => void lookup(false)}
          disabled={qrzBusy || !logCall.trim()}
          title={t('logEntry.lookup.title')}
        >
          {qrzBusy ? '…' : t('logEntry.lookup.label')}
        </button>
        <label className="le-rst-field" title={t('logEntry.rstSent.title')}>
          <span className="le-rst-cap">{t('logEntry.rstSent.label')}</span>
          <input
            ref={rstRef}
            className="settings-input mono le-rst"
            value={logRstSent}
            onChange={(e) => setLogRstSent(e.target.value)}
            onKeyDown={onEnter}
            placeholder={LOG_EXAMPLES.rst}
            autoComplete="off"
          />
        </label>
        <label className="le-rst-field" title={t('logEntry.rstRcvd.title')}>
          <span className="le-rst-cap">{t('logEntry.rstRcvd.label')}</span>
          <input
            className="settings-input mono le-rst"
            value={logRstRcvd}
            onChange={(e) => setLogRstRcvd(e.target.value)}
            onKeyDown={onEnter}
            placeholder={LOG_EXAMPLES.rst}
            autoComplete="off"
          />
        </label>
        {/* GRID — the last of this cluster still write-only. `logGrid` has always
            reached the RECORD and printed in the summary line and the recall card,
            but only a callbook could ever write it: the square a station passed ON
            AIR could not be entered at all.

            IN THE EXCHANGE ROW, beside the reports — not down in the QTH row with
            State and Country. Those are callbook facts about where a station
            LIVES; a grid on a bird is something the other operator SAYS, in the
            same breath as the report, and it is read and typed at the same moment.
            It also costs nothing here, which is the whole reason the first attempt
            was refused: this row already wraps at both measured widths, and Grid
            fits in the slack a wrapped line leaves. In the QTH row it did not —
            that row is a single full line at a 611 px column, so Grid pushed it to two and
            made the satellite strip TALLER than the park row it replaced (+52 vs
            −48). Measured in a real layout engine, and its two halves are pinned:
            LogEntry.test.tsx › 'sits in the EXCHANGE row' and 'the QTH row is
            BYTE-FOR-BYTE the same four fields' hold the placement, and
            panel-overflow.test.ts › '.le-grid fits the LINE BUDGET' computes the
            width that keeps it off a line of its own. Numbers in the CHANGELOG. */}
        {asksForGrid && (
          <input
            className={`settings-input mono le-grid${gridBlocked ? ' invalid' : ''}`}
            value={logGrid}
            onChange={(e) => setLogGrid(e.target.value.toUpperCase())}
            onKeyDown={onEnter}
            placeholder={t('logEntry.grid.placeholder')}
            autoComplete="off"
            spellCheck={false}
            aria-invalid={gridBlocked}
            title={t('logEntry.grid.title')}
          />
        )}
        <input
          className="settings-input le-name"
          value={logName}
          onChange={(e) => setLogName(e.target.value)}
          onKeyDown={onEnter}
          placeholder={t('logEntry.name.placeholder')}
          autoComplete="off"
        />
        <button type="button" className="le-qrz" onClick={reset} title={t('logEntry.clear.title')}>
          {t('logEntry.clear.label')}
        </button>
      </div>

      <div className="le-row">
        <input
          className="settings-input le-qth"
          value={logQth}
          onChange={(e) => setLogQth(e.target.value)}
          onKeyDown={onEnter}
          placeholder={t('logEntry.qth.placeholder')}
          autoComplete="off"
        />
        {/* State + Country are auto-filled from the QRZ lookup / cty.dat country resolve and
            were previously write-only — visible in the summary line but not editable here.
            Surfaced beside QTH because they are the same class of information (city, state,
            country) and an operator who hears the state on air should not have to open the
            logbook afterwards to correct it. */}
        <input
          className="settings-input le-state"
          value={logState}
          onChange={(e) => setLogState(e.target.value)}
          onKeyDown={onEnter}
          placeholder={t('logEntry.state.placeholder')}
          autoComplete="off"
          title={t('logEntry.state.title')}
        />
        <input
          className="settings-input le-country"
          value={logCountry}
          onChange={(e) => setLogCountry(e.target.value)}
          onKeyDown={onEnter}
          placeholder={t('logEntry.country.placeholder')}
          autoComplete="off"
          title={t('logEntry.country.title')}
        />
        <input
          className="settings-input le-comment"
          value={logComment}
          onChange={(e) => setLogComment(e.target.value)}
          onKeyDown={onEnter}
          placeholder={t('logEntry.comment.placeholder')}
          autoComplete="off"
        />
      </div>

      {/* THE PARK/SUMMIT REFERENCE — part of the terrestrial exchange only.
          The Satellites section asks for an exchange that has no park in it, so
          this is NOT ASKED FOR there rather than hidden there: there is nothing
          on a bird for a POTA/SOTA reference to name, and the row was taking
          vertical space in a column the operator shares with the sky dome
          (operator, 0.28.1: "that section still has a pota/sota section, which
          is shouldnt"). The hunt chip above stays in every exchange on purpose —
          it reports what the ENGINE will do (it auto-tags a pending hunt onto a
          matching callsign at log time, whichever strip logged it), so removing
          it would make that tag silent rather than absent. */}
      {asksForPark && (
      <div className="le-row le-park-row">
        <select
          className="settings-input le-park-prog"
          value={logParkProgram}
          onChange={(e) => setLogParkProgram(e.target.value)}
          title={t('logEntry.park.program.title')}
        >
          {PARK_PROGRAMS.map((p) => (
            <option key={p} value={p}>
              {p}
            </option>
          ))}
        </select>
        <div className="le-park-search">
          <input
            className="settings-input mono le-park-ref"
            value={logParkRef}
            onChange={(e) => setLogParkRef(e.target.value.toUpperCase())}
            onKeyDown={onEnter}
            onBlur={() => window.setTimeout(() => setParkHits([]), 150)}
            placeholder={
              logParkProgram === 'SOTA'
                ? t('logEntry.park.ref.placeholderSota', { example: LOG_EXAMPLES.sotaRef })
                : t('logEntry.park.ref.placeholderPota', { example: LOG_EXAMPLES.potaRef })
            }
            title={t('logEntry.park.ref.title')}
            autoComplete="off"
            spellCheck={false}
          />
          {parkHits.length > 0 && (
            <ul className="le-park-suggest">
              {parkHits.map((p) => (
                <li key={p.reference}>
                  <button
                    type="button"
                    onMouseDown={(e) => {
                      e.preventDefault() // pick before the input's onBlur clears the list
                      setParkPicked(true)
                      setLogParkRef(p.reference)
                      setParkHits([])
                    }}
                  >
                    <span className="mono le-park-hit-ref">{p.reference}</span>
                    <span className="le-park-hit-name">
                      {p.name}
                      {p.location ? ` · ${p.location}` : ''}
                    </span>
                  </button>
                </li>
              ))}
            </ul>
          )}
        </div>
      </div>
      )}

      {asksForPark &&
        parkDetail &&
        logParkProgram === 'POTA' &&
        // Only show the chip while it still matches the field — never the PREVIOUS ref's park
        // during an in-flight lookup of a newly-typed ref (both paths return the normalized ref).
        parkDetail.reference === logParkRef.trim().toUpperCase() && (
        <div className="le-park-detail" role="status">
          <span className="mono le-park-detail-ref">{parkDetail.reference}</span>
          <span className="le-park-detail-name">{parkDetail.name || '—'}</span>
          {parkDetail.location && <span className="le-park-detail-loc">{parkDetail.location}</span>}
          {parkDetail.grid && <span className="mono le-park-detail-grid">{parkDetail.grid}</span>}
          {parkDetailLive && (
            <span className="le-park-detail-src" title={t('logEntry.park.live.title')}>
              {t('logEntry.park.live.label')}
            </span>
          )}
        </div>
      )}

      <textarea
        className="settings-input le-notes"
        value={logNotes}
        onChange={(e) => setLogNotes(e.target.value)}
        placeholder={t('logEntry.notes.placeholder')}
        rows={2}
        spellCheck
      />

      {/* Opt-in override for a contact made on a radio not connected to Nexus (a V/UHF rig,
          say). Collapsed by default: the common "log the QSO I just made on the connected rig"
          flow is untouched. Open it to hand-enter band / freq / mode / UTC time. */}
      <div className="le-override">
        <button
          type="button"
          className="le-override-toggle"
          aria-expanded={overrideOpen}
          onClick={() => (overrideOpen ? setOverrideOpen(false) : openOverride())}
          title={t('logEntry.override.title')}
        >
          <span className="le-override-caret" aria-hidden="true">
            {overrideOpen ? '▾' : '▸'}
          </span>
          {t('logEntry.override.toggle')}
          <span className="le-override-sub"> {t('logEntry.override.toggleSub')}</span>
        </button>

        {overrideOpen && (
          <div className="le-override-fields">
            <label className="le-ov-field">
              <span className="le-rst-cap">{t('logEntry.override.date.label')}</span>
              <input
                type="date"
                className="settings-input le-ov-date"
                value={ovDate}
                onChange={(e) => setOvDate(e.target.value)}
              />
            </label>
            <label className="le-ov-field">
              <span className="le-rst-cap">{t('logEntry.override.time.label')}</span>
              <input
                type="time"
                className="settings-input le-ov-time"
                value={ovTime}
                onChange={(e) => setOvTime(e.target.value)}
              />
            </label>
            <label className="le-ov-field">
              <span className="le-rst-cap">{t('logEntry.override.band.label')}</span>
              <select
                className="settings-input le-ov-band"
                value={ovBand}
                onChange={(e) => onPickOvBand(e.target.value)}
              >
                {LOG_BANDS.map((b) => (
                  <option key={b.band} value={b.band}>
                    {b.band}
                  </option>
                ))}
              </select>
            </label>
            <label className="le-ov-field">
              <span className="le-rst-cap">{t('logEntry.override.freq.label')}</span>
              <input
                type="text"
                inputMode="decimal"
                className="settings-input mono le-ov-freq"
                value={ovFreq}
                onChange={(e) => onEditOvFreq(e.target.value)}
                placeholder={LOG_EXAMPLES.freqUnit}
                autoComplete="off"
                spellCheck={false}
              />
            </label>
            <label className="le-ov-field">
              <span className="le-rst-cap">{t('logEntry.override.mode.label')}</span>
              <select
                className="settings-input le-ov-mode"
                value={ovMode}
                onChange={(e) => setOvMode(e.target.value)}
              >
                {LOG_MODES.map((m) => (
                  <option key={m} value={m}>
                    {m}
                  </option>
                ))}
              </select>
            </label>
            {ovFreq.trim() !== '' && bandForMhz(ovFreqNum) !== ovBand && (
              <span className="le-ov-warn" role="alert">
                {Number.isFinite(ovFreqNum) && ovFreqNum > 0
                  ? t('logEntry.override.offBand', { freq: ovFreq, band: ovBand })
                  : t('logEntry.override.needFreq')}
              </span>
            )}
          </div>
        )}
      </div>

      <span className="le-hint">
        {overrideBlocked ? (
          <span className="le-ov-warn">{t('logEntry.override.blockedHint')}</span>
        ) : gridBlocked ? (
          // Why Log went dead, in the place that otherwise states what will be
          // written. `role="alert"` because it is the only explanation of a
          // disabled primary action, and a screen-reader operator gets no other.
          <span className="le-ov-warn le-grid-warn" role="alert">
            {t('logEntry.grid.blocked', { grid: logGrid.trim() })}
          </span>
        ) : (
          <>
            {/* The band slot is EMPTY for a dial the band plan cannot name (QO-100 at
                10.489 GHz, the microwave birds) — printing it anyway left a doubled
                separator with a hole in it, "as SSB ·  · 10489.550 MHz". Two whole
                messages rather than one with an optional slot, for the same reason the
                satellite rail drops its band chip: a sentence built around an empty slot
                reads as a hole, and ADIF's BAND field is optional anyway.
                The grid and country tails are separator + token — nothing to translate. */}
            {effBand
              ? t('logEntry.summaryBand', {
                  mode: effMode,
                  band: effBand,
                  freq: effFreqMhz.toFixed(3),
                })
              : t('logEntry.summary', { mode: effMode, freq: effFreqMhz.toFixed(3) })}
            {logGrid ? ` · ${logGrid}` : ''}
            {logCountry ? ` · ${logCountry}` : ''}
          </>
        )}
      </span>

      {/* The commit row. Log used to sit in the top field row beside the callbook button — and
          that row WRAPS (the log pane narrows to 24em), so Log's position on screen moved with
          the pane width and a mid-QSO glance reaching for the lookup could commit a contact
          instead (operator: several logged by accident, 2026-08-02). Here it is a deliberate
          reach, directly under the line that states what will be written, and Spot stays beside
          it — that adjacency is why spotting happens at all. Plain in-flow row: the pane body is
          the scroller, this adds no sticky/fixed positioning and no second scroll owner.

          `position: sticky; bottom: 0` WAS PROPOSED FOR THIS ROW (2026-08-04 density review) and
          refused — and NOT for the scroll-owner reason above, which does not apply: sticky inside
          an existing `overflow: auto` box adds no second scroller. It is refused because this row
          is not the last block in the strip. RecallPanel follows it, so an opaque sticky commit
          row parks Log over the recall card — its QRZ link included — for the whole scroll, and a
          reach for something underneath commits a contact instead. That is the 2026-08-02 accident
          again, from a fixed screen band rather than a wrapping row. It would also cut Log loose
          from the "logs to the shared logbook as …" line above it, which is the placement's whole
          point. The fold problem it was aimed at was answered by deleting chrome instead: the
          heading and the card-in-card padding, 43px of it above this row (LogEntry.density.test). */}
      <div className="le-row le-actions">
        <button
          type="button"
          className="le-log-btn"
          onClick={logIt}
          disabled={!logCall.trim() || overrideBlocked || gridBlocked}
          title={
            overrideBlocked
              ? t('logEntry.override.blocked')
              : gridBlocked
                ? t('logEntry.grid.blockedTitle')
                : undefined
          }
        >
          {t('logEntry.log')}
        </button>
        {onSpot && (
          <button
            type="button"
            className="le-spot-btn"
            onClick={() => onSpot(logCall.trim().toUpperCase())}
            disabled={!logCall.trim()}
            title={t('logEntry.spot.title')}
          >
            {t('logEntry.spot.label')}
          </button>
        )}
      </div>

      <RecallPanel
        call={logCall}
        band={snap.radio.band}
        name={logName}
        qth={logQth}
        grid={logGrid}
        lat={logCoords?.lat ?? null}
        lon={logCoords?.lon ?? null}
        country={logCountry}
        image={logImage}
        myGrid={snap.mygrid}
        hist={hist}
        newEntity={newEntity}
        newBandSlot={newBandSlot}
        newModeSlot={newModeSlot}
        onOpenLog={onOpenLogbook}
      />
    </div>
  )
}
