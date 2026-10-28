// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Every operator-visible
// string comes from the catalog (`i18n/en.ts`); a hardcoded one fails CI. What does NOT come from
// the catalog is LOG_EXAMPLES and PARK_PROGRAMS below, plus everything this strip prints about a
// contact — the callsign, RST, grid, band, mode and frequency are wire values, and a locale that
// "translated" one would have broken an example of a wire format. See the invariant-token rule in
// `i18n/index.ts`.

import { Fragment, useEffect, useRef, useState } from 'react'
import { X } from 'lucide-react'
import {
  isOnAir,
  type AppSnapshot,
  type ContestFieldSpec,
  type ContestFieldValue,
  type FieldDayQso,
  type FieldDayStatus,
  type LoggedQso,
} from '../types'
import { t } from '../i18n'
import { clearHuntTarget, contestEntryReset, contestIMoved, contestLogManual, contestLogManualRows, contestLogSatellite, contestRemoveLast, contestLoggedQth, contestPlace, contestWorking, logQso, lookupPark, lookupParkLive, qrzLookup, resolveEntity, searchParks, setCwPeerInfo, setLogFormGrid, type Park, type StateSource } from '../api'
import { bandKey, modeKey } from '../features/callHistory'
import { potaDupe } from '../features/potaActivation'
import { emptyAnswer } from '../features/logAnswers'
import { useLogAnswer } from '../features/logSource'
import { UTC_DATE_FORMAT, UTC_TIME_FORMATS, parseUtcDate, parseUtcTime, utcDateTimeToUnix } from '../features/utcLog'
import {
  domainSuggestions,
  type DomainValue,
} from '../features/contestDomains'
import {
  countyLineDomain,
  countyList,
  isCountyLine,
  lastCountyPart,
  readCountyLine,
  withLastCountyPart,
} from '../features/countyLine'
import { composingSlot } from '../features/contestExchange'
import { BLANK_LINE, readLogDraft, writeLogDraft, type BandClearedLine, type LineContext, type LogDraft } from '../features/logDraft'
import { CALL_HISTORY_SOURCE, SCP_SOURCE, useCallHistory, useScpList } from '../features/contestLists'
import { scpLine } from '../features/scp'
import { callCardTrigger } from './CallCard'
import { applyFill, contestLogFill, EMPTY_FILL, historyFill, loggedQthFill, offerFill, typedIn, type FillSource, type FillState } from '../features/callHistoryFill'
import { fdFieldOk, fdSlotValue, fdVerdict, receivedCaption, slotCaption, slotTitle } from '../features/contestSlots'
import { locationWarningText } from '../features/contestLocation'
import { contestUnsavedText } from '../features/contestUnsaved'
import { contestDupe, contestDupeRow, contestWorkedRows, workedValues, type ContestDupeVerdict } from '../features/contestDupe'
import { REMOVE_CONFIRM_MS, contactLabel, isModifierKey, isRemoveKey, newestContact, removalText } from '../features/contestRemoval'
import { askForOperatorBox, isOperatorBoxKey, isWipeKey } from '../features/contestKeys'
import { isMarkKey, markHere, publishEntryCall } from '../features/freqMarkers'
import { closedText, isEditKey } from '../features/contestEdit'
import { ContestContactEdit, focusOpenContestEdit, type EditNote } from './ContestContactEdit'
import { esmPress, type EsmCaret, type EsmRole, type EsmStrip } from '../features/esm'
import { ENTRY_CHANGED, ENTRY_LOGGED, useSharedEntry, type StripContent } from '../features/contestEntryShare'
import type { EsmHost } from '../features/esmHost'
import { esmRefusalText, esmStepName } from '../features/esmWords'
import { contestRoleWords, isFieldDay } from '../fdEvent'
import { azimuthLabel, azimuthTo, isValidLoggedGrid } from '../grid'
import { baseCall, sameCall } from '../callsign'
import { placeCode, placeName } from '../features/otaStates'
import { RecallPanel } from './RecallPanel'
import { ContestScoreChip } from './ContestScoreChip'
import { pushToast, withErrorToast } from '../toast'
import { isStreamInput } from '../remote-native/stream-input'
import { normaliseTyped, oneLineTyped, upperTyped } from '../typedInput'
import { IotaOfferChip, useIotaOffer } from './Iota'

// ---------------------------------------------------------------------------
// THE DYNAMIC ENTRY STRIP (spec §9) — Call plus one box per slot the session's role
// RECEIVES, in receive order.
//
// ⚠️ **Layout is a contract, not a preference.** The strip is a control strip:
// `fit="content"` per the pane-grid contract, so it may never use surplus and may
// never grow unboundedly with the field count. Budget: Call plus up to FIVE received
// fields on one row at the 1024 px floor, wrapping to a second row below it — and the
// rules validator refuses a role receiving more than five, so five is the ceiling in
// both places. Sweepstakes (serial, precedence, call, check, section) is the sizing
// case, and it is measured in the headless-Chrome harness (`ui/layout-harness/
// entry-strip.html`), never in jsdom — jsdom lays out nothing and every rect is 0.
// ---------------------------------------------------------------------------

/** How long a call stands in the strip before it is reported to the Rotor box (`onEntryCall`). */
export const ENTRY_SETTLE_MS = 300

/** The two slots a Field Day role receives — what a snapshot from a build older than
 *  `FieldDayStatus.receives` describes, and what this strip has always rendered. */
const FD_RECEIVES_FALLBACK: ContestFieldSpec[] = [
  { key: 'CLASS', kind: 'pattern', required: true },
  { key: 'SECTION', kind: 'enum', required: true, domain: 'fd_sections' },
]

/** The caption for a SENT slot's box, by id — `features/contestSlots.ts`. A slot id is an
 *  INVARIANT TOKEN; its caption is PROSE. A RECEIVED box is captioned by what it holds for this
 *  station's role (`receivedCaption`), as the contest log table and the edit caption it. */
const fdFieldLabel = slotCaption

/** The word on a filled box's caption: where the fill came from (a fill with no source is the
 *  call-history file's, the only source there once was). */
function fillMark(source: FillSource | undefined): string {
  if (source === 'contest') return t('logEntry.fill.contest')
  if (source?.startsWith('log:')) return t('logEntry.fill.log', { year: source.slice(4) })
  return t('logEntry.history.mark')
}
/** …and its title, which says the same at length. */
function fillTitle(source: FillSource | undefined, file: string): string {
  if (source === 'contest') return t('logEntry.fill.contest.title')
  if (source?.startsWith('log:')) return t('logEntry.fill.log.title', { year: source.slice(4) })
  return t('logEntry.history.title', { file })
}

/** A row's UTC time as HH:MM, the DUPE badge's "when". Digits only: an invariant token. */
function utcHhMm(whenUnix: number): string {
  const d = new Date(whenUnix * 1000)
  const p = (n: number) => String(n).padStart(2, '0')
  return `${p(d.getUTCHours())}:${p(d.getUTCMinutes())}`
}

/** One received value off a logged DTO row, by slot id.
 *
 *  ⚠️ A DTO row carries `class`/`section`, not a received VECTOR — the generalised row
 *  lands with the contests that receive something else. Until then this is the whole
 *  mapping, and an unknown slot reads back empty rather than guessing a column. */
function fdRcvd(row: Pick<FieldDayQso, 'class' | 'section'>, key: string): string {
  if (key === 'CLASS') return row.class
  if (key === 'SECTION') return row.section
  return ''
}

/** The boxes the next contact starts with once `row` is in the log: a signal report goes back to
 *  untouched (absent, so it shows the report of the mode the strip logs under), and every other
 *  box takes what a row carries over (`fdRcvd`), empty for the rest. No row (the contact was taken
 *  back since): every box empties. */
function afterLogged(
  fields: Record<string, string>,
  receives: ContestFieldSpec[],
  row: Pick<FieldDayQso, 'class' | 'section'> | undefined,
): Record<string, string> {
  const next = { ...fields }
  for (const f of receives) {
    if (f.kind === 'rst') delete next[f.key]
    else next[f.key] = row ? fdRcvd(row, f.key) : ''
  }
  return next
}

/** The report a contest strip offers before anything is typed: 59 on phone, 599 on CW and
 *  digital — the same split the engine makes for a sent RST on a phone row. */
function defaultContestRst(fdMode: 'CW' | 'PH' | 'DIG' | undefined): string {
  return (fdMode ?? 'PH') === 'PH' ? '59' : '599'
}

/** Where the optional "RST sent" box keeps a report typed in it, beside the received slots' ids:
 *  lower case, so no ruleset's slot id (upper case, always) can be it. It lives with the boxes, so
 *  the shared entry, Alt+W and Clear carry it as they carry them. */
const SENT_RST_KEY = 'sentRst'

/** A typed sent RST is logged only with its mode's digit count: 2 on phone (R 1–5, S 1–9), 3 on CW
 *  and digital (and T 1–9). The engine's own check (`ContestSession::sent_rst_ok`) refuses any
 *  other, so the strip holds Log back rather than sending one it will refuse. */
function sentRstOk(raw: string, fdMode: 'CW' | 'PH' | 'DIG' | undefined): boolean {
  return ((fdMode ?? 'PH') === 'PH' ? /^[1-5][1-9]$/ : /^[1-5][1-9]{2}$/).test(raw)
}

/** The cty.dat names of the two countries whose stations send a QTH in CQ WW RTTY —
 *  `propagation::dxcc`'s own spellings, which src-tauri's country-file test pins. */
const WVE_ENTITIES: ReadonlySet<string> = new Set(['United States', 'Canada'])

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

/** The `LOG_MODES` entry that covers `m` — what the override's `<select>` opens on.
 *
 * A cockpit's live mode is not always a MODE: the Phone cockpit logs the SIDEBAND a contact
 * was worked on ("USB"/"LSB"), which is an ADIF SUBMODE and is deliberately not in the list
 * above. Seeding the select with it left a CONTROLLED `<select>` holding a value no `<option>`
 * matches — and the box does NOT go blank, which is the trap. Nothing ends up selected, so the
 * HTML selectedness algorithm picks the FIRST option for a single-select: the operator reads
 * **SSB** while React state still says LSB, no warning anywhere, and committing the contact
 * writes a mode different from the one the strip is showing. A strip whose job is to state
 * what will be written must not be able to do that.
 *
 * So the override opens on the parent MODE. That is the honest reading of what it is: opening
 * it is an explicit "I am entering this contact by hand", it is all-or-nothing (band, freq,
 * mode AND time come from it together), and its vocabulary is MODE values. The ordinary
 * closed-override path is untouched and still carries the sideband. `modeKey` is the same fold
 * the dupe check and the stats use, so there is one answer to "which mode is this" in the UI. */
function overrideModeFor(m: string): string {
  const k = modeKey(m)
  return (LOG_MODES as readonly string[]).includes(k) ? k : m
}

/**
 * The invariant hints this strip shows in empty fields, gathered so the guard can prove they never
 * became catalog entries: the signal-report format's name and a unit, the same characters in every
 * language. Neither is a sample value; a box that would only show one (a callsign, a class, a
 * section, a park) is left blank.
 */
const LOG_EXAMPLES = {
  rst: 'RST',
  freqUnit: 'MHz',
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

interface Props {
  /** A typed hosted log adapter. All station lookups and native side effects
   * remain outside this form when provided; only an explicit Log submits. */
  remote?: {
    submit: (record: LoggedQso, time: 'station' | 'explicit') => Promise<void>
    canSubmit: boolean
    busy: boolean
    pending?: boolean
    resetKey: number
    recall: (call: string, context?: { band: string; freqMhz: number; mode: string }) => React.ReactNode
  }

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
  onOpenLogbook?: (call: string, id?: string | null) => void
  /** Click-to-work handoff from the Needed board: the callsign to prefill + focus RST.
   * `ts` changes per click so re-working the same call refires the prefill. `park` is the park
   * or summit a Needed row is an activation of: it fills the park box for that call, and no
   * hunt is set (see `workPark`). */
  pendingWork?: { call: string; ts: number; park?: { program: string; reference: string } | null } | null
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
    /** The report we SEND them, when the host knows it (JS8: their SNR at this station, which
     *  JS8Call's log dialog puts in "RST sent", mainwindow.cpp:3891-3896). Filled once per
     *  call, into a box nobody has typed in, as `rst` fills the received one. */
    rstSent?: string | null
  } | null
  /** Hz above the dial the contact was on, for hosts whose logged frequency is the dial plus the
   *  audio offset (JS8Call logs `m_freqNominal + freq()`, mainwindow.cpp:3899). Absent = the
   *  dial, as before. An override's own frequency still wins. */
  logOffsetHz?: number
  /** When the contact started, as the host knows it, for the call it names: JS8Call's QSO start
   *  is the moment the call was selected (`m_callSelectedTime`, mainwindow.cpp:3860-3866). Used
   *  only while the strip's call is that call, and then the contact's end is when it is logged.
   *  An override's own time still wins. */
  startedAt?: { call: string; unix: number } | null
  /**
   * The host holds the SAME callsign in a field of its own (RTTY's Call box), and the two are
   * one field shown twice. Called with the strip's call, trimmed and uppercased, whenever it
   * changes by any path but a `cwLive` fill — typing, a click-to-work, or the clear after a
   * contact is logged — so the host's box follows the strip.
   *
   * ⚠️ IT CHANGES WHAT `cwLive` MEANS. Without it, `cwLive` fills a call this strip has not
   * filled before and never clobbers one the operator typed over it. With it, `cwLive.call` is
   * simply the host's side of the one field: whenever it differs from the call both sides last
   * agreed on, the strip takes it — including an empty one (the host's box was cleared) and
   * including the call just logged (a station worked again). And a value this strip reported is
   * never filled straight back, which is what keeps a keystroke typed during the round trip.
   */
  onCallChange?: (call: string) => void
  /**
   * The call this strip would log, trimmed and uppercased, for the Rotor box beside its cockpit
   * (the bearing to it, and Point). Reported however the call got there, a fill included, once it
   * has stood for `ENTRY_SETTLE_MS`, so a call typed a key at a time is reported once rather than
   * per keystroke (each report re-renders the window and asks the station for a bearing); and `''`
   * the moment this strip goes, since a strip that is not on screen has no call in it.
   */
  onEntryCall?: (call: string) => void
  /** Called when the entry is cleared for the next contact: after one is logged, or by ✕. */
  onReset?: () => void
  /**
   * A host's question about a contact, answered "log it" by the operator (JTTY's Log QSO prompt
   * after a TU): a new `id` per press. The strip logs ONLY the station it names: when the strip's
   * call is `call` (trimmed, uppercased) and it could log, `claim` is asked (the host's guard
   * against a second screen logging the same contact) and on true the strip logs exactly as its
   * own Log does. Otherwise it logs nothing: an empty strip is filled with `call` for the operator
   * to check and press again, and a strip holding another call is left as it is. An empty `call`
   * names no station, so the strip's own is logged. The outcome is reported to `onLogRequest`;
   * after `logging`, `refused` follows when the log then logged nothing (the logger window's entry
   * moved on, the log refused it), so the host can give its claim back.
   */
  logRequest?: { id: number; call: string; claim: () => Promise<boolean> } | null
  onLogRequest?: (
    id: number,
    outcome: { kind: 'logging' | 'filled' | 'not-ready' | 'taken' | 'refused' } | { kind: 'other-call'; held: string },
  ) => void
  /** Called with the snapshot `clearHuntTarget` answered, after the hunt tag's ✕ ended the hunt, so
   *  the host applies it at once: the POTA / SOTA view's `onSnap`, for its banner ✕. */
  onSnap?: (s: AppSnapshot) => void
  /**
   * When provided, the component enters FD mode: contacts go to contestLogManual()
   * instead of the general logbook.  The `mode` prop determines the FD mode
   * code ('CW' in CwCockpit, 'PH' in PhoneCockpit).
   */
  fieldDay?: FieldDayStatus | null
  /**
   * The FD mode code to pass to contestLogManual: the SCORING CLASS this position logs under.
   *
   * ⚠️ 'DIG' IS ONE OF THE THREE, and leaving it out was a silent scoring error. Field Day
   * scores CW, phone and digital as three classes, and the engine's `log_mode_at`
   * (tempo-core/src/fieldday.rs) already handles 'DIG' specially — it stamps the actual on-air
   * submode behind the class so exports emit the real mode. A digital position logging a
   * station by hand was the first consumer with a DIG contact to log; before this it had to
   * send 'PH', which credits the wrong class and dupes against the wrong cell.
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
   * CONTEST STRIP ONLY — fill ONE received exchange box from outside the strip: the RTTY
   * cockpit's double-click grab of a zone or a QTH out of the decoded text.
   *
   * `key` is the slot id (`'ZN'`, `'QTH'`), `value` the text, and `ts` makes each grab an
   * event: the box is refilled on every NEW `ts`, including the same value again after the
   * operator overtyped it. A slot this session does not receive is ignored rather than
   * invented, focus is never moved, and nothing is logged — a fill is a keystroke the
   * operator did not have to make, not a commit.
   */
  fillExchange?: { key: string; value: string; ts: number } | null
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
  /**
   * CONTEST STRIP ONLY — is this strip's host the view on screen? Ctrl+D (remove the newest
   * contact) is a WINDOW key, and the RTTY, PSK and JS8 cockpits stay mounted while hidden, so
   * each hidden strip would otherwise take every press too. Default true: a host mounted only
   * while it shows has nothing to say here.
   */
  active?: boolean
  /**
   * CONTEST STRIP ONLY — Enter Sends Message, from the CW, RTTY and Phone cockpits
   * (`features/esmHost.ts`): with `on`, Enter in the strip's boxes sends the contact's next
   * message through the cockpit's own send path and logs the contact at its last step. Absent,
   * or off, and Enter is exactly what it always was. Never on the hosted page.
   */
  esm?: EsmHost
  /**
   * CONTEST STRIP ONLY — this is the contest logger window's strip. While that window is open
   * every contest strip on screen shares one contact in progress (`features/contestEntryShare`);
   * the logger's never puts what it holds over the shared entry when the sharing starts (the
   * main window's strip does, so a logger opened mid-contact shows that contact), and in its
   * window Esc clears the entry as Clear does. Nothing in that window transmits, so its Esc has
   * no over to stop. Absent everywhere else.
   */
  sharedEntry?: 'logger'
  /**
   * CONTEST STRIP ONLY — does the strip show the contest score chip in its own header row?
   * Default true. A cockpit that frames this strip in a Log pane passes false and shows the
   * chip in that pane's head instead, beside the title: there the strip sits in a 24–44em
   * column whose header row has no room left (`ContestScoreChip`).
   */
  scoreChip?: boolean
  /**
   * CONTEST STRIP ONLY — the chip that says the station's mode earns no credit in the contest
   * running ("FT8 · no credit", FdAdvisories' `NoCreditChip`), drawn in this strip's header row
   * right after the mode chip. Only the large log line passes it (`ContestLargeEntry`); every
   * cockpit's strip is as it was.
   */
  noCreditChip?: React.ReactNode
  /**
   * CONTEST STRIP ONLY — the Contest screen's operator box, drawn in this strip's header row right
   * beside Sending and I moved (ContestView's `OperatorBox`). Only the large log line passes it
   * (`ContestLargeEntry`: the top of the main window's Contest screen and of the logger window);
   * every cockpit's strip is as it was. It sits above the boxes, so the contact's Tab order (Call,
   * the report, the exchange, Log) does not pass through it, and its Enter is its own.
   */
  operatorBox?: React.ReactNode
  /**
   * WHOSE CONTACT THIS IS, for the draft that outlives the strip (`features/logDraft`): a host
   * that unmounts this strip when the operator goes to another page names its cockpit here, and
   * the strip comes back with the contact the operator left in it. Phone, CW and Satellites pass
   * it: App's view switch draws them, so a page switch unmounts them. Absent, nothing is kept: the
   * RTTY, PSK and JS8 strips never unmount (their hosts keep them), the Remote page keeps its own
   * forms mounted, and the Contest screen's entry is a contest strip, which keeps none anyway
   * (the draft effect says why).
   */
  draft?: string
  /**
   * CW AND PHONE ONLY — Contest mode is on but the station is out of the contest (the re-entry on
   * opening did not happen, or the session dropped while the screen was open), so this line is the
   * everyday form, with no exchange boxes and no dupe check. Given, one line says so, with Resume: the
   * host re-enters the contest in S&P (never a CQ). App passes it only while that is so.
   */
  onResumeContest?: () => void
  /**
   * The Phone, CW and digital cockpits: a BAND CHANGE (`snap.bandChangeTick`, any route) sets an
   * un-logged line aside, every box of it, because the station is not on the new band (operator
   * ruling, 2026-10-26). A notice with Restore puts it back, to log on the band it was made on. A
   * contact being logged is never touched, nor a line for another radio (the override); one open
   * for edit is set aside as the edit closes. Absent (Satellites): nothing clears.
   */
  clearCallOnBandChange?: boolean
}

/**
 * Shared rich log strip for the CW + Phone cockpits.
 *
 * When `fieldDay` is active (non-null) the strip shows Class + Section inputs
 * and routes the log through contestLogManual() — a dupe rejection shows the error
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
  logOffsetHz,
  startedAt,
  onCallChange,
  onEntryCall,
  onReset,
  logRequest,
  onLogRequest,
  onSnap,
  fieldDay,
  fdMode,
  fdSubmode,
  fillExchange,
  titled = true,
  active = true,
  remote,
  esm,
  sharedEntry,
  scoreChip = true,
  noCreditChip,
  operatorBox,
  draft,
  onResumeContest,
  clearCallOnBandChange = false,
}: Props) {
  const remoteMode = remote != null
  type DraftContext = { band: string; freqMhz: number; mode: string }
  const [remoteDraftContext, setRemoteDraftContext] = useState<DraftContext | null>(null)
  const [remoteWorkOffer, setRemoteWorkOffer] = useState<({ call: string; ts: number } & { context: DraftContext }) | null>(null)
  // The frequency a contact logged now is on: the dial, plus the host's audio offset where it logs
  // one (`logOffsetHz`), to the hertz. A Remote draft captures the same frequency.
  const liveFreqMhz =
    logOffsetHz == null ? snap.radio.dialMhz : Math.round(snap.radio.dialMhz * 1e6 + logOffsetHz) / 1e6
  const currentDraftContext = (): DraftContext => ({ band: snap.radio.band, freqMhz: liveFreqMhz, mode })
  const rememberRemoteContext = () => {
    if (remoteMode) setRemoteDraftContext(current => current ?? currentDraftContext())
  }
  const fdActive = fieldDay != null
  // Enter Sends Message runs here: the cockpit's switch, a contest strip, and never from afar.
  const esmLive = !!esm?.on && fdActive && !remoteMode
  // Does this cockpit's exchange carry a park/summit reference? See `exchange`.
  const asksForPark = exchange === 'terrestrial'
  // …and a grid square? A bird's exchange is grid-for-grid, so the field is asked
  // for there. NOT in Phone/CW — see `exchange` for why that is an open question
  // and not a settled no. The FD layout renders neither field.
  const asksForGrid = exchange === 'satellite' && !fdActive

  // The contact this strip's cockpit left when the operator last went to another page (`draft`),
  // read once as the strip mounts, and never into a contest strip: the draft effect says why.
  const [kept] = useState(() => (draft && !fdActive ? readLogDraft(draft) : undefined))
  const [logCall, setLogCall] = useState(kept?.call ?? '')
  // When the call in the strip was entered — typed or clicked in — which is the contact's start
  // (TIME_ON): the rule JS8's host already gives the strip (`startedAt`), for every cockpit. The
  // first keystroke is the start, so a call typed or corrected a key at a time keeps it; another
  // call is another contact, and a cleared strip has none. A draft brings it back with the call.
  const callEnteredRef = useRef<{ call: string; unix: number } | null>(
    kept?.call.trim() && kept.enteredUnix != null
      ? { call: kept.call.trim().toUpperCase(), unix: kept.enteredUnix }
      : null,
  )
  useEffect(() => {
    const c = logCall.trim().toUpperCase()
    const prev = callEnteredRef.current
    if (!c) callEnteredRef.current = null
    else if (!prev || !(c.startsWith(prev.call) || prev.call.startsWith(c))) {
      callEnteredRef.current = { call: c, unix: Math.floor(Date.now() / 1000) }
    } else if (prev.call !== c) callEnteredRef.current = { call: c, unix: prev.unix }
  }, [logCall])
  // The two reports as set for THIS contact (typed, or the CW decoder's read of theirs), or null for
  // a box nobody has touched, which shows the report of the mode the strip logs under NOW, as a
  // contest strip's report box does (`fdValue`). One strip lives across a mode change (the Satellites
  // strip follows the station's), and a report held from mount kept an FM pass's 59 on a CW contact.
  const [logRstSent, setLogRstSent] = useState<string | null>(kept?.rstSent ?? null)
  const [logRstRcvd, setLogRstRcvd] = useState<string | null>(kept?.rstRcvd ?? null)
  const rstSentShown = logRstSent ?? defaultRst
  const rstRcvdShown = logRstRcvd ?? defaultRst
  const [logName, setLogName] = useState(kept?.name ?? '')
  const [logQth, setLogQth] = useState(kept?.qth ?? '')
  const [logComment, setLogComment] = useState(kept?.comment ?? '')
  const [logNotes, setLogNotes] = useState(kept?.notes ?? '')
  // Autofill stash (written to the record, not separately edited): grid/state/country.
  const [logGrid, setLogGrid] = useState(kept?.grid ?? '')
  const [logState, setLogState] = useState(kept?.state ?? '')
  const [logCountry, setLogCountry] = useState(kept?.country ?? '')
  // The STATE box as it stands, and where its value came from: the operator (who also owns the
  // untouched empty box), a callbook lookup, or the park or summit in the park box. Every write
  // goes through `setStateBox`, so both are current before React renders, as `parkBoxRef` is for
  // the park box. The station reads the source when the contact is logged: a callbook's state is
  // the licensee's ADDRESS, so a park's own state outranks it, and the operator's outranks both.
  const stateBoxRef = useRef(kept?.state ?? '')
  const stateSourceRef = useRef<StateSource>(kept?.stateSource ?? 'operator')
  const [stateSource, setStateSource] = useState<StateSource>(kept?.stateSource ?? 'operator')
  const setStateBox = (value: string, source: StateSource) => {
    stateBoxRef.current = value
    stateSourceRef.current = source
    setStateSource(source)
    setLogState(value)
  }
  // The callbook's own state for this call, kept apart from the box: the recall card pairs a state
  // with the callbook's town ("Columbus, OH"), and once the park has placed the box its state is
  // not where that town is.
  const [callbookState, setCallbookState] = useState(kept?.callbookState ?? '')
  // Callbook profile photo (display-only, not written to the log). Cleared when the call changes.
  const [logImage, setLogImage] = useState<string | null>(kept?.image ?? null)
  // POTA/SOTA park of the station worked (ota.their_*). Prefilled from a hunted spot; editable.
  const [logParkProgram, setLogParkProgram] = useState(kept?.parkProgram ?? 'POTA')
  const [logParkRef, setLogParkRef] = useState(kept?.parkRef ?? '')
  // WHOSE park the box holds, and whether a hunt put it there. A prefill is bound to the hunted
  // activator (`hunt: true`); a park the operator typed or picked is bound to the call in the box
  // (`hunt: false`, and `call: ''` when there was no call yet), and follows that call while the
  // operator types it, so a park typed before any call belongs to the first call TYPED after it
  // (#383). Written wherever a park goes INTO the box and read only while one is there, so a clear
  // leaves it alone. See the prefill below for why the value alone cannot say.
  const parkForRef = useRef(kept?.parkFor ?? { call: '', hunt: false })
  // THE PARK A NEEDED CLICK HANDED OVER (`pendingWork.park`). The Needed board fills the strip with
  // the station's call and the park it is activating, and sets no hunt: nothing waits in the
  // engine, so nothing shows at the top of the POTA/SOTA view and the strip's Clear really clears
  // it. For its own station it stands in for the hunt in the
  // prefill below, so it fills, and leaves with its station, exactly as a hunted park does, and a
  // hunt pending for the same station at another park does not replace it. It is spent the way a
  // hunt is, by the contact it is logged with (`reset`).
  const [workPark, setWorkPark] = useState<{ call: string; program: string; reference: string } | null>(
    kept?.workPark ?? null,
  )
  // The park box as it stands, for the park prefill: the LAST value written to it. Every write
  // goes through `setPark`, so this always has it, even before React renders it. The call-change
  // effect runs just before the prefill and can empty the box in the same commit, while the
  // rendered `logParkRef` still holds the value being erased. And it is NOT re-synced from a render
  // (it was): a render can land ahead of a write still pending from a passive effect, and syncing
  // from it put the box back to what that render saw. After a hunted log, the spent hunt's refill
  // was such a write; the hunt's end rendered first, the prefill judged an empty box, and the
  // refill then landed with nothing left to take it out (#383).
  const parkBoxRef = useRef(kept?.parkRef ?? '')
  const setPark = (value: string) => {
    parkBoxRef.current = value
    setLogParkRef(value)
  }
  // Local park-directory suggestions (POTA only) as the operator types the reference.
  const [parkHits, setParkHits] = useState<Park[]>([])
  // A park the draft put back counts as picked, as a hunt's prefill does: the strip comes back as it
  // was left, not with a suggestion list open under the box.
  const [parkPicked, setParkPicked] = useState(!!kept?.parkRef)
  // Auto-loaded details for a complete park reference (name/location/grid), local-first then live.
  const [parkDetail, setParkDetail] = useState<Park | null>(null)
  const [parkDetailLive, setParkDetailLive] = useState(false)
  const [qrzBusy, setQrzBusy] = useState(false)
  // The general logbook behind the B4/dupe badges and the recall card: asked of `LogSource`
  // (features/logSource) below, following the engine's logTick. Not read in FD mode — the
  // contest log does the dupe checking there — and never in remote mode.
  const readsLog = !fdActive && !remoteMode
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
  /** The exchange boxes BY SLOT ID, so space can walk Call → field₁ → … → Call.
   *
   *  One map rather than one ref per slot, because the box list is the session's
   *  `receives` and a hook cannot be called per element of a list that changes. */
  const fdBoxRefs = useRef<Record<string, HTMLInputElement | null>>({})

  // THE ENTRY STRIP'S SHAPE — the session's received slots, in receive order.
  // Field Day receives CLASS then SECTION, so this is the two boxes the strip has
  // always had. `FD_RECEIVES_FALLBACK` is what a snapshot from a build older than
  // this DTO field produces, and it is the same two.
  const fdReceives = fieldDay?.receives?.length ? fieldDay.receives : FD_RECEIVES_FALLBACK

  // FD-specific: the received exchange being typed, by slot id. Defaults from the
  // last entry / fieldDay status, and persists across log-and-clear so a run does
  // not re-type them.
  const [fdFields, setFdFields] = useState<Record<string, string>>(() => ({
    // A first guess only: most Field Day contacts are the same class as ours, and the
    // operator overtypes it. The SESSION's composing value is the right source for a
    // guess about the next contact; it is never a record of one.
    CLASS: composingSlot(fieldDay?.composing, 'CLASS'),
  }))
  const setFdField = (key: string, v: string) =>
    setFdFields((prev) => ({ ...prev, [key]: v.toUpperCase() }))
  /** The boxes the operator typed in (or picked, or grabbed into) since the strip last cleared —
   *  the fill state's `typed`, which travels with the shared entry, rendered here. A report box
   *  not in it shows the mode's own report (`fdValue`). */
  const [typedMarks, setTypedMarks] = useState<FillState['typed']>({})
  // ⭐ THE EXCHANGE TYPE-AHEAD — which box is open and what it is offering. One entry,
  // not one per slot: only the box being typed in shows a list, and a stale list under
  // another box would be a menu of the wrong universe.
  const [fdHits, setFdHits] = useState<{ key: string; values: DomainValue[] }>({
    key: '',
    values: [],
  })
  const closeFdHits = () => setFdHits((h) => (h.key === '' ? h : { key: '', values: [] }))
  // A box's blur closes the list a moment later (below). That timer is the strip's: cleared on
  // unmount, so it never fires on a strip that has gone.
  const fdHitsCloseTimer = useRef<number | undefined>(undefined)
  useEffect(() => () => window.clearTimeout(fdHitsCloseTimer.current), [])
  /** ⭐ The county list a box takes a COUNTY LINE from (`COOK/DUPG`, one contact per county),
   *  or `undefined` for a box that takes one value per contact — which is every box but a
   *  county box whose sponsor counts a county-line contact once per county.
   *
   *  ⚠️ Never on the satellite strip. A pass logs through `contestLogSatellite`, which takes
   *  its band off the bird and writes one row; the county line's one-call, many-row write is
   *  the terrestrial command's. That strip keeps one county per contact, exactly as before. */
  const fdLineDomain = (f: ContestFieldSpec): string | undefined =>
    exchange === 'satellite' ? undefined : countyLineDomain(f.domains)
  /** Commit a box: a typed NAME becomes the code that goes on the air, and the list
   *  closes. Called where the operator LEAVES the box (space, tab, blur) rather than on
   *  every keystroke — resolving "CO" to Coles while somebody is still typing "COOK"
   *  would fight the fingers. */
  const commitFdField = (f: ContestFieldSpec) => {
    const raw = fdValue(f)
    const code = fdSlotValue(f, raw, fdLineDomain(f))
    if (code !== raw.trim().toUpperCase()) setFdField(f.key, code)
    closeFdHits()
  }
  // What a box holds: what was typed or filled, else — for a signal report nobody has
  // touched — the default report. A box is empty only because somebody emptied it, so the
  // report is there from the first contact however late the session's slots arrive.
  //
  // ⭐ A REPORT IS THE OPERATOR'S ONLY WHILE THEY HAVE TYPED IT FOR THIS CONTACT (`typedMarks`).
  // Otherwise it is the report of the mode this strip logs under NOW — 59 on phone, 599 on CW
  // and the keyboard modes — whatever the box map holds, because a value there may have been
  // written under another mode: one strip lives across a mode change (the logger window follows
  // the station's section, the Satellites strip the station's mode), and the shared entry hands
  // one window's boxes to another cockpit's strip. The map used to hold the reset report itself
  // from the first logged contact on, so a move from Phone to CW kept offering 59, and a CW
  // contact went into the log with it.
  const rstDefault = defaultContestRst(fdMode)
  const fdValue = (f: ContestFieldSpec): string => {
    if (f.kind !== 'rst') return fdFields[f.key] ?? ''
    return typedMarks[f.key] ? (fdFields[f.key] ?? rstDefault) : rstDefault
  }
  // ⭐ THE OPTIONAL "RST SENT" BOX (N1MM's Snt; Settings ▸ Contesting, off by default). The engine
  // says it is on by sending the report this contest sends (`rstSentBox`, `599`), and only for a
  // contest that sends one. It shows what this contact will log as sent — 59 on phone, the
  // session's own on CW and digital, exactly what the engine stamps — and, like the received
  // report, a value typed in it is the operator's only while it is marked typed for THIS contact:
  // the next contact goes back to the constant. Never on the Remote, which logs no contest here.
  const sentRstConst = fdActive && !remoteMode ? fieldDay?.rstSentBox : undefined
  const sentRstDefault = (fdMode ?? 'PH') === 'PH' ? '59' : (sentRstConst ?? '599')
  const sentRstTyped = sentRstConst !== undefined && !!typedMarks[SENT_RST_KEY]
  const sentRstValue = sentRstTyped ? (fdFields[SENT_RST_KEY] ?? sentRstDefault) : sentRstDefault
  const sentRstGood = !sentRstTyped || sentRstOk(sentRstValue, fdMode)
  const sentRstRef = useRef<HTMLInputElement | null>(null)

  // Pre-fill the exchange from the last logged FD contact — but ONLY when the
  // log actually GREW. `fieldDay` is a fresh object every 300 ms snapshot poll;
  // keying the effect on it overwrote whatever the operator was TYPING with the
  // last contact's exchange (the digital sequencer logging in the background
  // made the fields jump mid-keystroke).
  //
  // ⚠️ The per-row RECEIVED VECTOR does not reach the UI yet — a DTO row carries
  // `class`/`section`, which is what this reads through `fdRcvd`. That carries Field
  // Day's two slots and nothing else, so every other slot starts the next contact
  // EMPTY — the next station's zone is not the last one's — except a signal report,
  // which goes back to untouched, the mode's own: it read blank here, and every contest
  // that sends one needed it retyped on every contact. Untouched, not WRITTEN: a report
  // written here is the mode this contact was logged in, and it stayed in the box after a
  // move to another one.
  //
  // ⭐ ONCE PER CONTACT. A contact this strip logged was cleared for as its log landed
  // (`logIt` moves `fdSeenLen` on past its rows), so the snapshot that first shows it clears
  // nothing more here. A strip that did not log it clears here, once.
  const fdLogLen = fieldDay?.log?.length ?? 0
  const fdSeenLen = useRef(fdLogLen)
  useEffect(() => {
    if (fdActive && fieldDay && fdLogLen > fdSeenLen.current) {
      const lastEntry = fieldDay.log?.[fdLogLen - 1]
      if (lastEntry) {
        // Every box is rewritten, so no box is a call-history fill or the operator's any more.
        setFill(EMPTY_FILL)
        setFdFields((prev) => afterLogged(prev, fdReceives, lastEntry))
      }
    }
    fdSeenLen.current = fdLogLen
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [fdActive, fdLogLen])

  // ⭐ REMOVE THE NEWEST CONTACT — Ctrl+D or Remove last, PRESSED TWICE. The first
  // press names the contact on the verdict line and changes nothing; a second within
  // REMOVE_CONFIRM_MS removes it. Any other key, the window running out, or a new contact lets it
  // lapse, so nothing is removed that the operator was not just shown — and if the newest contact
  // changed between the presses, nothing is removed and the line says so. The engine checks the
  // same identity again (`changed`), so a snapshot that lagged cannot remove the wrong contact.
  //
  // ⛔ IT NEVER MOVES THE CARET. The Space-PTT incident was a focus change in this entry code: a
  // keyup aimed at an input was swallowed and the rig stayed keyed. The button cancels its
  // mousedown so focus stays where it was, and nothing here calls focus(). Nor does it key or stop
  // a transmission: Esc still stops TX, and this reaches the contest log alone.
  //
  // Not on the hosted Remote page or in the native client (`remoteMode`): removal is refused
  // from afar, and Remote's operations carry no contest-log command.
  const removable = fdActive && !remoteMode
  const newest = newestContact(fieldDay?.log)
  const newestKey = newest ? `${newest.call}|${newest.whenUnix}` : ''
  const [removeArmed, setRemoveArmed] = useState<
    { call: string; whenUnix: number; label: string; at: number } | null
  >(null)
  const [removeNote, setRemoveNote] = useState<{ text: string; alert: boolean } | null>(null)
  // Enter Sends Message's one line in the same slot (below): why a press sent nothing, Phone's
  // prompt, or what a stop left. `about` is the contact a stopped note names, so a new contact
  // that is not that one retires it. `esmLineKey` is the key press that wrote it: every OTHER
  // key lets it lapse, as every other key lets a removal lapse.
  const [esmLine, setEsmLine] = useState<{ text: string; alert: boolean; about?: string } | null>(null)
  const esmLineKey = useRef<Event | null>(null)
  // The line Alt+W wiped (`pressWipe`, below), kept until any other key: what a second Alt+W puts back.
  const wipedRef = useRef<{ call: string; fields: Record<string, string>; fill: FillState } | null>(null)
  const pressWipeRef = useRef<() => void>(() => {})
  // The call Alt+M marks with (`features/freqMarkers.ts`).
  const markCallRef = useRef(logCall)
  markCallRef.current = logCall
  const armedRef = useRef(removeArmed)
  armedRef.current = removeArmed
  const pressRemove = () => {
    // Only at the station: a press or a Ctrl+D that comes through the Remote stream removes
    // nothing, as removal is refused from afar. Read before anything awaits, while the stream's
    // dispatch is still the one running.
    if (isStreamInput()) {
      setRemoveArmed(null)
      setRemoveNote({ text: t('logEntry.remove.remote'), alert: true })
      return
    }
    const armed = armedRef.current
    const now = Date.now()
    if (!newest) {
      setRemoveArmed(null)
      setRemoveNote({ text: t('logEntry.remove.empty'), alert: false })
      return
    }
    if (!armed || now - armed.at > REMOVE_CONFIRM_MS) {
      setRemoveNote(null)
      setRemoveArmed({ call: newest.call, whenUnix: newest.whenUnix, label: contactLabel(newest.rows), at: now })
      return
    }
    setRemoveArmed(null)
    if (armed.call !== newest.call || armed.whenUnix !== newest.whenUnix) {
      setRemoveNote({ text: t('logEntry.remove.changed'), alert: true })
      return
    }
    contestRemoveLast(armed.call, armed.whenUnix).then(
      (answer) => setRemoveNote({ text: removalText(answer, armed.label), alert: answer.outcome === 'refused' }),
      () => setRemoveNote({ text: t('logEntry.remove.failed'), alert: true }),
    )
  }
  const pressRemoveRef = useRef(pressRemove)
  pressRemoveRef.current = pressRemove
  // ⭐ CTRL+Y OPENS THE NEWEST CONTACT FOR EDIT, in place of the entry boxes: its call,
  // what they sent, its band and its mode (`ContestContactEdit`). Enter saves, Esc leaves it as it
  // was, and the entry boxes come back as they were, with the caret in Call. The contact keeps its
  // time, serial and number. Never through the Remote stream; and at a club the line says how a
  // contact is fixed there today, because club sync cannot carry an edit to the other laptops.
  // Left open in a cockpit that is then hidden (RTTY, PSK, JTTY and JS8 stay mounted), it stays as
  // typed for when the cockpit is back, and holds back no other edit meanwhile (`focusOpenContestEdit`).
  const [editing, setEditing] = useState<{ call: string; whenUnix: number } | null>(null)
  const rowsOf = (call: string, whenUnix: number | undefined) =>
    fieldDay ? fieldDay.log.filter((q) => q.call === call && q.whenUnix === whenUnix) : []
  const editRows = editing ? rowsOf(editing.call, editing.whenUnix) : []
  // A contact this window no longer finds while its edit is open — its call changed elsewhere, the
  // commonest edit, or it was removed: the edit closes and says which (`closedText`). This edit's
  // own save changing the call answers for itself. One edited elsewhere with its call kept closes
  // itself and says so (`ContestContactEdit`).
  const saving = useRef(false)
  useEffect(() => {
    if (!editing || editRows.length > 0) return
    setEditing(null)
    if (!saving.current) setRemoveNote({ text: closedText(fieldDay?.log ?? [], editing.whenUnix), alert: true })
  }, [editing, editRows.length, fieldDay?.log])
  const pressEdit = () => {
    setRemoveArmed(null)
    if (isStreamInput()) {
      setRemoveNote({ text: t('contest.edit.remote'), alert: true })
      return
    }
    if (!newest) {
      setRemoveNote({ text: t('contest.edit.empty'), alert: false })
      return
    }
    if (fieldDay?.club) {
      setRemoveNote({ text: t('contest.edit.club'), alert: true })
      return
    }
    // Another edit open in this window (a last contact's, the contest screen's): it keeps the
    // keyboard and its typing, and no second one opens. With this strip's own open, Ctrl+Y opens
    // the newest afresh, as it always has.
    if (editRows.length === 0 && focusOpenContestEdit()) {
      setRemoveNote({ text: t('contest.edit.open'), alert: false })
      return
    }
    setRemoveNote(null)
    saving.current = false
    setEditing({ call: newest.call, whenUnix: newest.whenUnix })
  }
  const pressEditRef = useRef(pressEdit)
  pressEditRef.current = pressEdit
  const editDone = (note: EditNote | null) => {
    saving.current = false
    setEditing(null)
    if (note) setRemoveNote(note)
    requestAnimationFrame(() => callInputRef.current?.focus({ preventScroll: true }))
  }
  // The press window, from the moment of the first press.
  useEffect(() => {
    if (!removeArmed) return
    const id = window.setTimeout(() => setRemoveArmed(null), Math.max(0, removeArmed.at + REMOVE_CONFIRM_MS - Date.now()))
    return () => window.clearTimeout(id)
  }, [removeArmed])
  // A new contact (or the newest gone some other way) while a press waits: it lapses, and the
  // line says why the next press starts over. A new contact also retires the last answer.
  const seenNewest = useRef({ key: newestKey, len: fdLogLen })
  useEffect(() => {
    const was = seenNewest.current
    seenNewest.current = { key: newestKey, len: fdLogLen }
    if (was.key === newestKey && was.len === fdLogLen) return
    if (armedRef.current) {
      setRemoveArmed(null)
      setRemoveNote({ text: t('logEntry.remove.changed'), alert: true })
    } else if (fdLogLen > was.len) {
      setRemoveNote(null)
    }
    // …and an Enter Sends Message line, unless it is the stopped note about that very contact.
    const call = newest?.call.toUpperCase()
    setEsmLine((l) => (l && l.about !== undefined && l.about === call ? l : null))
  }, [newestKey, fdLogLen])
  // Ctrl+D, Alt+W and Ctrl+O from anywhere in the cockpit showing this strip — only while its host
  // is on screen, since the RTTY, PSK and JS8 strips stay mounted when hidden.
  useEffect(() => {
    if (!removable || !active) return
    const onKey = (e: KeyboardEvent) => {
      // Every key but Alt+W itself (a bare modifier is not a key) ends the chance to put back a line
      // Alt+W wiped: it comes back only if nothing new has been typed in since.
      if (!isModifierKey(e) && !isWipeKey(e)) wipedRef.current = null
      if (isRemoveKey(e)) {
        // This strip's key while it shows: the browser's own (a bookmark, or delete-next-character
        // in a macOS text box) never runs.
        e.preventDefault()
        // Held down, it is still ONE press: auto-repeat can never be the second.
        if (!e.repeat) pressRemoveRef.current()
        setEsmLine(null)
        return
      }
      if (isEditKey(e)) {
        // Ctrl+Y is redo in a text box; while this strip shows it edits the last contact.
        e.preventDefault()
        if (!e.repeat) pressEditRef.current()
        setEsmLine(null)
        return
      }
      if (isModifierKey(e)) return
      // Any other key lets a waiting press lapse, and retires the last answer — and an Enter
      // Sends Message line, except at the press that wrote it.
      setRemoveArmed(null)
      setRemoveNote(null)
      if (e !== esmLineKey.current) setEsmLine(null)
      // The strip's own keys while it shows (`features/contestKeys.ts`), held from the webview: a
      // Mac's Option+W would type ∑ into the box, and Ctrl+O is a browser's Open. Held down, each is
      // one press.
      if (isWipeKey(e)) {
        e.preventDefault()
        // Pressed in a contact's edit, it is that edit's moment, not the entry line's.
        if (e.target instanceof Element && e.target.closest('.contest-edit')) return
        if (!e.repeat) pressWipeRef.current()
      } else if (isOperatorBoxKey(e)) {
        e.preventDefault()
        if (!e.repeat) askForOperatorBox()
      } else if (isMarkKey(e)) {
        // N1MM's Mark: the dial, on the band map, with the call in this line (`features/freqMarkers.ts`).
        e.preventDefault()
        if (!e.repeat) void markHere(markCallRef.current)
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [removable, active])
  // The one the band map's right-click reads (another window, so a shared key): this strip's call,
  // while it is on screen.
  useEffect(() => {
    if (removable && active) publishEntryCall(logCall)
  }, [removable, active, logCall])
  // ONE line in the verdict slot while a press waits or an answer shows: the strip does not grow.
  const removeLine = removeArmed
    ? t('logEntry.remove.armed', { contact: removeArmed.label })
    : editRows.length > 0
      ? t('contest.edit.editing', { contact: contactLabel(editRows) })
      : removeNote?.text
  // …and the same for an Enter Sends Message line, behind a removal's.
  const slotFree = removeLine == null && esmLine == null

  // THE FILL API (`fillExchange`): one box, refilled on every new `ts`. Keyed on the
  // stamp alone so a snapshot re-render never refills, and a slot this session does not
  // receive is dropped rather than added — a box nobody renders would still ride the
  // wire. Focus is deliberately untouched.
  useEffect(() => {
    if (!fdActive || !fillExchange) return
    if (!fdReceives.some((f) => f.key === fillExchange.key)) return
    // The operator's own act (a double-click on what they copied), so call history never
    // writes over it.
    noteTyped(fillExchange.key)
    setFdField(fillExchange.key, fillExchange.value)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [fillExchange?.ts])

  // ⭐ SUPER CHECK PARTIAL AND CALL HISTORY are on where the ENGINE says they are: its effective
  // assistance sources (`assistanceOn`) fold Unassisted mode in, so declaring an unassisted entry
  // takes both off this strip at the next snapshot, as it does the cluster and the AI CW decoder.
  // Never on Remote: the hosted page gets neither.
  const assistOn = fieldDay?.assistanceOn ?? []
  const scpOn = fdActive && !remoteMode && assistOn.includes(SCP_SOURCE)
  const historyOn = fdActive && !remoteMode && assistOn.includes(CALL_HISTORY_SOURCE)
  const scpCalls = useScpList(scpOn)
  const historyFile = useCallHistory(historyOn)
  // What the call-history fill has done since the strip last cleared. The ref is the truth, read
  // by the fill effect and the key handlers in the same tick; the state renders the marks.
  const fillRef = useRef<FillState>(EMPTY_FILL)
  const [fillMarks, setFillMarks] = useState<FillState['filled']>({})
  const setFill = (next: FillState) => {
    if (next === fillRef.current) return
    fillRef.current = next
    setFillMarks(next.filled)
    setTypedMarks(next.typed)
  }
  /** The operator typed in a box (or picked from its list, or grabbed into it): it is theirs. */
  const noteTyped = (key: string) => setFill(typedIn(fillRef.current, key))
  /** The strip is clearing. A logged contact keeps what it logged; an abandoned one gives its
   *  call-history values back, so one station's last exchange never stays in a box, unmarked,
   *  for the next. */
  const endFill = (logged: boolean) => {
    if (fillRef.current === EMPTY_FILL) return
    if (!logged) {
      const values = Object.fromEntries(fdReceives.map((f) => [f.key, fdValue(f)]))
      const { writes } = applyFill(fillRef.current, '', {}, values)
      if (Object.keys(writes).length > 0) setFdFields((prev) => ({ ...prev, ...writes }))
    }
    setFill(EMPTY_FILL)
  }
  // ⭐ THE WHOLE LOG'S ANSWER for the call typed (the third source below): asked of the station,
  // which reads that one call's contacts through the log's per-call index, a moment after the
  // call stops changing. Never a scan of a log this strip holds, and never on the Remote.
  const [loggedQth, setLoggedQth] = useState<{ call: string; value: string; whenUnix: number } | null>(null)
  const hasQthBox = fdReceives.some((f) => f.key === 'QTH')
  useEffect(() => {
    const c = logCall.trim().toUpperCase()
    if (!fdActive || remoteMode || !hasQthBox || c.length < 3) return
    let live = true
    const timer = window.setTimeout(() => {
      // Through a promise from the start, so a station that cannot answer (an older one, or none)
      // is the same quiet "nothing" as a call the log does not hold.
      Promise.resolve()
        .then(() => contestLoggedQth(c, 'QTH'))
        .then((r) => {
          if (live) setLoggedQth(r && typeof r.value === 'string' ? { call: c, value: r.value, whenUnix: r.whenUnix } : null)
        })
        .catch(() => {})
    }, 200)
    return () => {
      live = false
      window.clearTimeout(timer)
    }
  }, [fdActive, remoteMode, hasQthBox, logCall])

  // ⭐ THE FILL, AS THE CALL IS TYPED (operator, 2026-10-08). Every change of the call (typed, a
  // click on the SCP line, the decoder's or the host's) brings the boxes in line with what is
  // known of exactly that call, from three sources in the operator's order (2026-10-18): this
  // contest's last contact with the station, then the call-history file, then the most recent
  // contact anywhere in the log. `features/callHistoryFill` decides what may go in, never over a
  // box the operator typed in, and takes a fill back out when the call stops being the one it
  // was for. A box that has the caret when its fill lands is selected, so typing replaces it, as
  // Space and Tab select a box they move to.
  const fdReceivesKey = fdReceives.map((f) => f.key).join(' ')
  useEffect(() => {
    if (!fdActive) return
    // ⚠️ A STATION ALREADY WORKED ON THIS BAND AND MODE takes no fill from this contest or the
    // log: what it sends NOW decides whether this is a dupe or a rover's new county, so the box
    // waits for the operator, with the counties already logged on the amber line above it. Filled
    // with the county logged last, it would turn the contact into a dupe nobody copied.
    const modeClass = fdMode ?? 'PH'
    const composing = (slot: string) => composingSlot(fieldDay?.composing, slot)
    const workedHere =
      contestDupe(fieldDay, logCall, snap.radio.band, modeClass) === 'own' ||
      contestWorkedRows(fieldDay, logCall, snap.radio.band, modeClass, { rx: () => '', tx: composing }).length > 0
    const { offered, sources } = offerFill(
      workedHere ? {} : contestLogFill(logCall, fdReceives, fieldDay?.log ?? []),
      historyFill(logCall, fdReceives, historyFile, fieldDay?.event),
      workedHere ? {} : loggedQthFill(logCall, fdReceives, loggedQth),
      loggedQth ? new Date(loggedQth.whenUnix * 1000).getUTCFullYear() : 0,
    )
    const values = Object.fromEntries(fdReceives.map((f) => [f.key, fdValue(f)]))
    const r = applyFill(fillRef.current, logCall, offered, values, sources)
    if (r.state === fillRef.current) return
    setFill(r.state)
    if (Object.keys(r.writes).length === 0) return
    setFdFields((prev) => ({ ...prev, ...r.writes }))
    requestAnimationFrame(() => {
      for (const key of Object.keys(r.writes)) {
        const box = fdBoxRefs.current[key]
        if (box && box === document.activeElement && r.state.filled[key]) box.select()
      }
    })
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [fdActive, logCall, historyFile, fieldDay?.event, fdReceivesKey, fdLogLen, loggedQth, snap.radio.band, fdMode])

  // "I moved" (§4.1): the read-only sent exchange becomes editable, one box per
  // composing slot, and takes effect on the NEXT contact.
  const [movingTo, setMovingTo] = useState<Record<string, string> | null>(null)

  // ⭐ ONE CONTACT, TWO WINDOWS (`features/contestEntryShare`). While the contest logger window is
  // open (`snap.contestEntry`), this strip and the logger's show one entry: the call, the boxes,
  // which boxes hold a call-history fill, and the take-back line. What is typed here is put to the
  // engine and the other window takes it up at its next snapshot, so every hint — the dupe
  // verdict, Super Check Partial, call history, Ctrl+D — follows the one contact in both. Only a
  // strip on screen takes part (`active`), and never on the Remote. With the window closed the
  // snapshot carries no entry and this strip is its own, exactly as before.
  const shareContent: StripContent = {
    call: logCall,
    fields: fdFields,
    fill: fillRef.current,
    take: { armed: removeArmed, note: removeNote },
  }
  const share = useSharedEntry({
    entry: snap.contestEntry,
    on: fdActive && !remoteMode && active,
    seeds: sharedEntry !== 'logger',
    content: shareContent,
    adopt: (c) => {
      setLogCall(c.call)
      setFdFields(c.fields)
      fillRef.current = c.fill
      setFillMarks(c.fill.filled)
      setTypedMarks(c.fill.typed)
      setRemoveArmed(c.take.armed)
      setRemoveNote(c.take.note)
    },
    // The entry a log left still holds that contact's boxes: they end as the after-log clear
    // (above) ends this strip's own.
    afterLog: (fields) => afterLogged(fields, fdReceives, fieldDay?.log?.[fdLogLen - 1]),
  })
  /** The engine's two refusals of an Enter made on the shared entry, said in words, logging
   *  nothing: the other window logged the contact first, or changed it since this one showed it.
   *  Anything else is the log command's own failure and goes to its toast as before. */
  const sharedRefusal = async <T,>(logged: Promise<T>, call: string): Promise<T | null> => {
    try {
      return await logged
    } catch (err) {
      const why = typeof err === 'string' ? err : err instanceof Error ? err.message : ''
      if (why === ENTRY_LOGGED) {
        pushToast(t('logEntry.shared.logged', { call }), 'info')
        return null
      }
      if (why === ENTRY_CHANGED) {
        pushToast(t('logEntry.shared.changed'), 'error')
        return null
      }
      throw err
    }
  }


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
   *
   * A call a draft put back (`kept`) starts settled: the draft effect keeps no call but the
   * operator's, so a callbook answer it never got is still asked for.
   */
  const settledCallRef = useRef(kept != null)

  // The call whose identity (name/QTH/grid/state/country) the enrichment fields currently hold.
  // Clear them when the call changes to a DIFFERENT one, so a previous callsign's data never
  // bleeds onto another call's recall card — and so onCallBlur re-looks-up the new call.
  const enrichedForRef = useRef(kept?.enrichedFor ?? '')
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
      setStateBox('', 'operator')
      setCallbookState('')
      setLogCountry('')
      setLogImage(null)
      setPark('') // the park was for the previous call; the prefill below decides in this commit
      // The wiped name may have been the CW decoder's copy — un-latch so it can refill for the
      // new call (declared below; the effect callback runs after render, so it's initialized).
      cwNameFilled.current = false
    }
  }, [logCall])

  // The grid this form holds for its call, typed or filled in from the callbook, handed to the
  // engine so pointing the rotator at this call aims at the station rather than the centre of its
  // country (a tester was sent 20° wide by the country centre). Only a real square, and a clear
  // only for a call it was given a square for, so typing a call costs nothing. A browser's form
  // tells the station nothing: the station resolves its own bearing.
  const toldGridRef = useRef<{ call: string; grid: string } | null>(null)
  useEffect(() => {
    if (remoteMode) return
    const call = logCall.trim().toUpperCase()
    const typed = logGrid.trim().toUpperCase()
    const grid = isValidLoggedGrid(typed) ? typed : ''
    const told = toldGridRef.current
    if (!call || (told?.call === call && told.grid === grid) || (!grid && told?.call !== call)) return
    toldGridRef.current = { call, grid }
    void setLogFormGrid(call, grid).catch(() => {})
  }, [logCall, logGrid, remoteMode])

  // Prefill the park field from a hunted spot (snap.hunt = the activator's program+ref). Keyed on
  // the reference string so it fires when a NEW park is hunted, not on every snapshot poll.
  //
  // Deliberately NOT gated on `asksForPark`. It latches in every exchange, and what stops a
  // latch reaching a satellite record is the ONE guard on `ota` at the commit — one seam,
  // one mechanism, mutable and therefore testable. A second guard here would make that one
  // untestable (remove it and the record still comes out clean, because the latch was never
  // taken) which is how a load-bearing fix quietly dies. The two effects below carry their
  // own guards for their own reason: they reach the NETWORK.
  //
  // ⚠️ THE MATCH IS THE ENGINE'S `same_call`, through `sameCall`. It was `.split('/').pop()`, which
  // made `KE7G/P` into `P`: a portable activator never matched the call logged for them, and any
  // two `/P` stations matched each other.
  //
  // ⚠️ A PARK SET FOR ANOTHER STATION IS NOT AN OVERRIDE. The operator clicks a second hunted spot
  // with the first activator's park still in the box. The new call lands one render after the
  // new hunt, and in that render the call-change effect above clears the box while this effect
  // reads it as rendered: still the first park, which it took for a park the operator had typed,
  // so it skipped the fill. The clear then emptied the box, and nothing re-ran this effect (the
  // operator's "why are some parks filled and not others": a hunt that lands a render after the
  // call finds the box already cleared, and fills). With no callbook answer nothing clears the box
  // at all, and the first park stayed, differing from the hunt, so it would have been logged as
  // one the operator typed. So an override is a park the operator TYPED for this call
  // (`parkForRef`), never merely a value that differs from the hunt. A park an earlier hunt put
  // there is never one: the same activator re-spotted at another park takes the new park, where the
  // earlier prefill used to hold the box and go out as a typed park. Nor is a park typed before any
  // call, for a call that arrives by a click: it belongs to the first call the operator types
  // (operator, 2026-09-29, #383), so a clicked spot for another park is a new station and fills
  // its own. It used to count for whatever call came next, and with no callbook the clicked
  // activator's contact went out at the typed park.
  //
  // ⚠️ THE BOX IS READ AS IT STANDS, NOT AS IT WAS RENDERED (`parkBoxRef`). The race above had a
  // second door. The call-change effect empties the box in the commit this effect decides in, so
  // a park it was erasing could still read as the operator's own: the fill was skipped and the box
  // ended empty under a chip naming the park. A park typed before any call opened it, once the
  // callbook had answered for the call typed after it.
  //
  // ⚠️ A HUNTED PARK LEAVES WITH ITS STATION. Once the call in the box is not the station a hunt
  // bound the park to, the park goes, whatever the pending hunt now is (another activator's, or
  // none) and whether or not a lookup ran. It used to go only when it equalled the CURRENT hunt,
  // so with the hunt moved on and no callbook, a hand-typed third call was logged at the first
  // activator's park. A park the operator typed never leaves this way: finishing a half-typed
  // call (`W1XY` → `W1XYZ`) keeps it. Its binding still matters, to the override above: a park
  // typed for W1XYZ does not stop the prefill when KE7G's spot is clicked next. With no call in
  // the box there is no station to hold it, and it leaves with its hunt. Logging the hunted
  // contact spends the hunt in the engine, but the snapshot here names it until the next poll, so
  // the reset strip was filled again; when the hunt's end arrived nothing took the park out, and
  // an empty strip showed the last contact's park.
  //
  // A park a Needed click handed over (`workPark`) is the hunt here, for its own station only.
  useEffect(() => {
    const call = logCall.trim()
    const h = workPark && sameCall(workPark.call, call) ? workPark : snap.hunt
    const shown = parkBoxRef.current.trim().toUpperCase()
    const huntRef = h?.reference?.trim().toUpperCase() ?? ''
    const bound = parkForRef.current
    if (h?.reference && (call === '' || sameCall(h.call, call))) {
      // Prefill (or keep) the hunted park, unless the box holds the operator's own park for this
      // call, so a manual override survives. `parkPicked` suppresses the search dropdown.
      const override = shown !== '' && shown !== huntRef && !bound.hunt && bound.call === baseCall(call)
      if (!override) {
        setParkPicked(true)
        setLogParkProgram(h.program)
        setPark(h.reference)
        parkForRef.current = { call: baseCall(h.call), hunt: true }
      }
    } else if (shown !== '' && (shown === huntRef || (bound.hunt && (call === '' || !sameCall(bound.call, call))))) {
      // No hunted station is in the box: drop the pending hunt's own prefill, a park an earlier hunt
      // bound to another station, and, with no call at all, one whose hunt is over, rather than
      // SHOW (and log) a park that is not this call's.
      setPark('')
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [snap.hunt?.reference, snap.hunt?.program, logCall, workPark])

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
    if (remoteMode || !asksForPark) {
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
  }, [logParkRef, logParkProgram, asksForPark, remoteMode])

  // Auto-load a park's details the moment a COMPLETE valid POTA reference is entered:
  // instant offline lookup first, then the live POTA directory if it's not in the local list. Purely
  // informational — never blocks typing or logging, and swallows all errors (offline = no chip).
  // `asksForPark` first: a pending hunt still PREFILLS logParkRef in every exchange
  // (the engine tags by callsign whichever strip logs), so without this the satellite
  // exchange would fire a local lookup and then a live POTA-directory fetch for a chip
  // it never renders.
  useEffect(() => {
    const ref = logParkRef.trim().toUpperCase()
    if (remoteMode || !asksForPark || logParkProgram !== 'POTA' || !/^[A-Z0-9]{1,4}-\d{4,5}$/.test(ref)) {
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
          // From a browser only the station's own list is read; the live directory is not.
          if (remoteMode) {
            setParkDetail(null)
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
  }, [logParkRef, logParkProgram, asksForPark, remoteMode])

  // WHERE THE PARK IN THE BOX IS ("US-ND"), the places its contact takes its state from: a hunted
  // park's from the hunt, which the station tags by call in every exchange, and a typed POTA
  // park's from its lookup. Empty with no park in the box, or one nothing here could place.
  const boxedRef = logParkRef.trim().toUpperCase()
  const parkPlaces: string[] =
    boxedRef === ''
      ? []
      : snap.hunt &&
          boxedRef === snap.hunt.reference.trim().toUpperCase() &&
          logCall.trim() !== '' &&
          sameCall(snap.hunt.call, logCall)
        ? (snap.hunt.states ?? [])
        : asksForPark && logParkProgram === 'POTA' && parkDetail?.reference === boxedRef
          ? (parkDetail.states ?? [])
          : []
  const parkPlacesKey = parkPlaces.join(',')
  // A park in one state puts that state in the STATE box, so the operator sees the state the
  // contact will log, and can change it. It replaces a callbook's (the activator's home address,
  // which is not where they are operating) and never a state the operator typed. When the park
  // that placed the box goes, its state goes with it.
  //
  // A park ON A STATE LINE empties the box instead, and the pick below asks which of its states:
  // the park decides, and has not decided yet. The callbook's state is not a pick (it says where
  // the activator lives), so it goes; logged with no pick, the contact has no state and the
  // Logbook flags it.
  useEffect(() => {
    if (stateSourceRef.current === 'operator' && stateBoxRef.current.trim() !== '') return
    if (parkPlaces.length === 1) {
      const code = placeCode(parkPlaces[0])
      if (stateBoxRef.current !== code || stateSourceRef.current !== 'park') setStateBox(code, 'park')
    } else if (parkPlaces.length > 1) {
      if (stateBoxRef.current !== '' || stateSourceRef.current !== 'park') setStateBox('', 'park')
    } else if (stateSourceRef.current === 'park') {
      setStateBox('', 'operator')
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [parkPlacesKey])

  // ⭐ A BAND CHANGE SETS THE UN-LOGGED LINE ASIDE (`clearCallOnBandChange`). EVERY box goes, not
  // just the call: a name, a comment or a report left behind would be logged against the next call
  // typed, on a line that looks fresh. The line is kept whole (`BandClearedLine`: the boxes, the
  // call's start time, the callbook fill and its owner, the contest exchange and its call-history
  // marks) with the band, frequency and mode it was made on, read from the render BEFORE the change
  // (`lineContextRef`). The notice and its Restore last until the operator restores, dismisses it,
  // or logs that very contact by hand (`loggedFromStrip`); typing or logging another call does not
  // end them, and they come back
  // with the cockpit after a page switch (the draft keeps them). A later band change with nothing in
  // the line leaves them as they are; one with another line in it sets that one aside instead.
  // Restore puts the line back over whatever is in it, and the contact logs on the band it was made
  // on (`restoredContext`); a contest contact logs on the radio's band, so a contest strip offers
  // Restore only back on that band. Declared BEFORE the click-to-work prefill below, so a spot on
  // another band, whose QSY and call land in one render, sets the old line aside first and then
  // takes the new call.
  type BandCleared = BandClearedLine & { fdFields: Record<string, string>; fill: FillState }
  const [bandCleared, setBandCleared] = useState<BandCleared | null>(() =>
    kept?.bandCleared ? { ...kept.bandCleared, fdFields: {}, fill: EMPTY_FILL } : null,
  )
  // The contact a Restore put back logs with the context it was made in, while that call is the
  // one in the line.
  const [restoredContext, setRestoredContext] = useState<(LineContext & { call: string }) | null>(kept?.restored ?? null)
  // A draft remembers the count it was kept at, so a band change made while this cockpit was away
  // (on the FT screen, say) sets the line aside when the strip comes back, as it would have here.
  const bandTickSeen = useRef(kept?.bandChangeTick ?? snap.bandChangeTick)
  // Where the radio was at the last render: the band, frequency and mode the line was being made
  // on. Updated by the effect declared just AFTER the band-change effect, so that effect still
  // reads the render before the change. A draft brings back where it was kept.
  const lineContextRef = useRef<LineContext | null>(kept?.context ?? null)
  // A band change that came while a contact's edit had the line's place: done as the edit closes.
  const bandClearPending = useRef<LineContext | null>(null)
  /** The line as it stands, every box of it — what a draft keeps and a band change sets aside. */
  const currentLine = (): LogDraft => ({
    call: logCall,
    rstSent: logRstSent,
    rstRcvd: logRstRcvd,
    name: logName,
    qth: logQth,
    comment: logComment,
    notes: logNotes,
    grid: logGrid,
    state: stateBoxRef.current,
    stateSource: stateSourceRef.current,
    callbookState,
    country: logCountry,
    image: logImage,
    parkProgram: logParkProgram,
    parkRef: parkBoxRef.current,
    parkFor: parkForRef.current,
    workPark,
    enrichedFor: enrichedForRef.current,
    enteredUnix: callEnteredRef.current?.unix ?? null,
  })
  /** Is anything in the line: the call, or any box typed without one (a name, a comment, a report).
   *  A park a hunt put in the empty line, and the state its park put there, are not: the hunt fills
   *  every empty line for hours, and counted, it replaced a set-aside contact at the next band change. */
  const lineHasContent = (l: LogDraft) =>
    [l.call, l.name, l.qth, l.comment, l.notes, l.grid].some((v) => v.trim() !== '') ||
    (l.stateSource === 'operator' && l.state.trim() !== '') ||
    (!l.parkFor.hunt && l.parkRef.trim() !== '') ||
    l.rstSent != null ||
    l.rstRcvd != null
  const setLineAside = (context: LineContext) => {
    setBandCleared({ line: currentLine(), context, fdFields, fill: fillRef.current })
    // As Clear does (the serial in flight released, Enter Sends Message at a new contact), and every
    // box with it, as Alt+W's wipe does. No caret move: the operator is at the band control.
    reset(false)
    setFdFields({})
  }
  useEffect(() => {
    const seen = bandTickSeen.current
    bandTickSeen.current = snap.bandChangeTick
    // Not before a snapshot has said where the count stands: the first one is no change.
    if (!clearCallOnBandChange || seen == null || snap.bandChangeTick == null || snap.bandChangeTick === seen) return
    // Never a contact on its way into the log, nor one for another radio (the override's band is
    // typed, not the dial's).
    if (!lineHasContent(currentLine()) || committing.current || overrideOpen) return
    const context = lineContextRef.current ?? lineContextNow()
    if (editing) {
      bandClearPending.current ??= context
      return
    }
    setLineAside(context)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [snap.bandChangeTick])
  useEffect(() => {
    // Off the bands on a listen (WWV) the line is still the one being made on the band left: a
    // band change after the listen sets it aside with that band and frequency, not with none. A
    // restored line is still the one made on its own band: set aside again, or kept by a draft, it
    // keeps that band.
    const now = draftContext ?? lineContextNow()
    if (now.band || !lineContextRef.current) lineContextRef.current = now
  })
  // A restored line logs on its own band while anything of it is in the line. Emptied by hand, it is
  // gone, and a contact typed into the empty line is a new one, made on the band the radio is on.
  useEffect(() => {
    if (restoredContext && !lineHasContent(currentLine())) setRestoredContext(null)
  })
  useEffect(() => {
    const context = bandClearPending.current
    if (editing || !context) return
    bandClearPending.current = null
    if (lineHasContent(currentLine()) && !committing.current && !overrideOpen) setLineAside(context)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [editing])
  /** A contact logged from this strip: the set-aside line goes only when it IS that contact (its call,
   *  on its band). A log of another call keeps it, as the engine's Log QSO keeps its own set-aside
   *  contact (operator ruling, 2026-10-27): it lasts until restored, dismissed or replaced by the next
   *  band change. */
  const loggedFromStrip = (loggedCall: string, loggedBand: string) =>
    setBandCleared((b) =>
      b && b.line.call.trim() && sameCall(b.line.call, loggedCall) && b.context.band.toLowerCase() === loggedBand.toLowerCase()
        ? null
        : b,
    )
  const restoreBlocked =
    bandCleared != null && fdActive && bandCleared.context.band.toLowerCase() !== snap.radio.band.toLowerCase()
  const restoreBandCleared = () => {
    const b = bandCleared
    if (!b || restoreBlocked) return
    setBandCleared(null)
    // Whatever the line holds now gives way, as Clear would clear it.
    reset(false)
    const l = b.line
    // The callbook fill belongs to this call again, so the call-change effect keeps it.
    enrichedForRef.current = l.enrichedFor
    setLogCall(l.call)
    callEnteredRef.current =
      l.call.trim() && l.enteredUnix != null ? { call: l.call.trim().toUpperCase(), unix: l.enteredUnix } : null
    setLogRstSent(l.rstSent)
    setLogRstRcvd(l.rstRcvd)
    setLogName(l.name)
    setLogQth(l.qth)
    setLogComment(l.comment)
    setLogNotes(l.notes)
    setLogGrid(l.grid)
    setStateBox(l.state, l.stateSource)
    setCallbookState(l.callbookState)
    setLogCountry(l.country)
    setLogImage(l.image)
    setLogParkProgram(l.parkProgram)
    parkForRef.current = l.parkFor
    setPark(l.parkRef)
    setParkPicked(!!l.parkRef)
    setWorkPark(l.workPark)
    humanCallEditRef.current = false
    settledCallRef.current = true
    if (fdActive) {
      setFdFields(b.fdFields)
      setFill(b.fill)
    }
    // Logged on the band it was made on. (Off the bands, a listen, there is no band to log on, and
    // the line logs on the radio's, as it always did.)
    if (b.context.band) {
      if (remoteMode) setRemoteDraftContext(b.context)
      else setRestoredContext({ ...b.context, call: l.call })
    }
    // The CW macros' {HISNAME}/{HISSTATE} for this call again, which the clear emptied.
    if (!remoteMode && l.call.trim()) void setCwPeerInfo(l.call.trim(), l.name, l.state)
    // …and the station takes the call up again, as Alt+W's put-back does.
    if (fdActive && !remoteMode && l.call.trim()) void contestWorking(l.call.trim()).catch(() => {})
  }

  // Click-to-work prefill: land the call + drop focus on RST so the operator types the report
  // and hits Enter. Keyed on `ts` to refire on re-click of the same call.
  useEffect(() => {
    if (!pendingWork) return
    if (remoteMode && (remote?.pending || remoteDraftContext || logCall.trim() || logName.trim() || logQth.trim() || logComment.trim() || logNotes.trim() || logParkRef.trim() || overrideOpen || rstSentShown !== defaultRst || rstRcvdShown !== defaultRst)) {
      setRemoteWorkOffer({ ...pendingWork, context: currentDraftContext() })
      onConsumeWork?.()
      return
    }
    if (remoteMode) { setRemoteDraftContext(currentDraftContext()); setRemoteWorkOffer(null) }
    setLogCall(pendingWork.call.toUpperCase())
    // The row's park comes with its call, a new object per click so the prefill above runs again
    // for a re-click of the same station. A handoff with no park ends the last one's.
    const handed = pendingWork.park
    setWorkPark(handed ? { call: pendingWork.call, program: handed.program, reference: handed.reference } : null)
    humanCallEditRef.current = false // a clicked spot is not a human keystroke…
    settledCallRef.current = true // …but it IS a final call, so it still gets enriched
    // ⭐ THE CONTEST SERIAL'S OTHER DOOR. This fill is programmatic, so no blur fires and
    // `onCallBlur` — the only other caller — never runs: the clicked station was never
    // bound to a number at all. `CwCockpit` arms the macro peer off this same click and
    // `{CALL}` expands from that peer, so the keyer sent the NEW callsign with the
    // PREVIOUS station's serial; on phone there is no macro and the operator read that
    // stale number aloud off this strip.
    //
    // A clicked spot is unambiguously a NEW contact, so this path opens the parking door
    // first — unlike typing over the box, which is a busted-call correction as often as
    // a move and is left to the session to resolve. CHAINED, not two `void`s: the park
    // has to land before the commit, or the commit is read as a correction of whatever
    // was still in flight.
    if (fdActive && !remoteMode) {
      const worked = pendingWork.call.trim()
      void contestEntryReset()
        .then(() => contestWorking(worked))
        .catch(() => {})
    }
    // A spot handed over is a station to answer: Enter Sends Message goes to S&P.
    if (esmLive) esm?.dispatch('spot')
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
  // The report SENT this strip last filled in (`cwLive.rstSent`): it follows the host's value while
  // the box still shows it, as JS8Call's log dialog takes their SNR when it opens.
  const cwRstSentAuto = useRef<string | null>(null)
  const cwNameFilled = useRef(false)
  // The two-way link (`onCallChange`): the call the strip and its host last agreed on, trimmed and
  // uppercased. A fill sets it BEFORE the strip changes, so the change is not reported back; a
  // report sets it before the host changes, so the host's echo is not filled back over a keystroke.
  const onCallChangeRef = useRef(onCallChange)
  onCallChangeRef.current = onCallChange
  const linkedCall = useRef('')
  useEffect(() => {
    const linked = onCallChangeRef.current != null
    if (linked && !cwLive?.call && linkedCall.current !== '') {
      // The host's box was emptied — the one field is empty.
      linkedCall.current = ''
      cwFilledFor.current = null
      setLogCall('')
      return
    }
    if (!cwLive?.call) return
    const up = cwLive.call.toUpperCase()
    const { rst, name } = cwLive
    if (linked ? up !== linkedCall.current : cwFilledFor.current !== up) {
      // A confirmed chip click always lands. An unconfirmed best-guess fills only if the
      // operator hasn't typed their own call over our previous auto-fill (don't clobber).
      const overridden =
        !cwLive.confirmed &&
        logCallRef.current.trim() !== '' &&
        logCallRef.current.toUpperCase() !== (cwFilledFor.current ?? '')
      if (!overridden) {
        if (linked) linkedCall.current = up
        setLogCall(up)
        humanCallEditRef.current = false // decoder fill, not a keystroke…
        // …and enrich it once the decoder is SURE. A best guess still changes character by
        // character, and each change would be a callbook request for a call nobody worked.
        settledCallRef.current = cwLive.confirmed === true
        cwFilledFor.current = up
        cwRstFilled.current = false
        cwRstSentAuto.current = null
        cwNameFilled.current = false
      }
    }
    if (cwFilledFor.current && cwLive.rstSent && cwLive.rstSent !== cwRstSentAuto.current) {
      const sent = cwLive.rstSent
      const auto = cwRstSentAuto.current
      setLogRstSent((v) => (v == null || v.trim() === '' || v === defaultRst || v === auto ? sent : v))
      cwRstSentAuto.current = sent
    }
    if (cwFilledFor.current && !cwRstFilled.current && rst) {
      // The decoder's read of `rst` is the report we RECEIVED from them.
      setLogRstRcvd((v) => (v == null || v.trim() === '' || v === defaultRst ? rst : v))
      cwRstFilled.current = true
    }
    if (cwFilledFor.current && !cwNameFilled.current && name) {
      setLogName((v) => (v.trim() ? v : name))
      cwNameFilled.current = true
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [cwLive?.call, cwLive?.rst, cwLive?.name, cwLive?.rstSent])
  // …and the strip's side of the link: every change the fill above did not make is reported.
  useEffect(() => {
    const c = logCall.trim().toUpperCase()
    if (!onCallChangeRef.current || c === linkedCall.current) return
    linkedCall.current = c
    onCallChangeRef.current(c)
  }, [logCall])
  // What this strip would log, for the Rotor box beside the cockpit (`onEntryCall`): settled, and
  // withdrawn when the strip goes.
  const onEntryCallRef = useRef(onEntryCall)
  onEntryCallRef.current = onEntryCall
  useEffect(() => {
    const c = logCall.trim().toUpperCase()
    const id = setTimeout(() => onEntryCallRef.current?.(c), ENTRY_SETTLE_MS)
    return () => clearTimeout(id)
  }, [logCall])
  useEffect(() => () => onEntryCallRef.current?.(''), [])

  // `mode` is this cockpit's LOG mode; the scope flag mirrors the engine's worked-band sets
  // so the Dupe badge and the B4 chips can never disagree about what a dupe is. Until the
  // window has an answer the card shows the empty log's history (none), and no need badge (below).
  const histQuestion = {
    kind: 'callHistory',
    call: logCall,
    band: snap.radio.band,
    mode,
    matchMode: snap.b4MatchMode ?? false,
  } as const
  const hist = useLogAnswer(readsLog ? histQuestion : null, snap.logTick) ?? emptyAnswer(histQuestion)

  // The award identity for the badges: cty.dat resolved from the CALL (a local
  // in-process table — no network), never the QRZ country string. Keying on the
  // QRZ spelling made NEW ONE fire on every German/Russian contact forever,
  // because "Germany" can never match the "Fed. Rep. of Germany" already in the
  // log. `logCountry` remains the display text and the legacy fallback.
  const [logEntity, setLogEntity] = useState<string | null>(null)
  useEffect(() => {
    const call = logCall.trim()
    if (remoteMode || !call) {
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
  }, [logCall, remoteMode])
  const entityForBadge = logEntity ?? logCountry
  // Is the entity new, and which of its band/mode slots are worked — one question. NO NEED BADGE
  // UNTIL ITS ANSWER IS HERE: the empty log's answer stands in until then, and it calls every entity
  // new — "New DXCC!" over a worked one while the log loaded (C17D). Its slots are empty, so the
  // band and mode badges are already off.
  const entityQuestion = { kind: 'entity', entity: entityForBadge } as const
  const entityAnswered = useLogAnswer(readsLog ? entityQuestion : null, snap.logTick)
  const entityAnswer = entityAnswered ?? emptyAnswer(entityQuestion)
  const newEntity = entityAnswered !== undefined && entityAnswer.newEntity

  // WHERE THE CONTEST PLACES THE CALL (`contestPlace`): the entity its scorer counts the call
  // under, for the country line, and the country file's zone, as a HINT in a contest that
  // receives a CQ zone — the slot the DTO tags with ADIF's `CQZ`. Both from ONE answer about
  // the call in the box, so the line never names a country the scorer does not count (IT9 is
  // Sicily in CQ WW, where the badges' `logEntity` says Italy; Italy in every other contest) and
  // never pairs one call's country with another's zone. The zone is the placeholder of that box
  // and never its value: a
  // prefix's zone is the likely one, and a station in a zone other than its prefix's
  // (a portable, a W6 in zone 4) is exactly what the operator must still copy.
  const zoneSlot = fdActive ? fdReceives.find((f) => f.adif === 'CQZ') : undefined
  const [place, setPlace] = useState<{ call: string; entity: string; cqZone: number | null } | null>(null)
  // Not asked in the Field Day events, which show no country and receive no zone.
  const placeAsked = fdActive && !isFieldDay(fieldDay?.event)
  const placeEvent = fieldDay?.event ?? ''
  useEffect(() => {
    const call = logCall.trim()
    if (remoteMode || !placeAsked || !call) {
      setPlace(null)
      return
    }
    let stale = false
    void contestPlace(call, placeEvent)
      .then((p) => {
        if (!stale) setPlace(p ? { call, ...p } : null)
      })
      .catch(() => {})
    return () => {
      stale = true
    }
  }, [logCall, remoteMode, placeAsked, placeEvent])
  const placed = place != null && place.call === logCall.trim() ? place : null
  const zoneHint = zoneSlot && logCall.trim().length >= 3 && placed?.cqZone != null ? String(placed.cqZone) : null
  // The contest strip's country line: the country, and its CQ zone in a contest that receives one.
  const fdCountryZone = placed != null && zoneHint != null
  const fdCountry = placed
    ? fdCountryZone
      ? t('logEntry.contest.country.zone', { country: placed.entity, zone: zoneHint })
      : placed.entity
    : ''

  // DXCC-Challenge axis: the entity is already worked, but is THIS band (or mode) a new
  // slot for it? Only meaningful once the entity is in the log (an ATNO is owned by
  // `newEntity` above; a blank/unresolved entity yields workedEver=false and falls through
  // to the plain "not in your log" line). Bands/modes are entity-wide; band wins over mode.
  const slots = entityAnswer.slots
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

  // The IOTA reference the station offers this call (a spot comment's, or the callbook's): shown as
  // a chip, logged only when the operator takes it (operator decision, 2026-10-27). Asked again when
  // a lookup here answers. Not in a contest strip, nor from a Remote browser, whose log goes by
  // another road.
  const [lookupTick, setLookupTick] = useState(0)
  const iotaOffer = useIotaOffer(logCall, null, lookupTick, remoteMode || fdActive)

  // QRZ callbook autofill — fills ONLY blank fields, never clobbers operator input. The
  // explicit button toasts; the on-blur auto-lookup is silent on failure so an operator
  // without QRZ configured isn't nagged on every Tab.
  const lookup = async (silent: boolean) => {
    if (remoteMode || qrzBusyRef.current) return
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
    // Only a blank the OPERATOR owns: a box the park placed holds the park's answer, and on a state
    // line that answer is an empty box until the operator picks. Filled here, it would show the
    // licence's state on a contact the station logs with none.
    if (r.state && !stateBoxRef.current.trim() && stateSourceRef.current !== 'park') setStateBox(r.state, 'callbook')
    if (r.country) setLogCountry((v) => (v.trim() ? v : r.country ?? ''))
    setLogImage(r.image ?? null) // display-only; no operator value to preserve
    setCallbookState(r.state ?? '') // the recall card's, beside the callbook's town
    enrichedForRef.current = call.toUpperCase()
    setLookupTick((n) => n + 1)
    // Feed the worked station's name/state to the engine for the {HISNAME}/{HISSTATE} CW-macro
    // tokens (keyed to the call so a stale lookup can't key the wrong name).
    void setCwPeerInfo(call, preferredName ?? '', r.state ?? '')
    if (!silent) {
      const detail = [
        r.name,
        r.grid && t('callbook.detail.grid', { grid: r.grid }),
        r.state,
        // Short-path bearing from the operator's grid, exactly as the StationCard shows it.
        azimuthLabel(azimuthTo(snap.mygrid, r.grid, r.country)),
      ]
        .filter(Boolean)
        .join(' · ')
      const vals = { call: r.call, detail: detail || r.country || t('callbook.detail.found') }
      pushToast(r.grid ? t('callbook.result', vals) : t('callbook.resultNoGrid', vals), 'info')
    }
  }

  const onCallBlur = () => {
    if (!fdActive && !qrzBusy && logCall.trim().length >= 3 && !logName.trim()) void lookup(true)
    // ⭐ THE CONTEST SERIAL'S PEER DOOR. Blur is the one hook that catches every way an
    // operator finishes a call in the contest strip — SPACE walking to the first exchange
    // box, Tab, or a click away — so the number the strip shows and the keyer sends is bound
    // to THIS station before the exchange goes out. On COMMIT and never per keystroke: bound
    // to onChange it would mint a binding for every prefix of the call and strand all but the
    // last. (`contestPlace` above is the per-keystroke endpoint; this is deliberately not.)
    if (fdActive && !remoteMode) void contestWorking(logCall.trim()).catch(() => {})
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
    setOvFreq(String(liveFreqMhz))
    setOvMode(overrideModeFor(mode))
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

  // ⭐ THE DRAFT, KEPT AS THE STRIP CHANGES (`draft`, `features/logDraft`). After every render the
  // cockpit's draft is the contact as this strip shows it, so the cockpit can leave the screen at any
  // moment with nothing left to save, and whatever empties the strip — a log, Clear, or a rule of the
  // strip's own — empties the draft with it. The radio's band, frequency and mode are not in it:
  // they are read as the contact is logged, as they always were.
  //
  // Only the OPERATOR'S contact is kept: a call typed, clicked to work or confirmed
  // (`humanCallEditRef`, `settledCallRef`). A decoder's unconfirmed guess is the decoder's, and it
  // offers one again when the cockpit is back, as on any mount. And none is kept
  //  · in a contest strip — its contact is the session's: the engine binds a serial number to the
  //    call, the logger window shares the entry, and Enter Sends Message would send to a call that
  //    came back without being typed;
  //  · with the other-radio override open — that contact is right only with the band, frequency,
  //    mode and time typed for it, and a draft carries none of them;
  //  · while the contact is being logged (`committing`), so a strip that goes before its log lands
  //    never comes back with the logged contact in it for a second Enter to log again.
  const committing = useRef(false)
  useEffect(() => {
    if (!draft) return
    const operators = logCall.trim() !== '' && (humanCallEditRef.current || settledCallRef.current)
    const keepsLine = operators && !fdActive && !overrideOpen && !committing.current
    // …and a line a band change set aside, while its Restore is on offer, with or without a line.
    const aside = bandCleared && !fdActive ? { line: bandCleared.line, context: bandCleared.context } : undefined
    writeLogDraft(
      draft,
      keepsLine || aside
        ? {
            ...(keepsLine ? currentLine() : BLANK_LINE),
            bandChangeTick: bandTickSeen.current,
            context: lineContextRef.current ?? lineContextNow(),
            bandCleared: aside,
            restored: keepsLine ? restoredContext ?? undefined : undefined,
          }
        : null,
    )
  })

  // `focus` false: no caret move to Call (a band change sets the line aside while the operator is at
  // the band control).
  const reset = (focus = true) => {
    // Before the call goes: a contact abandoned here gives its call-history values back. A
    // logged one has already said it keeps them (`endFill(true)` beside its `reset()`).
    endFill(false)
    setRemoteDraftContext(null)
    setRestoredContext(null)
    setLogCall('')
    setLogRstSent(null)
    setLogRstRcvd(null)
    setLogName('')
    setLogQth('')
    setLogComment('')
    setLogNotes('')
    setLogGrid('')
    setStateBox('', 'operator')
    setCallbookState('')
    setLogCountry('')
    setLogImage(null)
    setPark('')
    setWorkPark(null)
    // A Use pressed on the IOTA offer was for this contact, logged or let go of (Clear, a band
    // change setting the line aside): the next one, even the same call, starts with nothing taken.
    iotaOffer.reset()
    if (!remoteMode) void setCwPeerInfo('', '', '') // clear the {HISNAME}/{HISSTATE} tokens for the next contact
    // The entry line ended WITHOUT logging (the clear button, or moving on). The serial that
    // station copied stays bound to them — come back later and they get the same one — but the
    // live slot is released, so the next call committed is issued a number of its own instead
    // of inheriting the one that was in flight.
    if (fdActive && !remoteMode) void contestEntryReset().catch(() => {})
    // A new contact for Enter Sends Message: nothing has gone out to it.
    if (esmLive) esm?.dispatch('cleared')
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
    // contact immediately (rapid logging / a Field Day run), like WSJT-X.
    // rAF so it lands after the cleared value re-renders. Only the mounted layout's
    // input (FD or standard) holds the ref, so this always hits the visible field.
    // preventScroll for the same reason as the click-to-work RST focus above:
    // this focus readies the field, it must not snap the cockpit to the log.
    if (focus) requestAnimationFrame(() => callInputRef.current?.focus({ preventScroll: true }))
    onReset?.()
  }

  // ⭐ ALT+W — WIPE THE LINE, AND PUT IT BACK (`features/contestKeys.ts`: typing Alt+W again
  // reverses it). The wipe clears the call as Clear does — the serial in flight
  // released, Enter Sends Message at a new contact — and every box as well, a report back to its
  // default, keeping what the line held. A second Alt+W, with no other key between and the line
  // still as the wipe left it, puts back the call, the boxes and their call-history marks, and takes
  // the station's serial number up again, as the call box's commit does. What went out to the call
  // is not put back: Enter Sends Message starts that contact over, as after Clear.
  const pressWipe = () => {
    // A contact's edit (Ctrl+Y) has the line's place, and the line waits for it as typed: Alt+W
    // wipes nothing while the edit is open.
    if (editRows.length > 0) return
    const wiped = wipedRef.current
    wipedRef.current = null
    const blank =
      logCall.trim() === '' &&
      sentRstValue === sentRstDefault &&
      fdReceives.every((f) => fdValue(f) === (f.kind === 'rst' ? rstDefault : ''))
    if (wiped && blank) {
      setLogCall(wiped.call)
      setFdFields(wiped.fields)
      setFill(wiped.fill)
      if (wiped.call.trim()) void contestWorking(wiped.call.trim()).catch(() => {})
      return
    }
    wipedRef.current = { call: logCall, fields: fdFields, fill: fillRef.current }
    reset()
    setFdFields({})
  }
  pressWipeRef.current = pressWipe

  // THE LOGGER WINDOW'S ESC (`sharedEntry`): it clears the entry, as Clear does, from anywhere in
  // that window. The same press is also the station's halt there (`DetachedPanel`), so this listener
  // only clears. On every other strip Esc stays the cockpit's
  // (in CW, Operate, RTTY and PSK it stops TX), exactly as before. Left alone in the "I moved"
  // editor and a contact's edit, whose own Esc backs out of the edit, and while a dialog has the
  // key.
  const resetRef = useRef(reset)
  resetRef.current = reset
  useEffect(() => {
    if (sharedEntry !== 'logger' || !fdActive) return
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'Escape' || e.repeat || e.defaultPrevented) return
      if (e.target instanceof Element && e.target.closest('.le-fd-sent, .contest-edit, [role="dialog"]')) return
      resetRef.current()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [sharedEntry, fdActive])

  // FD exchange gate: every REQUIRED received slot must be satisfied. Never substitute '?' for a
  // blank (it would fake a section multiplier) — the manual log is blocked until class is present
  // AND section is a real ARRL/RAC code (validated against the same universe as Settings).
  //
  // The gate is now over the session's received slots rather than a hardcoded pair, which for
  // Field Day is the same two slots and the same two tests: CLASS non-blank, SECTION a member of
  // `fd_sections`. `FD_SECTION_CODES` above is that domain's value set, and `contestDomains.ts`
  // is where the strip and the board now read it from.
  //
  // A QSO party's QTH box is judged by the call's country too (`logEntity`): from a station in the
  // USA or Canada a value must be on the contest's lists, where a DX station may send its country.
  const fdBadField = fdReceives.find((f) => !fdFieldOk(f, fdValue(f), fdLineDomain(f), logEntity))
  const fdExchangeOk = fdBadField === undefined

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
  // …and the same for its UTC instant (#280). The time is a plain 24-hour box now, so a typo is
  // possible where the native control made one impossible; a date + time that is not a real one
  // holds Log like a bad frequency does, never quietly replaced by "now".
  const ovWhen = overrideOpen ? utcDateTimeToUnix(ovDate, ovTime) : null
  const ovTimeBad = parseUtcTime(ovTime) === null
  // …and the date box is UTC text for the same reason: a native date control is drawn in the
  // OS locale and its "Today" fills the LOCAL date, which after 0000Z west of Greenwich is
  // the previous UTC day — a contact filed a whole day early.
  const ovDateBad = parseUtcDate(ovDate) === null
  const ovFreqBlocked = overrideOpen && !ovFreqOk
  const overrideBlocked = ovFreqBlocked || (overrideOpen && ovWhen === null)
  // Remote drafts survive navigation and other operators' QSYs. Keep their
  // captured contact context; the explicit other-radio override still wins.
  // …and a line a band change set aside and Restore put back logs on the band it was made on.
  const draftContext = remoteMode
    ? remoteDraftContext
    : restoredContext && (!restoredContext.call.trim() || sameCall(restoredContext.call, logCall))
      ? restoredContext
      : null
  /** The band, frequency and mode a contact typed now is made on (a Remote draft's, where it has one). */
  function lineContextNow(): LineContext {
    return remoteDraftContext ?? { band: snap.radio.band, freqMhz: liveFreqMhz, mode }
  }
  const effBand = overrideOpen ? ovBand : draftContext?.band ?? snap.radio.band
  const effFreqMhz = overrideOpen ? ovFreqNum : draftContext?.freqMhz ?? liveFreqMhz
  const effMode = overrideOpen ? ovMode : draftContext?.mode ?? mode
  // ⭐ While activating, the POTA dupe — warn only, never a refusal: this call already worked
  // today from the activation's park on the band and mode this contact will log on (and the park
  // in the box, if any). The "worked ever on this band" badge is hidden meanwhile: last week's
  // contact is no dupe for today's activation.
  // A paused activation (taken up at launch, not yet confirmed) tags nothing, so it is not one.
  const activating = snap.activation?.reference != null && snap.activation.restored !== true
  const potaDupeNow = potaDupe(hist.qsos, snap.activation, {
    call: logCall,
    band: effBand,
    mode: effMode,
    theirProgram: logParkProgram,
    theirRef: logParkRef,
    // The day the contact will log on: the hand-entered UTC time of a back-dated one, else now.
    whenUnix: ovWhen ?? Math.floor(Date.now() / 1000),
  })

  useEffect(() => {
    if (remoteMode && remote?.resetKey) reset()
    // The explicit receipt/dismissal key alone owns this reset.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [remote?.resetKey])

  // True when the contact reached the log; every refusal, said or not, is false.
  const logIt = async (): Promise<boolean> => {
    if (remote && (!remote.canSubmit || remote.busy || remote.pending || fdActive)) return false
    const call = logCall.trim().toUpperCase()
    if (!call) return false
    if (overrideBlocked) {
      pushToast(
        ovFreqBlocked ? t('logEntry.override.blocked') : t('logEntry.override.timeBlocked', { date: UTC_DATE_FORMAT, ...UTC_TIME_FORMATS }),
        'error',
      )
      return false
    }
    if (gridBlocked) {
      pushToast(t('logEntry.grid.blockedToast', { grid: logGrid.trim() }), 'error')
      return false
    }

    if (fdActive) {
      // FD path: contestLogManual rejects on the ruleset's own dupe key. Every REQUIRED slot
      // is mandatory — never log
      // a blank as the literal '?' (it would inflate the section multiplier), so bail until the
      // exchange is complete and the section is a real ARRL/RAC code.
      if (!fdExchangeOk || !sentRstGood) return false
      // ⭐ THE TRANSPORT IS THE FIELD VECTOR — `contest_log_manual(call, fields, …)`,
      // one pair per box the strip actually rendered, in the session's own receive
      // order. It replaced `fd_log_manual`'s two positional slots, which could carry
      // Field Day's CLASS and SECTION and nothing else: a QSO party receives RST + QTH
      // and CQP receives a serial + QTH, so the first two slots of the wire meant a
      // different thing per contest and the UI would have had to know which.
      const ex = fdReceives.map(
        // The RESOLVED value, so a county typed by name reaches the log as its code —
        // the same function the verdict above used to admit it.
        (f) => [f.key, fdSlotValue(f, fdValue(f), fdLineDomain(f))] as [string, string],
      )
      const fmode = fdMode ?? 'PH'
      // The on-air mode behind the class, for 'DIG' alone — see `fdSubmode`. Sent only with
      // that class so a CW or phone contact can never acquire one it has no meaning for.
      const fsub = fmode === 'DIG' ? fdSubmode : undefined
      // The report typed in the "RST sent" box, for this contact only. The two commands below carry
      // it when one was typed, and are the commands themselves when none was, so a contact with
      // nothing typed there makes exactly the call it always made.
      const snt = sentRstTyped ? sentRstValue : undefined
      // ⭐ A CONTACT THIS STRIP LOGS ENDS AS ITS LOG LANDS (below): its boxes become the next
      // contact's then, as the after-log clear makes them once the snapshot shows the contact.
      // With the logger window open, what this strip puts next (the call going blank, the next call
      // typed before that snapshot) reaches the other window, which takes it up over its own clear:
      // put with the logged contact's boxes, it carried their county into the next contact.
      // `loggedRow` is what a row of this contact carries over (Field Day's class and section).
      const loggedRow = {
        class: ex.find(([k]) => k === 'CLASS')?.[1] ?? '',
        section: ex.find(([k]) => k === 'SECTION')?.[1] ?? '',
      }
      // ⭐ A COUNTY LINE IS ONE ACTION AND SEVERAL CONTACTS: the same vector once per county,
      // with that county in the line's box, in ONE engine call — so every row carries one
      // time, band and mode, and each is dupe-checked on its own. The gate above has already
      // refused a line with a part that is not a county.
      const lineSlot = fdReceives.find((f) => fdLineDomain(f) && isCountyLine(fdValue(f)))
      const lineRead = lineSlot ? readCountyLine(fdLineDomain(lineSlot)!, fdValue(lineSlot)) : undefined
      // ⭐ THE SHARED ENTRY'S ENTER: with the logger window open, the log names the entry this strip
      // shows, so Enter pressed in both windows at once logs the contact once (the engine refuses
      // the second). With it closed nothing is named and the commands get what they always got.
      const entry = share.sharing ? await share.claim(shareContent) : undefined
      // ⭐ THE STRIP THAT LOGS CLEARS ONCE, as its log lands (below), and the rows its log adds
      // are cleared for then: `fdSeenLen` moves on past them. The snapshot that first showed them
      // cleared again, emptying a county or report typed for the next contact in between (a fast
      // typist, or call history filling it from the next call). Counted from what this strip had
      // seen as it logged, so rows a snapshot showed before the log answered count once.
      const seenAtLog = fdSeenLen.current
      if (lineSlot && lineRead?.ok) {
        const exchangeWith = (county: string) =>
          ex.map(([k, v]) => [k, k === lineSlot.key ? county : v] as [string, string])
        const rows = lineRead.counties.map(exchangeWith)
        const logRows: typeof contestLogManualRows = snt
          ? (c, r, m, s, e) => contestLogManualRows(c, r, m, s, e, snt)
          : contestLogManualRows
        const r = await withErrorToast(
          () =>
            entry
              ? sharedRefusal(logRows(call, rows, fmode, fsub, entry), call)
              : logRows(call, rows, fmode, fsub),
          t('logEntry.fd.failed'),
        )
        if (r) {
          const logged = lineRead.counties.filter((_, i) => r[i])
          const dupes = lineRead.counties.filter((_, i) => !r[i])
          if (logged.length > 0) {
            pushToast(
              t('logEntry.countyLine.logged', {
                call,
                exchange: exchangeWith(logged.join('/'))
                  .map(([, v]) => v)
                  .filter((v) => v !== '')
                  .join(' '),
                mode: fmode,
                count: logged.length,
              }),
              'success',
            )
          }
          // Which county was the dupe, by name — the engine refused it exactly as it refuses
          // a single contact, and "a dupe" alone would not say which of the counties it was.
          if (dupes.length > 0) {
            pushToast(
              t('logEntry.countyLine.dupe', { call, counties: dupes.join(', '), count: dupes.length }),
              'error',
            )
          }
          // The contact is in the log once any county entered it; a line that was all dupes
          // leaves the entry on screen, as a refused single contact does.
          if (logged.length > 0) {
            // Logged through the shared entry: this strip's changes from here are the next contact's.
            if (entry) share.logged()
            fdSeenLen.current = Math.max(fdSeenLen.current, seenAtLog + logged.length)
            endFill(true)
            setFdFields((prev) => afterLogged(prev, fdReceives, loggedRow))
            reset()
            loggedFromStrip(call, snap.radio.band)
            return true
          }
        }
        return false
      }
      // ⭐ THE SATELLITE EXCHANGE TAKES ITS BAND OFF THE PASS, NOT THE DIAL. The
      // ordinary command stamps the band the radio is on at the moment you type, which
      // for a bird is whatever HF run the club went back to after LOS. `exchange` is
      // already the prop that says which surface this strip is on, so the routing is
      // the one fact the strip knows and the engine cannot infer: a transponder still
      // held does not make a contact typed into the Phone cockpit a satellite contact.
      const logOne = exchange === 'satellite' ? contestLogSatellite : contestLogManual
      const logToContest: typeof contestLogManual = snt ? (c, f, m, s, e) => logOne(c, f, m, s, e, snt) : logOne
      const r = await withErrorToast(
        () =>
          entry
            ? sharedRefusal(logToContest(call, ex, fmode, fsub, entry), call)
            : logToContest(call, ex, fmode, fsub),
        t('logEntry.fd.failed'),
      )
      if (r) {
        // The toast names what was copied, in receive order, whatever the slots are —
        // it read `class`/`section` off two named locals, which for CQP would have
        // announced a serial as a Field Day class.
        // A blank optional slot (a DX station's QTH) is not a word in the toast.
        const exchangeWords = ex.map(([, v]) => v).filter((v) => v !== '').join(' ')
        // ⭐ A DUPE IS LOGGED, MARKED AND SCORES ZERO in every shipped contest, so the press
        // answers like any other — and said "Logged" exactly as for a new contact. The engine's
        // own mark on the row it just wrote (the newest row for this call) says which it was.
        const rows = r.fieldDay?.log ?? []
        let newest: FieldDayQso | undefined
        for (let i = rows.length - 1; i >= 0 && newest === undefined; i--) {
          if (sameCall(rows[i].call, call)) newest = rows[i]
        }
        if (newest?.dupe) {
          pushToast(t('logEntry.fd.loggedDupe', { call, exchange: exchangeWords, mode: fmode }), 'info')
        } else {
          pushToast(t('logEntry.fd.logged', { call, exchange: exchangeWords, mode: fmode }), 'success')
        }
        if (entry) share.logged()
        fdSeenLen.current = Math.max(fdSeenLen.current, seenAtLog + 1)
        endFill(true)
        setFdFields((prev) => afterLogged(prev, fdReceives, loggedRow))
        reset()
        loggedFromStrip(call, snap.radio.band)
        return true
      }
      return false
    }

    // Standard logbook path.
    const rstSent = rstSentShown.trim() || defaultRst
    const rstRcvd = rstRcvdShown.trim() || defaultRst
    // Override open → the hand-entered UTC instant (one that is not a real date and time held
    // Log above); closed → now, exactly as before.
    const nowUnix = Math.floor(Date.now() / 1000)
    // The contact started when its call was entered: the host's start for this call (`startedAt`,
    // JS8's), else the moment it was typed or clicked into this strip — and it ends now.
    const hostStart = startedAt && sameCall(startedAt.call, call) ? startedAt.unix : null
    const entered = callEnteredRef.current && sameCall(callEnteredRef.current.call, call) ? callEnteredRef.current.unix : null
    const startUnix = hostStart ?? entered
    const started = !overrideOpen && startUnix != null ? Math.min(startUnix, nowUnix) : null
    const whenUnix = overrideOpen && ovWhen !== null ? ovWhen : started ?? nowUnix
    // A contact logged live ends now (TIME_OFF), as an FT contact's does — Phone, CW, RTTY and PSK
    // logged none until now. A hand-entered UTC time wins over both: the contact starts and ends at
    // the time typed.
    const live = !(overrideOpen && ovWhen !== null)
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
      timeOffUnix: live ? nowUnix : whenUnix,
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
      //
      // …the pending hunt's park FOR ITS OWN STATION. The same park in the box for another call (a
      // second activator at that park, clicked in Needed or typed) is that call's park, and the
      // auto-tag, which matches by call, would never put it on this record.
      ota:
        asksForPark &&
        logParkRef.trim() &&
        !(
          logParkRef.trim().toUpperCase() === (snap.hunt?.reference ?? '').trim().toUpperCase() &&
          sameCall(snap.hunt?.call ?? '', call)
        )
          ? { theirProgram: logParkProgram, theirRef: logParkRef.trim().toUpperCase() }
          : undefined,
    }
    if (remote) {
      try { await remote.submit(rec, overrideOpen ? 'explicit' : 'station'); reset(); loggedFromStrip(call, effBand); return true }
      catch { /* The adapter keeps a visible outcome and the draft. */ }
      return false
    }
    // The draft lets go of the contact as its log starts (see the draft effect); one that fails is
    // still on screen, and kept again from the next render on.
    committing.current = true
    if (draft) writeLogDraft(draft, null)
    const r = await withErrorToast(() => logQso(rec, stateSourceRef.current, iotaOffer.taken), t('logEntry.logFailed'))
    committing.current = false
    if (r) {
      pushToast(t('logEntry.logged', { call, mode: effMode }), 'success')
      reset()
      loggedFromStrip(call, effBand)
      return true
    }
    return false
  }

  // A host's "log it" (`logRequest`): this strip's own Log, for the station the request names and
  // no other. Keyed on the request's id, so one press is one answer.
  const logRequestRef = useRef(onLogRequest)
  logRequestRef.current = onLogRequest
  useEffect(() => {
    if (!logRequest) return
    const report = logRequestRef.current ?? (() => {})
    const want = logRequest.call.trim().toUpperCase()
    const held = logCall.trim().toUpperCase()
    if (want && !held) {
      // Filled for the operator to check: a sure call, looked up as a click-to-work is.
      setLogCall(want)
      humanCallEditRef.current = false
      settledCallRef.current = true
      report(logRequest.id, { kind: 'filled' })
      return
    }
    if (want && held !== want) {
      report(logRequest.id, { kind: 'other-call', held })
      return
    }
    // What disables this strip's Log button refuses the request too.
    const ready = fdActive
      ? held !== '' && fdExchangeOk && sentRstGood
      : held !== '' && !overrideBlocked && !gridBlocked && !(remote != null && (!remote.canSubmit || remote.busy || remote.pending))
    if (!ready) {
      report(logRequest.id, { kind: 'not-ready' })
      return
    }
    const { id, claim } = logRequest
    void claim()
      .catch(() => false)
      .then((mine) => {
        if (!mine) {
          report(id, { kind: 'taken' })
          return
        }
        report(id, { kind: 'logging' })
        // Claimed, but the log can still refuse it: said, so the host gives the claim back.
        void logIt()
          .catch(() => false)
          .then((landed) => {
            if (!landed) report(id, { kind: 'refused' })
          })
      })
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [logRequest?.id])

  const onEnter = (e: React.KeyboardEvent) => {
    if (e.key !== 'Enter') return
    // Alt+Enter logs without sending anything (rule 6); Ctrl or Cmd with Enter is not ESM either.
    if (esmLive && !e.altKey && !e.ctrlKey && !e.metaKey) {
      void pressEsm(e)
      return
    }
    void logIt()
  }

  /**
   * SPACE ADVANCES THE CONTEST EXCHANGE, because that is the key contest operators already
   * have in their fingers: contest loggers tab with the space bar, so there is no house style
   * to invent. Tab keeps working exactly as it did (plain DOM order), and the one tabIndex in this
   * file is the optional "RST sent" box's, which Tab skips as Space does (N1MM's Snt).
   *
   * Scoped to the Field Day strip on purpose: a space is legitimate in a name, a park
   * reference or a comment on the ordinary strip, so this belongs to the contest exchange
   * and nowhere else.
   *
   * ⚠️ IT CANNOT KEY THE RIG. The phone cockpits' space push-to-talk is a WINDOW handler
   * that ignores any event whose target is an input, so with the caret in these three boxes
   * that keystroke is already inert — this gives a dead key a job rather than taking a live
   * one away. `preventDefault` is what stops the space reaching the value: Class and Section
   * have no space-stripping of their own, so without it a reflexive space would quietly log
   * a class of "3A " or a section of " WI".
   */
  const onExchangeSpace = (e: React.KeyboardEvent, next: HTMLInputElement | null, prev: HTMLInputElement | null) => {
    if (e.key !== ' ' && e.code !== 'Space') return
    e.preventDefault()
    // SHIFT+SPACE WALKS BACK, the reverse of the same loop (Call ← field₁ ← … ← Call), as Shift+Tab
    // does in DOM order: a wrong box is corrected without the mouse.
    const to = e.shiftKey ? prev : next
    to?.focus()
    to?.select()
  }

  // ⭐ THE SENT SIDE: what the session is composing right now, as a VECTOR. Never a
  // preformatted string off the snapshot — the only thing that renders a row's sent
  // exchange is `sent_exchange(row, spec)` in Rust, and this describes no row.
  const fdComposing = fieldDay?.composing ?? []
  /** The Sending line: the session's exchange as THIS strip's contacts send it. A phone strip reads
   *  the engine's phone form, its report at two digits (`59`), as a phone row is stamped; CW and
   *  the keyboard modes read the session's own (`599`). A station that sends no phone form reads
   *  as it always did. */
  const fdSending =
    ((fdMode ?? 'PH') === 'PH' ? fieldDay?.composingTextPhone : undefined) ||
    fieldDay?.composingText ||
    fdComposing.map((v) => v.raw).filter((r) => r !== '').join(' ')

  /** "I moved" — commit the edited sent exchange (§4.1).
   *
   *  Writes the live session AND the setting a new session starts from, in one engine
   *  call, because either alone is a defect: a session-only edit is reverted by the
   *  next rebuild, and a setting-only edit reaches nothing while the mode is live.
   *
   *  A move into a different ROLE is refused by the engine with its own sentence —
   *  "that's a separate entry. End this session and start a new one?" — which surfaces
   *  through the error toast verbatim. It is deliberately NOT a catalog string: the
   *  refusal is the engine's, and a second copy here could drift from the one the
   *  engine actually enforces.
   *
   *  ⭐ A box is read by the QTH box's own function, `fdSlotValue`, so a county given by NAME
   *  (`DuPage`) goes as its code (DUPG), and naming the county already being sent moves
   *  nothing. It reads within the list the value being sent came from — an in-state station's
   *  counties, an out-of-state station's states and provinces — because a move the engine
   *  accepts stays on that list (over the state line is a separate entry). Five counties share
   *  their name with a state or province on the list beside them (`Washington`: WASH in
   *  Illinois, WA outside it); the QTH box cannot tell which was meant, and I moved can. A value
   *  sent from no list is read over the slot's lists, as the QTH box reads it. A name that
   *  resolves to no code goes as typed, for the engine to refuse in its own words. */
  const commitMove = async () => {
    if (movingTo === null) return
    const meant = (v: ContestFieldValue): string => {
      const typed = movingTo[v.key] ?? ''
      const f = fdReceives.find((s) => s.key === v.key)
      // A slot this role sends and never receives (no shipped role has one) keeps what was typed.
      if (!f) return typed.trim().toUpperCase()
      return fdSlotValue(v.domain ? { ...f, domains: [v.domain] } : f, typed)
    }
    const values = fdComposing
      .filter((v) => meant(v) !== v.raw.toUpperCase())
      .map((v) => [v.key, meant(v)] as [string, string])
    if (values.length === 0) {
      setMovingTo(null)
      return
    }
    const r = await withErrorToast(() => contestIMoved(values), t('logEntry.sent.moved.failed'))
    if (r) {
      setMovingTo(null)
      pushToast(t('logEntry.sent.moved.done'), 'success')
    }
  }

  // Enter in the CALL field: on a fresh call (not yet enriched, no name typed) do the QRZ lookup
  // first — like Tab — so a single Enter pulls the callbook; once enriched, Enter logs as usual.
  const onCallEnter = (e: React.KeyboardEvent) => {
    if (e.key !== 'Enter') return
    if (remoteMode) { e.preventDefault(); void logIt(); return }
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
  // plain lookups on data in hand. OWN dupe = the hard block contestLogManual will
  // refuse (same key: call, band, mode class). CLUB dupe = another position
  // already worked them — N3FJP semantics, a WARNING only; logging proceeds
  // and the host keeps both rows.
  //
  // The verdict itself lives in `features/contestDupe`, because the FT cockpit's callsign
  // card asks it too: two implementations of a dupe verdict is exactly how one cockpit ends
  // up telling the operator to call a station this log is about to refuse. The mode-class
  // folding that ILQP needs lives there with it.
  const fdTypedCall = logCall.trim().toUpperCase()
  const fdModeClass = fdMode ?? 'PH'
  // The exchange being typed, for a contest whose dupe key names a slot (every QSO party:
  // a station is worked once per band and mode FROM EACH COUNTY). The received side is what
  // the boxes will LOG, a county typed by name as its code (`fdSlotValue`, the function the
  // log resolves it with); the sent side is what this station is sending now, which is what
  // the engine stamps on the row it is about to write.
  const fdRx = (slot: string): string => {
    const box = fdReceives.find((f) => f.key === slot)
    return box ? fdSlotValue(box, fdValue(box), fdLineDomain(box)) : ''
  }
  const fdDupeOf = (rx: (slot: string) => string): ContestDupeVerdict =>
    contestDupe(fieldDay, fdTypedCall, snap.radio.band, fdModeClass, {
      rx,
      tx: (slot) => composingSlot(fieldDay?.composing, slot),
    })
  // ⭐ A COUNTY LINE IS JUDGED COUNTY BY COUNTY, as the log writes it: one row per county, each
  // admitted on its own key. Judged whole, `COOK/WILL` is no row's county, so a line whose every
  // county was already worked warned of nothing while each county alone did. The line's one
  // verdict, which ESM reads too, is an own dupe only when EVERY county is one (a line with a
  // new county is a contact that scores), and a club dupe when any county is.
  const fdLineBox = fdReceives.find((f) => fdLineDomain(f) && isCountyLine(fdValue(f)))
  const fdLineRead = fdLineBox ? readCountyLine(fdLineDomain(fdLineBox)!, fdValue(fdLineBox)) : undefined
  const fdLineDupes =
    fdLineBox && fdLineRead?.ok
      ? fdLineRead.counties.map((county) => ({
          county,
          dupe: fdDupeOf((slot) => (slot === fdLineBox.key ? county : fdRx(slot))),
        }))
      : undefined
  /** The line's counties whose verdict `keep` takes, in the order typed. */
  const fdLineCounties = (keep: (dupe: ContestDupeVerdict) => boolean): string[] =>
    (fdLineDupes ?? []).filter((c) => keep(c.dupe)).map((c) => c.county)
  const fdLineOwn = fdLineCounties((d) => d === 'own')
  const fdLineNew = fdLineCounties((d) => d !== 'own')
  const fdDupe: ContestDupeVerdict =
    fdLineDupes === undefined
      ? fdDupeOf(fdRx)
      : fdLineOwn.length === fdLineDupes.length
        ? 'own'
        : fdLineDupes.some((c) => c.dupe === 'club')
          ? 'club'
          : 'none'
  const fdOwnDupe = fdDupe === 'own'
  const fdClubDupe = fdDupe === 'club'
  // ⭐ THE DUPE BADGE'S "WHEN" — the row the same check matched, so it names the contact that
  // makes this one a dupe: under a rule that ignores band or mode, the row's own band and mode,
  // never the dial's. A county line is an own dupe only when every county is one; its first
  // county's row stands for the line.
  const fdDupeRow = !fdOwnDupe
    ? undefined
    : fdLineDupes === undefined
      ? contestDupeRow(fieldDay, fdTypedCall, snap.radio.band, fdModeClass, {
          rx: fdRx,
          tx: (slot) => composingSlot(fieldDay?.composing, slot),
        })
      : contestDupeRow(fieldDay, fdTypedCall, snap.radio.band, fdModeClass, {
          rx: (slot) => (slot === fdLineBox!.key ? fdLineOwn[0] : fdRx(slot)),
          tx: (slot) => composingSlot(fieldDay?.composing, slot),
        })
  const fdDupeWorked =
    fdDupeRow === undefined
      ? ''
      : fdDupeRow.whenUnix
        ? t('logEntry.fd.dupe.workedAt', {
            time: utcHhMm(fdDupeRow.whenUnix),
            band: fdDupeRow.band,
            mode: fdDupeRow.submode || fdDupeRow.mode || '',
          })
        : t('logEntry.fd.dupe.workedOn', {
            band: fdDupeRow.band,
            mode: fdDupeRow.submode || fdDupeRow.mode || '',
          })
  // ⭐ THE EARLY "WORKED" WARNING (`contestWorkedRows`): under a rule that keys on a county, the
  // moment the call is in, a station this position already logged on this band and mode turns the
  // Call box amber with the counties it was worked from. It is never the DUPE: the county decides
  // that, and a rover in a new county still scores. A county line is judged county by county above,
  // so it takes no early warning of its own.
  const fdWorkedQths =
    fdDupeRow === undefined && fdLineDupes === undefined
      ? workedValues(
          fieldDay,
          contestWorkedRows(fieldDay, fdTypedCall, snap.radio.band, fdModeClass, {
            rx: fdRx,
            tx: (slot) => composingSlot(fieldDay?.composing, slot),
          }),
        )
      : []
  const fdWorkedLine = fdWorkedQths.length > 0 ? t('logEntry.fd.worked', { qths: fdWorkedQths.join(', ') }) : ''
  // ⭐ WHICH COMPONENTS THE OWN-DUPE SENTENCE MAY NAME — the same two flags `contestDupe` just
  // applied. It has always keyed on exactly what the rule names; the sentence did not, and
  // naming the rig's CURRENT band and mode under a rule that ignores them points the operator
  // at the one place the contact is NOT. Seven of the nine rulesets that reach this sentence
  // name fewer than three components (CQ WW and WPX drop the mode class; Sweepstakes drops
  // both — rule 2.2, a station is worked once). The CLUB sentence below keeps both: that check
  // matches its key RAW, ungated by the rule, so it really is on this band in this mode class.
  //
  // ⚠️ `!== false`, not `=== true`. An absent rule is `contestDupe`'s LEGACY_TRIPLE, which
  // names all three, so a station too old to send a rule must keep the sentence it always had.
  const fdDupeByBand = fieldDay?.dupeRule?.byBand !== false
  const fdDupeByModeClass = fieldDay?.dupeRule?.byModeClass !== false
  // ⭐ WHICH CONTEST'S WORDS. Field Day's chip, hint and button name Field Day; every
  // other contest's contacts go to the same contest log under that contest's rules, and
  // telling a CQ WW operator their contacts go "to the Field Day log" is simply wrong.
  const fdEventIsFieldDay = isFieldDay(fieldDay?.event)
  // THE CONTEST'S OWN BANDS, as advice. A ruleset that names its bands (CQ WW RTTY: 80, 40,
  // 20, 15 and 10 m) makes the hint say so when the rig is somewhere else. It is never a
  // refusal — the contact still logs and nothing in the score reads the list — because the
  // sponsor's log checker, not this strip, decides what a contact on another band is worth.
  const contestBands = fdEventIsFieldDay ? [] : (fieldDay?.bands ?? [])
  const offContestBand =
    contestBands.length > 0 &&
    snap.radio.band !== '' &&
    !contestBands.some((b) => b.toLowerCase() === snap.radio.band.toLowerCase())
  // A USA or Canada station logged with no QTH, in a contest where a QTH is part of what
  // such a station sends (an OPTIONAL received QTH slot — DX stations send none). A
  // WARNING, never a refusal: the operator may not have copied it, and a contact is still
  // a contact. The entity is the cty.dat resolve the badges already use.
  const qthSlot = fdReceives.find((f) => f.key === 'QTH' && !f.required)
  const wveMissingQth =
    fdActive &&
    qthSlot !== undefined &&
    fdTypedCall !== '' &&
    logEntity !== null &&
    WVE_ENTITIES.has(logEntity) &&
    fdValue(qthSlot).trim() === ''
  // The SCP line for what is in the Call box: calls already in this log first, then the list's
  // partial matches, then its one-character-different ones. A scan per render, zero IPC.
  const scpHits =
    scpOn && scpCalls.length > 0
      ? scpLine(logCall, scpCalls, (fieldDay?.log ?? []).map((q) => q.call.toUpperCase()))
      : []
  /** A click on the SCP line puts that call in the Call box, as typing it would, and leaves
   *  the caret there. Nothing is ever put in the box from the line on its own. */
  const pickScp = (call: string) => {
    setLogCall(call)
    requestAnimationFrame(() => callInputRef.current?.focus({ preventScroll: true }))
  }

  // ⭐ ENTER SENDS MESSAGE (`features/esm.ts`; its rules signed by the operator, 2026-10-09). With
  // the cockpit's switch on, Enter in this strip's boxes sends the contact's next message through
  // the COCKPIT's own send path (`esm.send`, the one its F-keys use), and the press for the
  // contact's last step logs it through `logIt`, the one log path, once that path has taken the
  // message (decision 1). The message is expanded and taken BEFORE `logIt`'s reset clears the
  // strip, so a message that names his call names the one in the strip (rule 7).
  //
  // ⛔ IT NEVER TURNS TX ON, AND A REFUSED PRESS LOGS NOTHING. Every refusal (the step's, the set's,
  // the cockpit's transmit guards, then the send path's own answer) comes before `logIt`. With ESM
  // off, stepping aside, or on the hosted page, Enter is what it always was (`onEnter`). The caret
  // moves only between this strip's own boxes, on a press made in them, with `preventScroll`: the
  // Space-PTT incident was a focus change in this shared entry code.

  /** A box as ESM reads it: complete only with what the operator gave for THIS contact — typed,
   *  picked or grabbed since the strip last cleared, an untouched default report, a blank
   *  optional box, or a fill (call history's, this contest's last contact's, or the log's), which
   *  counts as copied (rule 10): a station already worked on this band and mode gets no fill but
   *  the file's, so no fill can make a dupe nobody copied. A value the strip
   *  carried over from the last contact is not one: Field Day's class and section stay in their
   *  boxes after a contact is logged, and read as complete they would make the first Enter on a
   *  new call in S&P send the exchange and log the last station's. */
  const esmGiven = (f: ContestFieldSpec): boolean => {
    const v = fdValue(f)
    if (!fdFieldOk(f, v, fdLineDomain(f), logEntity)) return false
    const fill = fillRef.current
    return (
      fill.typed[f.key] === true ||
      fill.filled[f.key]?.value === v ||
      (f.kind === 'rst' && v === rstDefault) ||
      (v.trim() === '' && !f.required)
    )
  }
  /** What ESM judges a press on: the strip's call, its exchange, whether a box still shows its
   *  call-history fill (the box's **history** mark, below), and the dupe verdict. */
  const esmStrip = (): EsmStrip => ({
    call: logCall.trim().toUpperCase(),
    exchangeComplete: fdReceives.every(esmGiven),
    fromHistory: fdReceives.some((f) => fillRef.current.filled[f.key]?.value === fdValue(f)),
    dupe: fdDupe,
  })
  // The dock's plate and highlight follow what the strip holds.
  const stripNow = esmLive ? esmStrip() : null
  const stripKey = stripNow
    ? `${stripNow.call}|${stripNow.exchangeComplete}|${stripNow.fromHistory}|${stripNow.dupe}`
    : ''
  useEffect(() => {
    esm?.onStrip(stripNow)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [stripKey])
  useEffect(
    () => () => esm?.onStrip(null),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [],
  )

  // The last message a press sent, until its over ends: what a stop may have cut.
  const esmSent = useRef<{ role: EsmRole; call: string; logged: boolean; seen: boolean } | null>(null)
  const onAir = isOnAir(snap.radio)
  useEffect(() => {
    const sent = esmSent.current
    if (!sent) return
    if (onAir) sent.seen = true
    else if (sent.seen) esmSent.current = null
  }, [onAir])
  /** WHAT COUNTS AS A STOP (decision 3): any Esc, Stop TX, the keyer's ■ Stop (the cockpit counts
   *  them in `esm.stops`) or the TX latch falling, the watchdog's among them. What went out to
   *  the call in the strip then counts as not sent, so the next Enter sends it again rather than
   *  logging (rule 8). A stopped last message stays logged, and the strip says so (rule 8, as
   *  changed by the operator's D8). Nothing here stops anything: the cockpit's stop already has. */
  const esmStopped = () => {
    if (!esmLive || !esm) return
    const call = logCall.trim().toUpperCase()
    if (call) esm.dispatch('stopped')
    const sent = esmSent.current
    esmSent.current = null
    if (!sent) return
    const step = esmStepName(sent.role)
    if (sent.logged) {
      setEsmLine({
        text: t('contest.esm.stopped.logged', { step, call: sent.call }),
        alert: true,
        about: sent.call,
      })
    } else if (sent.call === call) {
      setEsmLine({ text: t('contest.esm.stopped.again', { step }), alert: true })
    }
  }
  const stopsSeen = useRef(esm?.stops ?? 0)
  useEffect(() => {
    const n = esm?.stops ?? 0
    if (n === stopsSeen.current) return
    stopsSeen.current = n
    esmStopped()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [esm?.stops])
  const latchUp = useRef(snap.radio.txEnabled === true)
  useEffect(() => {
    const was = latchUp.current
    latchUp.current = snap.radio.txEnabled === true
    if (was && !latchUp.current) esmStopped()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [snap.radio.txEnabled])

  const esmCaret = (caret: EsmCaret) => {
    if (caret === 'call') {
      callInputRef.current?.focus({ preventScroll: true })
    } else if (caret === 'ex0') {
      const box = fdBoxRefs.current[fdReceives[0]?.key ?? ''] ?? null
      box?.focus({ preventScroll: true })
      box?.select()
    }
  }
  // One press, one message: a press still being taken holds the next one off.
  const esmBusy = useRef(false)
  const pressEsm = async (e: React.KeyboardEvent) => {
    if (!esm || e.repeat || esmBusy.current) return
    esmLineKey.current = e.nativeEvent
    setEsmLine(null)
    const strip = esmStrip()
    const decision = esmPress({
      state: esm.state,
      strip,
      callOnce: esm.callOnce,
      guards: esm.guards(),
      roles: esm.roles,
      slots: esm.slots,
    })
    if (decision.kind === 'inert') {
      void logIt()
      return
    }
    if (decision.kind === 'refuse') {
      setEsmLine({ text: esmRefusalText(decision.refusal), alert: true })
      return
    }
    if (decision.kind === 'speak') {
      esm.dispatch(decision.next)
      esmCaret(decision.caret)
      setEsmLine({ text: t('contest.esm.phone.speak'), alert: false })
      return
    }
    if (decision.kind === 'log') {
      // QSO B4 has gone to this dupe: the press logs it through the one log path, sending nothing,
      // and the log's clear is a new contact for ESM too. A press still logging holds the next off.
      esmBusy.current = true
      try {
        await logIt()
      } finally {
        esmBusy.current = false
      }
      esmCaret(decision.caret)
      return
    }
    esmBusy.current = true
    let why: string | null
    try {
      why = await esm.send({ role: decision.role, keys: decision.keys, text: decision.text, call: strip.call })
    } catch (err) {
      why = err instanceof Error ? err.message : String(err)
    } finally {
      esmBusy.current = false
    }
    if (why !== null) {
      setEsmLine({ text: t('contest.esm.notSent', { why }), alert: true })
      return
    }
    esm.dispatch(decision.next)
    esmSent.current = { role: decision.role, call: strip.call, logged: decision.log, seen: false }
    if (decision.log) await logIt()
    esmCaret(decision.caret)
  }
  // The band change's notice (`bandCleared`), in both layouts.
  const bandClearedNotice = bandCleared && (
    <p className="le-contest-resume le-band-cleared" role="status">
      {bandCleared.line.call.trim()
        ? t('logEntry.bandCleared', {
            call: bandCleared.line.call.trim().toUpperCase(),
            band: bandCleared.context.band || snap.radio.band,
          })
        : t('logEntry.bandCleared.noCall', { band: bandCleared.context.band || snap.radio.band })}{' '}
      {restoreBlocked && <>{t('logEntry.bandCleared.contestBand', { band: bandCleared.context.band })} </>}
      <button type="button" className="export-btn" onClick={restoreBandCleared} disabled={restoreBlocked}>
        {t('logEntry.bandCleared.restore')}
      </button>{' '}
      <button type="button" className="export-btn" onClick={() => setBandCleared(null)}>
        {t('common.dismiss')}
      </button>
    </p>
  )

  if (fdActive) {
    return (
      <div className="log-entry log-entry-fd">
        <div className="le-fd-header">
          <span className="le-fd-chip">
            {fdEventIsFieldDay ? t('logEntry.fd.chip') : t('logEntry.contest.chip')}
          </span>
          <span className="le-fd-mode">{fdMode ?? 'PH'}</span>
          {noCreditChip}
          <span className="le-fd-hint">
            {fdEventIsFieldDay
              ? t('logEntry.fd.hint', { band: snap.radio.band })
              : offContestBand
                ? t('logEntry.contest.offBand', { band: snap.radio.band })
                : t('logEntry.contest.hint', { band: snap.radio.band })}
          </span>
          {/* ⭐ THE SCORE CHIP — the Contest screen's own numbers at a glance, in the room the
              row has left: a zero-cost slot, so nothing else in this header moves for it. */}
          {fieldDay && scoreChip && <ContestScoreChip fieldDay={fieldDay} />}
          {/* The operator box, right beside the sent side (only the large log line passes one). */}
          {operatorBox}

          {/* ⭐ THE SENT SIDE, READ-ONLY (§9) — what is going on the air right now.
              It is the SESSION's composing exchange, never a row's: a row already
              logged carries its own, and `sent_exchange(row, spec)` is the only
              thing that renders one.

              ⚠️ IT IS IN THE HEADER ROW, AND §9 SAYS "beside the entry boxes". That
              is a DEVIATION, and it is the harness's verdict rather than a
              preference. Built in the entry row and measured there
              (`ui/layout-harness/entry-strip.html`, headless Chrome, 2026-09-09):
              at the 1024 px supported floor, Field Day's two boxes, the block fits
              the line — by taking the slack out of the CALL box, 506.7 px → 207.8 px.
              Widen it back and the row wraps instead, and this dock is
              bottom-anchored, so the strip's extra line comes off the log feed and
              moves the very fields the fixed-height verdict slot exists to keep
              still. In the header row — one line above the boxes it describes, inside
              the same strip — the whole block costs 13 px (117 → 130) at every width,
              and Call keeps all 506.7. jsdom reports none of those numbers, because
              every rect there is 0. */}
          {fdComposing.length > 0 && (
            <div className="le-fd-sent" aria-label={t('logEntry.sent.aria')}>
              <span className="le-fd-sent-cap">
                {t('logEntry.sent.label')}
                {/* The ROLE, beside the exchange, so an operator can see which one
                    they are in before they cross a line that ends the session. Blank
                    for a symmetric contest — both Field Day events — and shown only
                    when it names something. In words (in-state, W/VE…), never its id. */}
                {fieldDay?.role ? <span className="le-fd-role"> {contestRoleWords(fieldDay.role)}</span> : null}
              </span>
              {movingTo === null ? (
                <>
                  <span className="mono le-fd-sent-val" title={t('logEntry.sent.title')}>
                    {fdSending}
                  </span>
                  <button
                    type="button"
                    className="le-qrz le-fd-moved"
                    onClick={() =>
                      setMovingTo(Object.fromEntries(fdComposing.map((v) => [v.key, v.raw])))
                    }
                    title={t('logEntry.sent.moved.title')}
                  >
                    {t('logEntry.sent.moved.label')}
                  </button>
                </>
              ) : (
                <>
                  {fdComposing.map((v) => (
                    <input
                      key={v.key}
                      className="settings-input mono le-fd-moved-input"
                      value={movingTo[v.key] ?? ''}
                      onChange={(e) =>
                        setMovingTo((prev) => ({ ...prev, [v.key]: upperTyped(e.target) }))
                      }
                      // Enter commits and Escape backs out, because a contest
                      // operator's hands are on the keyboard and a two-box edit that
                      // needs the mouse to finish is one they will not use mid-run.
                      onKeyDown={(e) => {
                        if (e.key === 'Enter') void commitMove()
                        else if (e.key === 'Escape') setMovingTo(null)
                      }}
                      aria-label={fdFieldLabel(v.key)}
                      autoComplete="off"
                      spellCheck={false}
                    />
                  ))}
                  <button type="button" className="le-log-btn" onClick={commitMove}>
                    {t('logEntry.sent.moved.save')}
                  </button>
                  <button type="button" className="le-qrz" onClick={() => setMovingTo(null)}>
                    {t('logEntry.sent.moved.cancel')}
                  </button>
                </>
              )}
            </div>
          )}
        </div>
        {/* ⭐ THIS STATION IS ABOUT TO SEND THE DX EXCHANGE, and its call is in the USA or
            Canada — a blank or unlisted contest state. Prominent, and never a refusal: a US
            call operating from abroad really is DX, so the Log button is untouched. It is a
            persistent condition rather than a per-keystroke verdict, so its own line here
            rather than the fixed-height verdict slot. */}
        {fieldDay?.locationWarning ? (
          <div className="fd-section-warn le-fd-location-warn" role="alert">
            {locationWarningText(fieldDay.locationWarning)}
          </div>
        ) : null}
        {/* The contest journal's writes are failing: the contacts since the last good one are in
            memory only. Its own line, red, until a write lands. */}
        {fieldDay?.unsaved ? (
          <div className="fd-section-warn fd-unsaved" role="alert">
            {contestUnsavedText(fieldDay.unsaved)}
          </div>
        ) : null}

        {editRows.length > 0 ? (
          <ContestContactEdit
            // One edit per contact: Ctrl+Y on another newest contact opens that one afresh.
            key={`${editRows[0].call}|${editRows[0].whenUnix}`}
            rows={editRows}
            receives={fdReceives}
            role={fieldDay?.role}
            bands={fieldDay?.bands}
            onSaving={() => {
              saving.current = true
            }}
            onDone={editDone}
          />
        ) : (
        <div className="le-fd-big">
          <label className="le-fd-field le-fd-field-call">
            {/* ⭐ THE DUPE BADGE rides the Call caption's own line, so it adds no height and sits
                nowhere near Stop or the ESM switch. The live region is always there, empty for a
                new call, so a screen reader hears it fill. */}
            <span className="le-fd-cap le-fd-cap-marked">
              <span className="le-fd-cap-text">{t('logEntry.fd.call.label')}</span>
              <span
                className={`le-fd-dupe${fdWorkedLine ? ' le-fd-dupe-maybe' : ''}`}
                aria-live="polite"
                title={fdDupeWorked || fdWorkedLine || undefined}
              >
                {fdDupeRow !== undefined && (
                  <>
                    <span className="le-fd-dupe-word">{t('logEntry.fd.dupe.badge')}</span>{' '}
                    {fdDupeWorked}
                  </>
                )}
                {fdWorkedLine}
              </span>
              {/* ⭐ THE COUNTRY of the call being typed, as the contest places it (`contestPlace`), with
                  the CQ zone its prefix usually has where the contest receives one (`zoneHint`, the
                  zone box's placeholder). Read-only: nothing here goes into the exchange, and the zone a
                  station sends is still copied. On this same caption line, after the dupe badge,
                  which it gives way to first, so it adds no height and no width in any host. Outside
                  the live region, so a screen reader is not read a country per keystroke. Not in the
                  Field Day events, where every station is W/VE. */}
              {!fdEventIsFieldDay && fdCountry && (
                <span
                  className="le-fd-country"
                  title={fdCountryZone ? t('logEntry.contest.country.zoneTitle') : t('logEntry.contest.country.title')}
                >
                  {fdCountry}
                </span>
              )}
            </span>
            <input
              ref={callInputRef}
              className={`settings-input mono le-fd-input le-fd-input-call${fdDupeRow !== undefined ? ' le-fd-input-dupe' : fdWorkedLine ? ' le-fd-input-maybe' : ''}`}
              value={logCall}
              // SPACES ARE DROPPED, not kept: no callsign contains one, and the space bar is
              // the phone position's push-to-talk. An operator who reaches for it out of habit
              // while the caret is in this field would otherwise log "K1 ABC" verbatim —
              // `logIt` only trims the ends. Costs nothing on any other path (a pasted call
              // with a stray space comes out clean).
              onChange={(e) => setLogCall(normaliseTyped(e.target, (s) => s.replace(/\s+/g, '').toUpperCase()))}
              onBlur={onCallBlur}
              onKeyDown={(e) => {
                // SPACE walks Call → field₁, whatever field₁ is; Shift+Space back to the last.
                onExchangeSpace(
                  e,
                  fdBoxRefs.current[fdReceives[0]?.key ?? ''] ?? null,
                  fdBoxRefs.current[fdReceives[fdReceives.length - 1]?.key ?? ''] ?? null,
                )
                onEnter(e)
              }}
              autoComplete="off"
              spellCheck={false}
            />
          </label>
          {/* ONE BOX PER RECEIVED SLOT, in receive order. Space walks each to the next
              and the last one back to Call, which is the loop the shipped strip has:
              Call → Class → Section → Call. */}
          {fdReceives.map((f, i) => {
            // ⭐ MARKED WITH WHERE THE FILL CAME FROM for exactly as long as the box shows the
            // filled value: typing over it, or the strip clearing, takes the mark away.
            const fromHistory = fillMarks[f.key]?.value === fdValue(f)
            const fillSource = fillMarks[f.key]?.source
            // What the box holds for this station's role: "County / State" in Illinois.
            const caption = receivedCaption(f, fieldDay?.role)
            // The "RST sent" box stands just before the received report, sent then received, as
            // N1MM lays out Snt and Rcv (before the first box, for a contest that receives none).
            const sentHere = sentRstConst !== undefined && i === Math.max(0, fdReceives.findIndex((r) => r.kind === 'rst'))
            // Where Shift+Space goes from this box (and from the "RST sent" box before it): the box
            // before it, or Call from the first. Read at the key press, as the refs are.
            const prevBox = () => (i > 0 ? (fdBoxRefs.current[fdReceives[i - 1].key] ?? null) : callInputRef.current)
            return (
            <Fragment key={f.key}>
            {sentHere && (
              <label className="le-fd-field le-fd-field-sent">
                <span className="le-fd-cap">{t('logEntry.fd.rstSent.label')}</span>
                <input
                  ref={sentRstRef}
                  className={`settings-input mono le-fd-input le-fd-input-code${sentRstGood ? '' : ' invalid'}`}
                  // ⭐ SPACE AND TAB SKIP IT, as N1MM's space bar skips Snt: Space walks Call to the
                  // received boxes by their own refs, and Tab passes over a -1. A click reaches it,
                  // and Shift-Tab from the received report (below).
                  tabIndex={-1}
                  value={sentRstValue}
                  aria-invalid={!sentRstGood}
                  onChange={(e) => {
                    noteTyped(SENT_RST_KEY)
                    setFdField(SENT_RST_KEY, normaliseTyped(e.target, (s) => s.replace(/\s+/g, '')))
                  }}
                  onKeyDown={(e) => {
                    onExchangeSpace(e, fdBoxRefs.current[f.key] ?? null, prevBox())
                    onEnter(e)
                  }}
                  placeholder={sentRstDefault}
                  autoComplete="off"
                  spellCheck={false}
                  title={t('logEntry.fd.rstSent.title', { rst: sentRstDefault })}
                />
              </label>
            )}
            <label className="le-fd-field">
              {fromHistory ? (
                <span className="le-fd-cap le-fd-cap-marked">
                  <span className="le-fd-cap-text">{caption}</span>
                  <span className="le-fd-from-history">{fillMark(fillSource)}</span>
                </span>
              ) : (
                <span className="le-fd-cap">{caption}</span>
              )}
              <input
                ref={(el) => {
                  fdBoxRefs.current[f.key] = el
                }}
                className={`settings-input mono le-fd-input le-fd-input-code${
                  fromHistory ? ' le-fd-input-history' : ''
                }`}
                value={fdValue(f)}
                onChange={(e) => {
                  noteTyped(f.key)
                  setFdField(f.key, upperTyped(e.target))
                  // The list is offered from every universe this slot draws on, which for
                  // a QSO party's one QTH box is counties AND states — and, inside a county
                  // line, for the part being typed and from the county list alone.
                  const line = fdLineDomain(f)
                  setFdHits({
                    key: f.key,
                    values:
                      line && isCountyLine(e.target.value)
                        ? domainSuggestions([line], lastCountyPart(e.target.value))
                        : domainSuggestions(f.domains, e.target.value),
                  })
                }}
                onKeyDown={(e) => {
                  const next = fdReceives[i + 1]
                  // SPACE leaves the box, so it is where a typed name becomes its code —
                  // before the eye moves on, and before Enter can log anything.
                  if (e.key === ' ' || e.code === 'Space' || e.key === 'Tab') commitFdField(f)
                  // Shift-Tab from the received report reaches the "RST sent" box beside it, which
                  // plain Tab order passes over.
                  if (e.key === 'Tab' && e.shiftKey && sentHere && sentRstRef.current) {
                    e.preventDefault()
                    sentRstRef.current.focus()
                    sentRstRef.current.select()
                  }
                  onExchangeSpace(
                    e,
                    next ? (fdBoxRefs.current[next.key] ?? null) : callInputRef.current,
                    prevBox(),
                  )
                  onEnter(e)
                }}
                onBlur={() => {
                  commitFdField(f)
                  // …after any click on the list itself has been taken (mouse-down picks).
                  window.clearTimeout(fdHitsCloseTimer.current)
                  fdHitsCloseTimer.current = window.setTimeout(closeFdHits, 150)
                }}
                placeholder={f === zoneSlot && zoneHint ? zoneHint : undefined}
                autoComplete="off"
                spellCheck={false}
                title={
                  fromHistory
                    ? fillTitle(fillSource, historyFile?.fileName ?? '')
                    : slotTitle(f.key)
                }
              />
              {fdHits.key === f.key && fdHits.values.length > 0 && (
                <ul className="le-fd-suggest">
                  {fdHits.values.map((v) => (
                    <li key={v.code}>
                      <button
                        type="button"
                        onMouseDown={(e) => {
                          e.preventDefault() // pick before the input's onBlur closes the list
                          // Inside a county line the pick completes the part being typed.
                          const line = fdLineDomain(f)
                          noteTyped(f.key)
                          setFdField(
                            f.key,
                            line && isCountyLine(fdValue(f)) ? withLastCountyPart(fdValue(f), v.code) : v.code,
                          )
                          closeFdHits()
                        }}
                      >
                        <span className="mono le-fd-hit-code">{v.code}</span>
                        <span className="le-fd-hit-name">{v.name}</span>
                      </button>
                    </li>
                  ))}
                </ul>
              )}
            </label>
            </Fragment>
            )
          })}
          {/* No `gridBlocked` term, and that is not an omission: `asksForGrid`
              is false whenever `fdActive` is, so `logIt`'s grid guard — which
              sits above the FD branch — is provably inert on this path. A layout
              with no Grid input can never be held by grid state. */}
          <button
            type="button"
            className="le-log-btn le-fd-log-btn"
            onClick={logIt}
            disabled={!logCall.trim() || !fdExchangeOk || !sentRstGood}
          >
            {fdEventIsFieldDay ? t('logEntry.fd.log') : t('logEntry.log')}
          </button>
          <button
            type="button"
            className="le-qrz"
            onClick={() => reset()}
            title={t('logEntry.clear.title')}
          >
            {t('logEntry.clear.label')}
          </button>
          {removable && (
            <button
              type="button"
              className="le-qrz le-fd-remove"
              // ⛔ THE CARET STAYS WHERE IT WAS: a click never takes focus (see `pressRemove`).
              onMouseDown={(e) => e.preventDefault()}
              onClick={pressRemove}
              disabled={!newest}
              aria-pressed={removeArmed != null}
              title={t('logEntry.remove.title')}
            >
              {t('logEntry.remove.label')}
            </button>
          )}

        </div>
        )}

        {/* ⭐ SUPER CHECK PARTIAL — one line under the boxes, there whenever SCP is on so the
            strip does not grow and shrink as calls come and go (it is a `fit="content"` pane,
            so its height is its content). A click puts the call in the Call box; nothing is put
            there from this line on its own, and a call missing from it is not an error. A log
            aid, not a transmit control. */}
        {scpOn && (
          <div className="le-scp" role="group" aria-label={t('logEntry.scp.label')} title={t('logEntry.scp.title')}>
            {scpCalls.length === 0 ? (
              <span className="le-scp-empty">{t('logEntry.scp.none')}</span>
            ) : (
              scpHits.map((h) => (
                <button
                  key={h.call}
                  type="button"
                  className={`le-scp-call le-scp-${h.kind}`}
                  // The caret stays in the Call box: a press here is a pick, not a move.
                  onMouseDown={(e) => e.preventDefault()}
                  onClick={() => pickScp(h.call)}
                  title={
                    h.kind === 'worked'
                      ? t('logEntry.scp.worked.title')
                      : h.kind === 'near'
                        ? t('logEntry.scp.near.title')
                        : undefined
                  }
                >
                  {h.call}
                </button>
              ))
            )}
          </div>
        )}

        {/* THE VERDICT SLOT — always present, empty or not.

            The dock this strip lives in is bottom-anchored, so anything that makes the strip
            taller moves the fields UP, and these three verdicts appear and vanish PER
            KEYSTROKE right under the fingers that are typing. Reserving the height here rather
            than on the strip is the difference between a reservation and a coincidence: a floor
            on `.log-entry-fd` has to be re-derived every time the header row or the field row
            changes height, and the one this shipped with (8.5em = 119px against a 117px natural
            strip) reserved two pixels of an eleven-pixel line — i.e. nothing. The wrapper is
            zero-height in every other host, so Phone and CW are unchanged. */}
        {bandClearedNotice}
        <div className="le-fd-verdicts">
        {removeLine != null && (
          <div className="le-fd-hint le-fd-remove-line" role={removeArmed || removeNote?.alert ? 'alert' : 'status'}>
            {removeLine}
          </div>
        )}
        {removeLine == null && esmLine != null && (
          <div className="le-fd-hint le-fd-esm-line" role={esmLine.alert ? 'alert' : 'status'}>
            {esmLine.text}
          </div>
        )}
        {slotFree && logCall.trim() !== '' && fdBadField !== undefined && (
          <div className="le-fd-hint" role="alert">
            {fdVerdict(fdBadField, fdValue(fdBadField), fdLineDomain(fdBadField), fieldDay?.role)}
          </div>
        )}
        {slotFree && fdOwnDupe && (
          <div className="le-fd-hint" role="alert">
            {/* FOUR literal call sites rather than one with a computed key: the i18n
                extractor reads keys statically and the placeholder guard compares each
                site's params against its entry, and a ternary INSIDE `t()` hides both
                from them — the entries read as orphans nobody references, and a band
                passed to a band-free sentence stops being a CI failure. Which is the
                whole point here: the sites that drop a component drop its VALUE too. */}
            {fdDupeByBand
              ? fdDupeByModeClass
                ? t('logEntry.fd.dupe.own', {
                    call: fdTypedCall,
                    band: snap.radio.band,
                    mode: fdModeClass,
                  })
                : t('logEntry.fd.dupe.ownAnyMode', {
                    call: fdTypedCall,
                    band: snap.radio.band,
                  })
              : fdDupeByModeClass
                ? t('logEntry.fd.dupe.ownAnyBand', {
                    call: fdTypedCall,
                    mode: fdModeClass,
                  })
                : t('logEntry.fd.dupe.ownAnyBandOrMode', { call: fdTypedCall })}
            {fdLineDupes && t('logEntry.countyLine.dupeFor', { counties: countyList(fdLineOwn) })}
          </div>
        )}
        {/* Some counties of a line already worked: which, and which are new. The contact still
            scores in the new ones, so this is not the line's "Dupe:" sentence. */}
        {slotFree && !fdOwnDupe && fdLineOwn.length > 0 && (
          <div className="le-fd-hint" role="alert">
            {t('logEntry.countyLine.someDupe', { counties: countyList(fdLineOwn), count: fdLineOwn.length })}
            {t('logEntry.countyLine.someNew', { counties: countyList(fdLineNew), count: fdLineNew.length })}
          </div>
        )}
        {slotFree && fdClubDupe && (
          <div className="le-fd-hint" role="status">
            {t('logEntry.fd.dupe.club', {
              call: fdTypedCall,
              band: snap.radio.band,
              mode: fdModeClass,
            })}
            {fdLineDupes && t('logEntry.countyLine.dupeFor', { counties: countyList(fdLineCounties((d) => d === 'club')) })}
          </div>
        )}
        {slotFree && wveMissingQth && (
          <div className="le-fd-hint" role="status">
            {t('logEntry.contest.qthMissing', { call: fdTypedCall })}
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
  const huntMatches = hunt != null && logCall.trim() !== '' && sameCall(hunt.call, logCall)
  // THE TAG'S ✕ ENDS THE HUNT with the POTA/SOTA view's banner ✕'s own call, and hands up the
  // snapshot it answered as that view does, so the tag and the banner go at once. Clear could not
  // end it: the hunt's prefill fills the emptied strip again. The park that prefill put in the box
  // goes with the hunt; a park the operator typed, or one a Needed click handed over (`workPark`,
  // the prefill's source for its own station), is theirs and stays.
  const endHunt = async (huntRef: string) => {
    const s = await withErrorToast(() => clearHuntTarget(), t('ota.hunt.clearFailed'))
    if (!s) return
    const handed = workPark != null && sameCall(workPark.call, logCall.trim())
    if (parkForRef.current.hunt && !handed && parkBoxRef.current.trim().toUpperCase() === huntRef.trim().toUpperCase()) {
      setPark('')
    }
    onSnap?.(s)
    pushToast(t('ota.hunt.cleared'), 'info', 2000)
  }

  return (
    <div className="log-entry" onChangeCapture={remoteMode ? rememberRemoteContext : undefined}>
      {titled && <h2>{t('logEntry.title')}</h2>}
      {onResumeContest && (
        <p className="le-contest-resume" role="status" aria-label={t('logEntry.contest.notRunning.aria')}>
          {t('logEntry.contest.notRunning')}{' '}
          <button type="button" className="export-btn" onClick={onResumeContest}>
            {t('logEntry.contest.resume')}
          </button>
        </p>
      )}
      {bandClearedNotice}
      {remoteMode && remoteWorkOffer && <div>
        <p role="status">{t('remote.workDraftKept', { call: remoteWorkOffer.call })}</p>
        <div className="remote-actions"><button type="button" className="remote-button" disabled={remote?.pending}
          onClick={() => {
            if (remote?.pending) return
            const next = remoteWorkOffer
            reset()
            setRemoteWorkOffer(null)
            setOverrideOpen(false)
            setRemoteDraftContext(next.context)
            setLogCall(next.call.toUpperCase())
            humanCallEditRef.current = false
            settledCallRef.current = true
          }}>{t('remote.workReplaceDraft', { call: remoteWorkOffer.call })}</button></div>
      </div>}

      {hunt && (
        <div className={`le-hunt-chip${huntMatches ? ' match' : ''}`} title={t('logEntry.hunt.title')}>
          🌲 {hunt.program} {hunt.reference}
          <span className="le-hunt-for"> · {hunt.call}</span>
          {!huntMatches && logCall.trim() !== '' && (
            <span className="le-hunt-warn"> {t('logEntry.hunt.mismatch')}</span>
          )}
          <button
            type="button"
            className="le-hunt-clear"
            onClick={() => void endHunt(hunt.reference)}
            title={t('ota.hunt.clear')}
            aria-label={t('ota.hunt.clear')}
          >
            <X aria-hidden="true" />
          </button>
        </div>
      )}

      <IotaOfferChip state={iotaOffer} />

      <div className="le-row">
        <input
          ref={callInputRef}
          className="settings-input mono le-call"
          value={logCall}
          onChange={(e) => {
            humanCallEditRef.current = true
            const next = upperTyped(e.target)
            // #383: a park the operator typed follows the call as they type it. Typed before any
            // call, it belongs to the first call typed after it; typed mid-call, to the call being
            // finished. Only a keystroke here moves it, so a clicked spot is a new station.
            const bound = parkForRef.current
            if (!bound.hunt && (bound.call === '' || bound.call === baseCall(logCall))) {
              parkForRef.current = { call: baseCall(next), hunt: false }
            }
            setLogCall(next)
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
          disabled={remoteMode || qrzBusy || !logCall.trim()}
          title={t('logEntry.lookup.title')}
        >
          {qrzBusy ? '…' : t('logEntry.lookup.label')}
        </button>
        {/* The call's station card (CallCard.tsx). Never in the tab order and never takes focus on a
            press, so typing in the call field is never interrupted; this strip has no ESM, and the
            contest strip has no such button. Never an F-key: F1–F12 are the transmit macros. */}
        <button
          type="button"
          className="le-qrz le-lookup le-card"
          tabIndex={-1}
          onMouseDown={(e) => e.preventDefault()}
          disabled={!logCall.trim()}
          {...callCardTrigger(logCall)}
        >
          {t('logEntry.card.label')}
        </button>
        <label className="le-rst-field" title={t('logEntry.rstSent.title')}>
          <span className="le-rst-cap">{t('logEntry.rstSent.label')}</span>
          <input
            ref={rstRef}
            className="settings-input mono le-rst"
            value={rstSentShown}
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
            value={rstRcvdShown}
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
            onChange={(e) => setLogGrid(upperTyped(e.target))}
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
        <button type="button" className="le-qrz" onClick={() => reset()} title={t('logEntry.clear.title')}>
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
          onChange={(e) => setStateBox(e.target.value, 'operator')}
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
            onChange={(e) => {
              setPark(upperTyped(e.target))
              parkForRef.current = { call: baseCall(logCall), hunt: false }
            }}
            onKeyDown={onEnter}
            onBlur={() => window.setTimeout(() => setParkHits([]), 150)}
            placeholder={
              logParkProgram === 'SOTA' ? t('logEntry.park.ref.placeholderSota') : t('logEntry.park.ref.placeholderPota')
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
                      setPark(p.reference)
                      parkForRef.current = { call: baseCall(logCall), hunt: false }
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

      {/* A PARK ON A STATE LINE: which of its states the activator is in is never guessed. Not
          the licence's state (where the activator lives) and not the grid's (a square straddles
          the line). The operator picks one, from what the activator says on the air or the
          spot's comment; with no pick the contact logs no state and the Logbook flags it. In
          every exchange, as the hunt chip is: the station places a hunted park by call. */}
      {parkPlaces.length > 1 && (
        <div className="le-park-states" role="group" aria-label={t('logEntry.parkState.ask')} title={t('logEntry.parkState.title')}>
          <span className="le-park-states-ask">{t('logEntry.parkState.ask')}</span>
          {parkPlaces.map((place) => {
            const code = placeCode(place)
            const picked = logState.trim().toUpperCase() === code
            return (
              <button
                key={place}
                type="button"
                className={`le-park-state${picked ? ' picked' : ''}`}
                aria-pressed={picked}
                title={placeName(place)}
                onClick={() => setStateBox(code, 'operator')}
              >
                {code}
              </button>
            )
          })}
        </div>
      )}

      {/* THE SHARED COMMENT: its own full-width box, always directly above Notes. It was the
          fourth field of the QTH row, where a wrapped line squeezed it to a few words (operator,
          1.21.0). It wraps over several lines but holds ONE: ADIF's COMMENT is a String and
          NOTES the multi-line field, so a pasted line break lands as a space, and Enter logs
          the contact, as it always did from this box. */}
      <textarea
        className="settings-input le-comment"
        value={logComment}
        onChange={(e) => setLogComment(oneLineTyped(e.target))}
        onKeyDown={(e) => {
          if (e.key === 'Enter') e.preventDefault()
          onEnter(e)
        }}
        placeholder={t('logEntry.comment.placeholder')}
        rows={2}
        autoComplete="off"
        spellCheck
      />

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
              {/* UTC as typed — see `ovDateBad`. */}
              <input
                className={`settings-input mono le-ov-date${ovDateBad ? ' invalid' : ''}`}
                value={ovDate}
                onChange={(e) => setOvDate(e.target.value)}
                placeholder={UTC_DATE_FORMAT}
                maxLength={10}
                spellCheck={false}
                autoComplete="off"
                aria-invalid={ovDateBad}
              />
            </label>
            <label className="le-ov-field">
              <span className="le-rst-cap">{t('logEntry.override.time.label')}</span>
              {/* 24-hour UTC as typed — a native time control would draw it in the OS locale (#280). */}
              <input
                className={`settings-input mono le-ov-time${ovTimeBad ? ' invalid' : ''}`}
                value={ovTime}
                onChange={(e) => setOvTime(e.target.value)}
                placeholder={UTC_TIME_FORMATS.short}
                maxLength={8}
                spellCheck={false}
                autoComplete="off"
                aria-invalid={ovTimeBad}
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

      {/* THE COMMIT BLOCK: the "logs to … as" line and the Log/Spot row below it, as one block. In a
          cockpit's log pane it sticks to the bottom of the pane's own scroller (`.le-commit`,
          styles.css), so Log stays on screen while the form overflows. See the commit row below. */}
      <div className="le-commit">
      <span className="le-hint">
        {overrideBlocked ? (
          <span className="le-ov-warn">
            {ovFreqBlocked
              ? t('logEntry.override.blockedHint')
              : t('logEntry.override.timeBlockedHint', { date: UTC_DATE_FORMAT, ...UTC_TIME_FORMATS })}
          </span>
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
          it — that adjacency is why spotting happens at all. The pane body is the scroller; this
          row owns no scrolling of its own.

          A sticky Log row WAS PROPOSED (2026-08-04 density review) and refused: an opaque sticky
          row would park Log over the recall card that follows it — its QRZ link included — and
          cut Log loose from the "logs to …" line above it. Deleting chrome (the heading and the
          card-in-card padding, 43px, LogEntry.density.test) answered the fold then. It did not
          answer it at 1280×800: in all five cockpits the form still needs more height than the
          log pane gets, and no legal sizing lever closes the gap (measured 2026-10-19).
          So the operator ruled (2026-10-19) for the bottom-sticky COMMIT BLOCK, which answers
          both reasons: `.le-commit` wraps the line AND this row, so they never part; and
          `bottom: 0` only ever moves the block UP from its natural place, while the recall card
          sits BELOW that place — when the card scrolls into view the block is back in flow above
          it, so the card is never covered. What scrolls under the block is the form above it
          (notes, the override), and only while it is scrolled out past the pane's bottom edge.
          Sticky inside the pane body's existing `overflow: auto` adds no second scroller, and
          its parent `.log-entry` bounds it, so it never rides over Recent below the form. */}
      <div className="le-row le-actions">
        <button
          type="button"
          className="le-log-btn"
          onClick={logIt}
          disabled={!logCall.trim() || overrideBlocked || gridBlocked || (remote != null && (!remote.canSubmit || remote.busy || remote.pending))}
          title={
            ovFreqBlocked
              ? t('logEntry.override.blocked')
              : overrideBlocked
                ? t('logEntry.override.timeBlocked', { date: UTC_DATE_FORMAT, ...UTC_TIME_FORMATS })
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
      </div>

      {remote ? remote.recall(logCall, { band: effBand, freqMhz: effFreqMhz, mode: effMode }) : <RecallPanel
        call={logCall}
        band={snap.radio.band}
        name={logName}
        qth={logQth}
        // Already filled from the callbook lookup / cty.dat resolve above (#237). The callbook's own
        // state once the park has placed the box: the card says where the station lives, beside its
        // town, and the box where this contact counts.
        state={stateSource === 'park' ? callbookState : logState}
        grid={logGrid}
        country={logCountry}
        image={logImage}
        myGrid={snap.mygrid}
        hist={hist}
        activating={activating}
        potaDupe={potaDupeNow}
        newEntity={newEntity}
        newBandSlot={newBandSlot}
        newModeSlot={newModeSlot}
        onOpenLog={onOpenLogbook}
      />}
    </div>
  )
}
