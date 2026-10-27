import type { FdDisplaySettings } from '../fieldDayObservation'
import { Fragment, useEffect, useMemo, useRef, useState, type CSSProperties, type ReactNode } from 'react'
import type {
  ClubLink,
  ContestBoard,
  ContestFieldSpec,
  DupeRule,
  FdClubBoardRow,
  FdClubLog,
  FdClubStatus,
  FdMergeReport,
  FdOperatorStat,
  FieldDayQso,
  FieldDayStatus,
  ModeRequest,
  Settings,
} from '../types'
import { openContestLogger } from '../features/contestLoggerLayout'
import { contestClear, contestMergeUnmerged, contestRemoveLast, contestRemoved, contestRestore, contestUnmerged, exportLog, fdClubAllowBack, fdClubExport, fdClubGivePosition, fdClubLogNewest, fdClubRemovePosition, fdMergeToGeneral, fdSetUpload, getFdRuleset, getSettings, setFdOperator, openPanelWindow, saveTextToDownloads, type ContestUnmerged, type FdGiveAnswer, type FdRemoveAnswer, type FdRulesetDto, type RemovedContest } from '../api'
import { isStreamInput } from '../remote-native/stream-input'
import { drivesAnotherStation } from '../features/durableStore'
import { patchSettings } from '../settings/patch'
import { FdAdvisories } from './FdAdvisories'
import { ContestOverDialog, contestOverOffer, hasOffer, type ContestOverOffer } from './ContestOverDialog'
import { ContestSetupWizard, ContestSoonLine } from './ContestSetupWizard'
import { blockerTarget, blockerText, cabrilloLineWords, READ_AS_THE_SESSION_STARTS, startBlocker, type StartBlocker } from '../features/contestStart'
import { WfdObjectivesSection } from './WfdObjectivesSection'
import { WFD_OBJECTIVES, wfdEarnedIds, type WfdObjective } from '../features/wfdObjectives'
import { pushToast } from '../toast'
import { ADDR_SEP, CLUB_DEAD_SECS, CLUB_PING_SECS, clubLinkLines, heardAgo, type ClubLinkLine } from '../features/clubLink'
import { clubSyncRefusal, clubSyncRefusalText, contestFileStem, contestName, contestShortName, fdEventFromWindow, fdHeaderSubtitle, FD_EVENT_NAMES, isFieldDay, type ClubSyncRefusal, type FdKind } from '../fdEvent'
import type { ContestSwitch } from '../contestSwitch'
import { usePinnedScroll } from '../usePinnedScroll'
import { textPx } from '../useTextSize'
import { ARRL_SECTIONS_BY_DIVISION, ARRL_SECTION_TOTAL } from '../features/arrlSections'
import { contestDomain, multCode, multTotal, type DomainGroup } from '../features/contestDomains'
import { composingSlot } from '../features/contestExchange'
import { receivedCaption } from '../features/contestSlots'
import { contestUnsavedText } from '../features/contestUnsaved'
import { closedText } from '../features/contestEdit'
import { contestRate, RATE_WINDOWS } from '../features/contestRate'
import { bandRank, operatorSummaryLines } from '../features/contestOperators'
import { formatOpTime, OFF_TIME_MIN_SEC, offTimeRule, operatingTime } from '../features/contestOpTime'
import { OperatorStatsBoard } from './ContestOperators'
import { stationMode, useKeyboardMode } from '../features/contestMode'
import { CLEAR_TOAST_MS, REMOVE_CONFIRM_MS, SERVICE_NAMES, clearText, contactCount, contactLabel, newestContact, removalText, restoreText, utcHhmm } from '../features/contestRemoval'
import { ContestContactEdit, focusOpenContestEdit, type EditNote } from './ContestContactEdit'
import { ContestSpotMe } from './ContestSpotMe'
import { ContestStats } from './ContestStats'
import { t } from '../i18n'
import type { MessageKey } from '../i18n'
import { T } from '../i18n/T'
import { bandColor } from '../bandColors'

/** ⭐ The score-note keys a RULESET may name (`score_note_key`), and the reason this set
 *  exists rather than the render trusting whatever the rules file says.
 *
 *  Two things at once. A rules file is DOWNLOADED data, so a key this build has no
 *  catalog entry for would render as the raw key string on the operator's scoreboard —
 *  the set makes an unknown key render nothing instead. And the catalog's own
 *  "leaves no orphan entry behind" guard scans SOURCE for key literals: a key reached
 *  only through data looks unreferenced to it, and would be deleted as dead by the next
 *  person who trusted that guard. Naming it here is what keeps it alive and translated —
 *  in the `…Key` property-table shape that guard's extractor reads, which is why this is
 *  a table of one rather than a bare array of strings. */
const SCORE_NOTES: { noteKey: MessageKey }[] = [{ noteKey: 'settings.contestScore.incomplete' }]
const SCORE_NOTE_KEYS: ReadonlySet<string> = new Set(SCORE_NOTES.map((n) => n.noteKey))
import { modeClassOf } from '../features/needs'
import { CONTEST_LOG_MAX_SHARE, CONTEST_LOG_SPLIT_MAX, CONTEST_LOG_SPLIT_MIN } from '../features/paneSeam'
import { PaneSeam } from './PaneSeam'
import { UtcClock } from './UtcClock'
import { askMainToWorkIn, dismissWorkInHint, workInHintSeen, workInLabel, workInTitle, workInViewsFor, type WorkInView } from '../features/contestWorkIn'
import { useFeatures } from '../useFeatures'
import { upperTyped } from '../typedInput'

/**
 * ⚠️ INVARIANT — the FD mode codes an entry is scored by, printed as they are logged and
 * exported. Never translated, and never a catalog entry.
 */
const FD_MODE_CODES = { dig: 'DIG', cw: 'CW', ph: 'PH' } as const

/** The short name in the header — WFD is the event's own abbreviation. */
const FD_SHORT_NAMES: Record<FdKind, string> = { arrlfd: 'Field Day', wfd: 'WFD' }

// ---------------------------------------------------------------------------
// FD bonus table — mirrored from the Rust FD_BONUSES table.
//
// ⚠️ The LABELS are invariant and deliberately not catalog entries: each names a bonus the
// ARRL scores by that name on a submitted entry, exactly as an award name does. They mirror
// the Rust FD_BONUSES table one for one — change neither side alone.
// ---------------------------------------------------------------------------
export interface FdBonus {
  id: string
  label: string
  points: number
}

export const FD_BONUSES: FdBonus[] = [
  { id: 'emergency-power',    label: 'Emergency Power',             points: 100 },
  { id: 'media-publicity',    label: 'Media Publicity',             points: 100 },
  { id: 'public-location',    label: 'Public Location',             points: 100 },
  { id: 'public-info-table',  label: 'Public Info Table',           points: 100 },
  { id: 'nts-message',        label: 'NTS Message',                 points: 100 },
  { id: 'w1aw-bulletin',      label: 'W1AW Bulletin',               points: 100 },
  { id: 'natural-power',      label: 'Natural Power (solar/wind)',  points: 100 },
  { id: 'site-visit-official', label: 'Site Visit by Elected Official', points: 100 },
  { id: 'site-visit-agency',  label: 'Site Visit by Agency Official', points: 100 },
  { id: 'gota',               label: 'GOTA Station',                points: 100 },
  { id: 'youth',              label: 'Youth Participation',         points: 100 },
  { id: 'web-submission',     label: 'Web Submission',              points: 50  },
  { id: 'safety-officer',     label: 'Safety Officer',              points: 100 },
  { id: 'social-media',       label: 'Social Media',               points: 100 },
  { id: 'educational',        label: 'Educational Activity',        points: 100 },
  // ARRL 7.3.8: "100 bonus points for successfully completing at least one QSO via an
  // amateur radio satellite during the Field Day period." One bonus however many birds
  // you work — "You do not receive an additional bonus for contacting different
  // satellites" — so it is a single box like the rest, not a per-bird tally.
  { id: 'satellite',          label: 'Satellite QSO',               points: 100 },
]

/**
 * THREE STATES PER BONUS, out of two id lists.
 *
 * `settings.fdBonuses` is the EARNED set and keeps that meaning exactly — it is what the
 * score is made of, on both sides of the wire (`fd_rules::bonus_points(&fd_bonuses)`).
 * `settings.fdBonusesPlanned` is the club's Friday intent, and NOTHING that scores,
 * exports or reports reads it. A plan is a plan, not points.
 *
 * Earned wins where an id sits in both lists, which is why confirming a bonus does not
 * have to edit two lists — and why un-confirming a mis-click puts it back on the chase
 * list instead of losing the plan.
 */
export type FdBonusState = 'none' | 'planned' | 'earned'

export function fdBonusState(id: string, earned: string[], planned: string[]): FdBonusState {
  if (earned.includes(id)) return 'earned'
  if (planned.includes(id)) return 'planned'
  return 'none'
}

/**
 * The chase arithmetic. `earnedPoints` is the ONLY number here that reaches the score —
 * it is the same sum `computeFdScore` falls back to. `plannedPoints` counts
 * planned-but-not-yet-earned bonuses only, so a bonus never lands in both columns and
 * "if all land" is a real ceiling rather than a double count.
 */
export function fdBonusTally(earned: string[], planned: string[]) {
  let earnedCount = 0
  let earnedPoints = 0
  let plannedCount = 0
  let plannedPoints = 0
  for (const b of FD_BONUSES) {
    const state = fdBonusState(b.id, earned, planned)
    if (state === 'earned') {
      earnedCount++
      earnedPoints += b.points
    } else if (state === 'planned') {
      plannedCount++
      plannedPoints += b.points
    }
  }
  return {
    earnedCount,
    earnedPoints,
    plannedCount,
    plannedPoints,
    potentialPoints: earnedPoints + plannedPoints,
  }
}

/**
 * The power tiers, one for one with Settings ▸ Contesting ▸ Field Day. The SAME
 * `fdPowerMult` field and the SAME catalog keys deliberately: two surfaces that edit one
 * setting cannot drift, and they must not describe it in two different sets of words.
 */
const FD_POWER_TIERS = [
  { value: 5, labelKey: 'settings.fieldDay.power.qrp.label',     hintKey: 'settings.fieldDay.power.qrp.hint' },
  { value: 2, labelKey: 'settings.fieldDay.power.hundred.label', hintKey: 'settings.fieldDay.power.hundred.hint' },
  { value: 1, labelKey: 'settings.fieldDay.power.high.label',    hintKey: 'settings.fieldDay.power.high.hint' },
] as const

// ---------------------------------------------------------------------------
// Internal types
// ---------------------------------------------------------------------------

interface Props {
  fieldDay: FieldDayStatus | null
  /** This laptop's club link (`AppSnapshot.clubLink`), said here whatever the Field Day mode
   *  switch says. Not passed on Remote or in the logger window. */
  clubLink?: ClubLink | null
  onSetMode?: (mode: ModeRequest) => void
  /** A complete ephemeral display capture; native loaders and writes stay disabled. */
  observation?: FdDisplaySettings
  /** The Contest mode switch (settings.fdActive) — gates the advisories. */
  fdActive?: boolean
  /** The active event's ruleset facts (App fetches get_fd_ruleset once per
   *  configured event) — the warn-only advisories read these. */
  fdRuleset?: FdRulesetDto | null
  /** The active digital tier (App's snap.link.tier) — the banned-mode chip
   *  checks it against the ruleset's bannedModes. */
  tier?: string
  /** The station's operating section (`snap.radio.operatingMode`). Given, the banned-mode chip names
   *  the mode the station is on — the tier on the digital section, the mode its contacts log under on
   *  the others (`stationMode`) — so a station on phone at a contest that bans FT8 is not told FT8 is
   *  not permitted. Absent (the Remote page, whose observation carries no section), it reads `tier`. */
  section?: string
  /** The Keyboard section's mode on the air (`snap.radio.keyboardMode`: PSK31, QPSK31 or JTTY), which
   *  the chip names on that section: PSK and JTTY are one section, so `section` cannot say which. */
  keyboardMode?: string
  /** Open Settings at a section (App's `openSettingsAt`): where the mode switch sends an
   *  operator whose contest cannot start yet. */
  onOpenSettings?: (target: string) => void
  /** App re-reads the settings and the snapshot after this screen saves the mode switch, so
   *  the rail, Settings and every cockpit see the change at once. */
  onSettingsSaved?: () => void
  /** The contest logger window's copy of this screen (`ContestLogger`), where nothing may start,
   *  stop or hold back the operator's transmission. So three things are read-only there and a
   *  line says they are on the main screen: the contest switch and Running / S&P (Running calls
   *  CQ in the digital cockpits), and the scoring panel, whose saves go through the whole-settings
   *  path that keeps an over already planned from keying. Everything else works as it does here:
   *  the operator box, the exports, the merge, the club, Remove and Restore. */
  logger?: boolean
  /** The switch a saved pick just made, for the session shown here (`useContestSwitch`, kept by
   *  App and the logger window): the screen says the station switched, and from what. */
  contestSwitch?: ContestSwitch | null
  /** CTRL+O FROM A CONTEST STRIP (`features/contestKeys.ts`), kept by App and the logger window:
   *  the asks the operator box has not answered yet. At each new one the box takes the caret once
   *  it can be typed in, and `onOperatorFocused` says it did. */
  operatorAsk?: number
  onOperatorFocused?: () => void
  /** The operator box is on the log line above this screen (`ContestLargeEntry`'s title line, while
   *  a contest runs in the main window and the logger window): the scoreboard draws none of its own,
   *  so the screen has one. Absent, the scoreboard keeps its box, as with no contest running, in the
   *  torn-off scoreboard and on the Remote page. */
  operatorBoxAbove?: boolean
  /** The contest entry (`ContestLargeEntry`) is drawn above this screen in the same window: the main
   *  window and the logger window. It carries the save-failure line, so the screen draws none of its
   *  own and the window shows it once. Absent in the popped-out screen, which has no entry. */
  entryAbove?: boolean
  /** Opens this screen in a window of its own (App's, in the main window only — never the logger
   *  window's copy, the window itself or the Remote page): its Pop out, at the end of the banner. */
  onPopOut?: () => void
  /** "Work this contest in CW / in Phone": the main window's own switch to that cockpit (App's
   *  `handleView`). Absent in every other window, where a press asks the main window instead
   *  (`askMainToWorkIn`, features/contestWorkIn). */
  onWorkIn?: (view: WorkInView) => void
}

interface LogRowMeta {
  qso: FieldDayQso
  /** first appearance of this section in the log = a new multiplier */
  isNewSection: boolean
  /** the ENGINE logged this row as a duplicate, scored zero — never re-derived here */
  isDupe: boolean
}

type ExportFormat = 'cabrillo' | 'adif' | 'summary' | 'dupesheet' | 'club-cabrillo' | 'club-adif'
const EXT: Record<ExportFormat, string> = {
  cabrillo: 'cbr',
  adif: 'adi',
  summary: 'txt',
  dupesheet: 'txt',
  'club-cabrillo': 'cbr',
  'club-adif': 'adi',
}

/**
 * Annotate each log entry with multiplier / dupe state. Sections are marked the first time
 * they appear (scanning oldest -> newest).
 *
 * ⚠️ **THE DUPE VERDICT IS THE ENGINE'S, READ OFF THE ROW.** This counted `call|band|mode`
 * occurrences and called anything seen twice a dupe — a fourth copy of the dupe key, and
 * wrong three ways. Counting marked the ORIGINAL as well as the duplicate, so a real
 * scoring contact was styled as a dupe for being worked again later. The triple is the key
 * for two of the eighteen shipped rulesets: Sweepstakes works a station once on ANY band
 * (rule 2.2), so its cross-band dupe went unmarked, while a QSO party counts a new county
 * as a new contact, so two legal contacts were both marked. And it could not see what
 * actually decides whether a row scores — whether the ENGINE logged it as a dupe.
 *
 * That last one stopped being academic when the ten cross-checked contests began LOGGING
 * a duplicate instead of refusing it (`DupeRule::log_dupes`): the log now holds rows worth
 * zero, and an operator checking the sponsor's math needs to know which.
 *
 * Absent means false — the flag is skipped on the wire for ordinary rows, and a station
 * older than the field sends none at all, which correctly marks nothing.
 */
export function annotate(log: FieldDayQso[]): LogRowMeta[] {
  const seenSections = new Set<string>()
  return log.map((q) => {
    // A contact in a mode the contest bans counts for nothing (the engine's mark, as `dupe`
    // is), so it brings no section: the next contact from it is the section's first.
    const counts = q.banned !== true
    const isNewSection = counts && !seenSections.has(q.section)
    if (counts) seenSections.add(q.section)
    return {
      qso: q,
      isNewSection,
      isDupe: q.dupe === true,
    }
  })
}

/** Per-mode contact count from the log. */
function modeCounts(log: FieldDayQso[]): { dig: number; cw: number; ph: number } {
  let dig = 0, cw = 0, ph = 0
  for (const q of log) {
    if (q.mode === 'DIG') dig++
    else if (q.mode === 'CW') cw++
    else if (q.mode === 'PH') ph++
  }
  return { dig, cw, ph }
}

/** The log table's grid for a contest that is not Field Day: one column per received slot,
 *  sized in styles.css off `--fd-slots` (the rules validator caps a role at five). Field
 *  Day passes 0 and keeps its own six-column rule untouched. */
function slotGrid(n: number): { 'data-slots'?: number; style?: CSSProperties } {
  return n > 0 ? { 'data-slots': n, style: { '--fd-slots': n } as CSSProperties } : {}
}

/** ⭐ The log carries QSO numbers (`FieldDayQso.nr`), so its tables show them in a first column. A
 *  station older than the field sends none, and the tables keep the columns they had. */
export function logIsNumbered(log: readonly FieldDayQso[]): boolean {
  return log.some((q) => q.nr != null)
}

/** "HH:MM" UTC for the FD log's time column. Blank the column when the QSO
 * predates the timestamp field or hasn't been logged yet. */
function qsoTimeUtc(q: FieldDayQso): string {
  const unix = q.whenUnix
  if (!unix) return ''
  const d = new Date(unix * 1000)
  const p = (n: number) => String(n).padStart(2, '0')
  return `${p(d.getUTCHours())}:${p(d.getUTCMinutes())}`
}

/** The log table's head row: Time · Call, then Field Day's Class (Category for Winter Field Day) and
 *  Section or one column per received slot, then Band · Mode. Shared by this screen's table and the
 *  contest logger window's last contacts (ContestLogger.tsx), so the two read alike, and by the
 *  host's club log, which alone adds Position. */
export function ContestLogHead({
  fieldDayColumns,
  isWfd,
  slotColumns,
  role,
  position = false,
  numbered = false,
}: {
  fieldDayColumns: boolean
  isWfd: boolean
  slotColumns: readonly ContestFieldSpec[]
  /** The session's role, which a column's caption depends on as its box's does. */
  role?: string
  /** One more column after Mode: the position that logged each contact (`ClubLogList`). */
  position?: boolean
  /** One more column before Time: each contact's QSO number (`logIsNumbered`). */
  numbered?: boolean
}) {
  return (
    <div
      className="fd-log-head"
      {...slotGrid(slotColumns.length)}
      data-pos={position ? '' : undefined}
      data-nr={numbered ? '' : undefined}
    >
      {/* `#` is the column's caption in every language, as N1MM heads it. */}
      {numbered && <span className="fd-col nr">#</span>}
      <span className="fd-col time">{t('fieldDay.log.column.time')}</span>
      <span className="fd-col call">{t('fieldDay.log.column.call')}</span>
      {fieldDayColumns ? (
        <>
          {/* Two words for two events, not one word with a variant: ARRL calls the exchange field
              Class and WFD calls it Category. H/I/M/O are Winter Field Day's category letters,
              which travel with the class number in this column (2O: two transmitters, outdoor) —
              exchange codes, never translated. */}
          <span className="fd-col cls">
            {isWfd ? t('fieldDay.log.column.category') : t('fieldDay.log.column.class')}
            {isWfd && <span className="fd-wfd-hint"> (H/I/M/O)</span>}
          </span>
          <span className="fd-col sec">{t('fieldDay.log.column.section')}</span>
        </>
      ) : (
        slotColumns.map((f) => (
          <span className="fd-col slot" key={f.key}>{receivedCaption(f, role)}</span>
        ))
      )}
      <span className="fd-col band">{t('fieldDay.log.column.band')}</span>
      <span className="fd-col mode">{t('fieldDay.log.column.mode')}</span>
      {position && <span className="fd-col pos">{t('fieldDay.club.board.column.position')}</span>}
    </div>
  )
}

/** One row of the log table, under `ContestLogHead`'s columns. `children` follow the mode chip in
 *  its cell: the screen's Remove on its newest row. */
export function ContestLogRow({
  row,
  fieldDayColumns,
  slotColumns,
  position,
  numbered = false,
  children,
}: {
  row: LogRowMeta
  fieldDayColumns: boolean
  slotColumns: readonly ContestFieldSpec[]
  /** The position that logged it, in the column `ContestLogHead`'s `position` adds. */
  position?: string
  /** Its QSO number, in the column `ContestLogHead`'s `numbered` adds. */
  numbered?: boolean
  children?: ReactNode
}) {
  return (
    <div
      className={`fd-log-row${fieldDayColumns && row.isNewSection ? ' mult' : ''}${row.isDupe ? ' dupe' : ''}`}
      {...slotGrid(slotColumns.length)}
      data-pos={position !== undefined ? '' : undefined}
      data-nr={numbered ? '' : undefined}
      title={
        row.isDupe
          ? t('fieldDay.log.dupe.title')
          : fieldDayColumns && row.isNewSection
            ? t('fieldDay.log.mult.title')
            : undefined
      }
    >
      {numbered && <span className="fd-col nr mono">{row.qso.nr ?? ''}</span>}
      <span className="fd-col time mono">{qsoTimeUtc(row.qso)}</span>
      <span className="fd-col call mono">{row.qso.call}</span>
      {fieldDayColumns ? (
        <>
          <span className="fd-col cls mono">{row.qso.class}</span>
          <span className="fd-col sec mono">
            {row.qso.section}
            {row.isNewSection && <span className="fd-mult-tag">{t('fieldDay.log.mult')}</span>}
          </span>
        </>
      ) : (
        slotColumns.map((f, j) => (
          <span className="fd-col slot mono" key={f.key}>{row.qso.rcvd?.[j] ?? ''}</span>
        ))
      )}
      <span className="fd-col band">{row.qso.band}</span>
      <span className="fd-col mode">
        {row.qso.mode && (
          <span className={`fd-mode-chip sm ${(row.qso.mode ?? '').toLowerCase()}`}>
            {row.qso.mode}
          </span>
        )}
        {children}
      </span>
      {position !== undefined && <span className="fd-col pos">{position}</span>}
    </div>
  )
}

// ---------------------------------------------------------------------------
// Client-side summary + dupe-sheet exports (spec §6). Both derive from the same
// log + annotate()/modeCounts() the board already uses — no backend command —
// and save through saveTextToDownloads() like the Cabrillo/ADIF paths.
// ---------------------------------------------------------------------------

/** Count QSOs per band, in canonical HF→VHF order. */
function bandCounts(log: FieldDayQso[]): { band: string; n: number }[] {
  const m = new Map<string, number>()
  for (const q of log) m.set(q.band, (m.get(q.band) ?? 0) + 1)
  return [...m.entries()]
    .map(([band, n]) => ({ band, n }))
    .sort((a, b) => bandRank(a.band) - bandRank(b.band) || a.band.localeCompare(b.band))
}

/** One column of the band × mode grid: the scoring classes it counts, and its heading. */
export interface ModeColumn {
  classes: string[]
  label: string
}

/** The scoring classes in the order ARRL's Field Day summary sheet lists them, and its names. */
const SHEET_CLASSES = ['CW', 'DIG', 'PH'] as const
const SHEET_CLASS_NAMES: Record<string, string> = { CW: 'CW', DIG: 'Digital', PH: 'Phone' }

/**
 * ⭐ **A contest's own mode classes, as the grid's columns**, in the summary sheet's order (CW,
 * Digital, Phone): each class its own column where the contest counts it apart, the classes its
 * dupe rule counts as one in one column (the Illinois QSO Party's "CW/Digital"), and for a contest
 * that counts no mode class at all (CQ WW, Sweepstakes, the VHF contests) the classes its log
 * holds. No rule (an older build) means the three, which is what such a build counted. The club
 * board's grid passes its positions' classes as `log`, so its rows are these same classes.
 */
export function modeColumns(rule: DupeRule | undefined, log: readonly Pick<FieldDayQso, 'mode'>[]): ModeColumn[] {
  const split = rule?.byModeClass ?? true
  const held = SHEET_CLASSES.filter((c) => log.some((q) => q.mode === c))
  const classes: string[] = split || held.length === 0 ? [...SHEET_CLASSES] : held
  const groups = split ? (rule?.modeClassGroups ?? []) : []
  const columns: ModeColumn[] = []
  for (const c of classes) {
    if (columns.some((col) => col.classes.includes(c))) continue
    const group = groups.find((g) => g.includes(c))
    const together = group ? classes.filter((x) => group.includes(x)) : [c]
    columns.push({ classes: together, label: together.map((x) => SHEET_CLASS_NAMES[x]).join('/') })
  }
  return columns
}

/** ARRL Field Day's summary sheet, item 18: its band rows, in its order and spelling. */
const SHEET_BANDS: [string, string][] = [
  ['160m', '160 M'],
  ['80m', '80 M'],
  ['40m', '40 M'],
  ['20m', '20 M'],
  ['15m', '15 M'],
  ['10m', '10 M'],
  ['6m', '6 M'],
  ['2m', '2 M'],
  ['1.25m', '1.25 M'],
  ['70cm', '70 CM'],
]

/**
 * ⭐ **The Summary's band × mode grid** — contacts per band and mode, laid out as ARRL's Field
 * Day summary sheet lays out its item 18 (2026 Summary Sheet, page 2): columns "CW Digital
 * Phone", rows "160 M, 80 M, 40 M, 20 M, 15 M, 10 M, 6 M, 2 M, 1.25 M, 70 CM, Other, Satellite,
 * GOTA" over "TOTALS", whose CW, Digital and Phone the sheet carries to its lines 8, 9 and 10.
 *
 * With `sheet` (ARRL Field Day) the rows are the sheet's own: a band it has no row for counts
 * under Other, and a contact made through a satellite under Satellite, which the rules (7.3.7)
 * list "separately on the summary sheet as a separate band". Its GOTA row is the GOTA station's,
 * which this laptop's log does not tell apart from its own, so it reads "—": that station's own
 * summary has its contacts. Every other contest gets the same grid in its own mode classes
 * ([`modeColumns`]), one row per band its log holds. Every contact the log holds is counted,
 * a duplicate logged at zero points included, as the QSO total above it counts it.
 */
export function bandModeGrid(log: FieldDayQso[], columns: ModeColumn[], sheet: boolean): string[] {
  const NOT_TRACKED = '—'
  const rowOf = (q: FieldDayQso): string => {
    if (!sheet) return q.band
    if (q.sat) return 'Satellite'
    return SHEET_BANDS.find(([band]) => band === q.band)?.[1] ?? 'Other'
  }
  const rows: string[] = sheet
    ? [...SHEET_BANDS.map(([, label]) => label), 'Other', 'Satellite', 'GOTA']
    : bandCounts(log).map((b) => b.band)
  const count = (row: string, col: ModeColumn) =>
    log.filter((q) => rowOf(q) === row && col.classes.includes(q.mode ?? '')).length
  const labelWidth = Math.max('TOTALS'.length, ...rows.map((r) => r.length)) + 2
  const width = (col: ModeColumn) => Math.max(col.label.length, 5) + 2
  const line = (label: string, cells: string[]) =>
    '  ' + label.padEnd(labelWidth) + cells.map((c, i) => c.padStart(width(columns[i]))).join('')
  const L: string[] = []
  L.push(line('Band', columns.map((c) => c.label)))
  for (const row of rows) {
    L.push(line(row, columns.map((c) => (sheet && row === 'GOTA' ? NOT_TRACKED : String(count(row, c))))))
  }
  L.push(line('TOTALS', columns.map((c) => String(log.filter((q) => c.classes.includes(q.mode ?? '')).length))))
  return L
}

interface SummaryArgs {
  eventName: string
  isWfd: boolean
  /** Which rules data scored this summary (FieldDayStatus.rulesYear /
   *  rulesGenerated) — 0/'' on an older backend skips the line. */
  rulesYear: number
  rulesGenerated: string
  myClass: string
  mySection: string
  log: FieldDayQso[]
  modes: { dig: number; cw: number; ph: number }
  workedSet: Set<string>
  powerMult: number
  qsoPts: number
  poweredPoints: number
  bonusPoints: number
  totalScore: number
  claimedBonuses: FdBonus[]
  /** Winter Field Day's completed objectives and the multiplier they earn, when the station
   *  scores by them — `totalScore` is then QSO points × (om + 1). */
  objectives?: { om: number; done: WfdObjective[] }
  /** The break that counts as off time in this contest (`offTimeRule`); 30 minutes when absent. */
  minOffSec?: number
  /** A contest that is NOT Field Day: what the session sends and how many multipliers the
   *  log has. Absent means Field Day's summary, byte for byte — class, section, sections
   *  worked, power multiplier and bonuses are Field Day's words and nobody else's. */
  contest?: { sending: string; multCount: number | null }
  /** The running contest's dupe rule, whose mode classes are the grid's columns
   *  ([`modeColumns`]); absent on an older build, which counted the three. */
  dupeRule?: DupeRule
  /** ARRL Field Day: lay the grid out as its summary sheet's item 18, rows and all. */
  fieldDaySheet?: boolean
  /** Per-operator statistics (FieldDayStatus.operators); absent from an older station. */
  operators?: FdOperatorStat[]
}

/** ⭐ **The Summary export's text, as the Contest screen's Summary button writes it**, for `event` (the
 *  running session's contest): the end-of-contest checklist offers the same file from the same
 *  inputs, so the two cannot differ. */
export function contestSummaryText(
  fieldDay: FieldDayStatus | null,
  settings: FdDisplaySettings | null,
  event: string | undefined,
): string {
  const isWfd = event === 'wfd'
  const fdEventIsFieldDay = isFieldDay(event)
  const log = fieldDay?.log ?? []
  const { fdPowerMult, qsoPts, poweredPoints, claimedBonusIds, bonusPoints, totalScore } = computeFdScore(
    fieldDay,
    settings,
  )
  const wfdOm = isWfd && typeof fieldDay?.objectiveMultiplier === 'number' ? fieldDay.objectiveMultiplier : null
  return buildSummaryText({
    eventName: fdEventIsFieldDay ? FD_EVENT_NAMES[isWfd ? 'wfd' : 'arrlfd'] : contestName(event),
    isWfd,
    minOffSec: offTimeRule(event).minOffSec,
    rulesYear: fieldDay?.rulesYear ?? 0,
    rulesGenerated: fieldDay?.rulesGenerated ?? '',
    myClass: composingSlot(fieldDay?.composing, 'CLASS'),
    mySection: composingSlot(fieldDay?.composing, 'SECTION'),
    log,
    modes: modeCounts(log),
    workedSet: workedSectionSet(fieldDay),
    powerMult: fdPowerMult,
    qsoPts,
    poweredPoints,
    bonusPoints,
    totalScore,
    claimedBonuses: FD_BONUSES.filter((b) => claimedBonusIds.includes(b.id)),
    objectives:
      wfdOm === null
        ? undefined
        : {
            om: wfdOm,
            done: WFD_OBJECTIVES.filter((o) => wfdEarnedIds(settings?.fdObjectives ?? []).has(o.id)),
          },
    // What the station sends, as it renders it: the issued serial, never the composing
    // vector's "0" placeholder, which a serial contest's summary used to print.
    contest: fdEventIsFieldDay
      ? undefined
      : { sending: fieldDay?.composingText ?? '', multCount: fieldDay?.multCount ?? null },
    dupeRule: fieldDay?.dupeRule,
    fieldDaySheet: fdEventIsFieldDay && !isWfd,
    operators: fieldDay?.operators,
  })
}

/**
 * One-page score summary (QSOs by band/mode, sections, power, bonuses, total).
 *
 * ⚠️ NOT MIGRATED, deliberately (i18n phase 2). This and `buildDupeSheetText` below build a
 * DOCUMENT the operator downloads and hands to a club or checks an entry against — not
 * interface prose. Its lines are hand-aligned fixed-width columns, so a translation would have
 * to preserve the column widths to stay readable, and its vocabulary is the one an ARRL entry
 * is submitted in. Localising the exports is a design decision of its own, not a mechanical
 * migration; it is recorded here rather than left looking like an omission.
 */
export function buildSummaryText(a: SummaryArgs): string {
  const L: string[] = []
  L.push(`${a.eventName.toUpperCase()} — SCORE SUMMARY`)
  if (a.contest) L.push(`Sending ${a.contest.sending || '—'}`)
  else L.push(`Station class ${a.myClass || '—'}   Section ${a.mySection || '—'}`)
  L.push(`Generated ${new Date().toISOString()}`)
  // Which rules parameters scored this document (design 3f) — a data update
  // that changes a number is visible on the artifact an operator hands over.
  if (a.rulesYear) {
    L.push(`Scored under ${a.rulesYear} rules (data ${a.rulesGenerated.slice(0, 10)})`)
  }
  L.push('')
  L.push(`QSOs: ${a.log.length}`)
  L.push(`  By mode:  DIG ${a.modes.dig}   CW ${a.modes.cw}   PH ${a.modes.ph}`)
  const bands = bandCounts(a.log)
  L.push(`  By band:  ${bands.length ? bands.map((b) => `${b.band} ${b.n}`).join('   ') : '—'}`)
  // How long the station was on, by the sponsors' off-time rule (features/contestOpTime).
  const minOff = a.minOffSec ?? OFF_TIME_MIN_SEC
  const op = operatingTime(a.log, minOff)
  if (op) {
    L.push(
      `Operating time:  on ${formatOpTime(op.onSec)}   off ${formatOpTime(op.offSec)} (breaks of ${minOff / 60} min or more)`,
    )
  }
  L.push('')
  // ⭐ The same contacts by band AND mode, as ARRL's Field Day summary sheet asks for them.
  L.push(a.fieldDaySheet ? 'QSOs BY BAND AND MODE (ARRL summary sheet, item 18)' : 'QSOs BY BAND AND MODE')
  L.push(...bandModeGrid(a.log, modeColumns(a.dupeRule, a.log), a.fieldDaySheet === true))
  if (a.fieldDaySheet) L.push("  GOTA: on the GOTA station's own summary.")
  const dupes = a.log.filter((q) => q.dupe === true).length
  if (dupes > 0) {
    L.push(`  Counted above: ${dupes} duplicate contact${dupes === 1 ? '' : 's'} logged at zero points.`)
  }
  // …and the contacts in a mode the contest bans (FT8 at the Illinois QSO Party), logged and
  // worth nothing, as the engine marked them.
  const banned = a.log.filter((q) => q.banned === true).length
  if (banned > 0) {
    L.push(`  Counted above: ${banned} contact${banned === 1 ? '' : 's'} in a mode this contest does not allow, logged at zero points.`)
  }
  L.push('')
  // Each operator's share, when more than one operator logged (features/contestOperators).
  L.push(...operatorSummaryLines(a.operators, a.contest?.multCount != null))
  if (a.contest) {
    // The score the engine computed: QSO points × multipliers, with no power tier and no
    // claimed bonuses — neither exists outside Field Day.
    const mults = a.contest.multCount
    if (mults !== null) {
      L.push(`Multipliers: ${mults}`)
      L.push('')
    }
    L.push('SCORE')
    L.push(`  QSO points                 ${a.qsoPts}`)
    if (mults !== null) L.push(`  × multipliers              ${mults}`)
    L.push('  --------------------------------')
    L.push(`  TOTAL                      ${a.totalScore}`)
    L.push('')
    return L.join('\n')
  }
  const secs = [...a.workedSet].sort()
  L.push(`Sections worked (${secs.length}):  ${secs.length ? secs.join(' ') : '—'}`)
  L.push('')
  if (a.objectives) {
    // WINTER FIELD DAY scores by its objectives: no power multiplier and no bonus menu, and
    // the total is the sponsor's QSO points × (OM + 1).
    L.push(`Objectives completed (${a.objectives.done.length}, OM ${a.objectives.om}):`)
    if (a.objectives.done.length === 0) L.push('  (none)')
    else for (const o of a.objectives.done) L.push(`  ${o.label} — ×${o.multiplier}`)
    L.push('')
    L.push('SCORE')
    L.push(`  QSO points                 ${a.qsoPts}`)
    L.push(`  × (OM ${a.objectives.om} + 1)`.padEnd(29) + `= ${a.totalScore}`)
    L.push('  --------------------------------')
    L.push(`  TOTAL                      ${a.totalScore}`)
    L.push('')
    return L.join('\n')
  }
  L.push(`Power multiplier: ×${a.powerMult}`)
  L.push(`Bonuses claimed (${a.claimedBonuses.length}, ${a.bonusPoints} pts):`)
  if (a.claimedBonuses.length === 0) L.push('  (none)')
  else for (const b of a.claimedBonuses) L.push(`  ${b.label} — ${b.points} pts`)
  L.push('')
  L.push('SCORE')
  L.push(`  QSO points                 ${a.qsoPts}`)
  if (a.isWfd) {
    // WFD scores by objectives at submission — claiming the ARRL power×+bonus
    // total here would be a number WFD rules never produce, so stay honest.
    L.push('  WFD objective multipliers apply at submission (not tracked here).')
  } else {
    L.push(`  × power ×${a.powerMult}                 = ${a.poweredPoints}`)
    L.push(`  + bonuses                  ${a.bonusPoints}`)
    L.push('  --------------------------------')
    L.push(`  TOTAL                      ${a.totalScore}`)
  }
  L.push('')
  return L.join('\n')
}

/** Dupe / multiplier check sheet: new-section multipliers + alphabetical callsign list.
 *  `contestName` is set for a contest that is not Field Day, whose sheet is the callsign
 *  check alone: its multipliers are not sections, and its board says what they are. */
function buildDupeSheetText(rows: LogRowMeta[], contestName?: string): string {
  const L: string[] = []
  L.push(contestName ? `${contestName.toUpperCase()} — DUPE SHEET` : 'FIELD DAY — DUPE & MULTIPLIER SHEET')
  L.push(`Generated ${new Date().toISOString()}`)
  L.push('')

  if (!contestName) {
    const mults = rows.filter((r) => r.isNewSection)
    L.push(`MULTIPLIERS — sections worked (${mults.length})`)
    if (mults.length === 0) L.push('  (none yet)')
    else for (const r of mults) {
      L.push(`  ${r.qso.section.padEnd(5)} first worked by ${r.qso.call} on ${r.qso.band}`)
    }
    L.push('')
  }

  const byCall = new Map<string, FieldDayQso[]>()
  for (const r of rows) {
    const list = byCall.get(r.qso.call) ?? []
    list.push(r.qso)
    byCall.set(r.qso.call, list)
  }
  const calls = [...byCall.keys()].sort()
  const dupeCount = calls.filter((c) => (byCall.get(c)?.length ?? 0) > 1).length
  L.push(`CALLSIGN CHECK — ${calls.length} unique / ${rows.length} QSOs   (${dupeCount} worked more than once, * = dupe)`)
  for (const call of calls) {
    const qs = byCall.get(call) ?? []
    const flag = qs.length > 1 ? ' *' : ''
    const where = qs.map((q) => `${q.band}${q.mode ? '/' + q.mode : ''}`).join(', ')
    L.push(`  ${call.padEnd(10)} x${qs.length}${flag}   [${where}]`)
  }
  L.push('')
  return L.join('\n')
}

// ---------------------------------------------------------------------------
// Worked-sections board (spec §5). Styled inline off the shared design tokens
// so it stays theme-aware without touching styles.css. Worked cells use the
// DESIGN.md `confirmed` role (green + ✓ glyph — color is a redundant cue);
// unworked cells recede (dim, muted, no glyph).
// ---------------------------------------------------------------------------
// The board is the club-loved feature, so it grows to fill the space the capped log
// gives back (flex:1 in both the docked column and the torn-off scoreboard window);
// the grid scrolls internally when the sections overflow.
const SECTIONS_BOARD_WRAP: CSSProperties = {
  flex: '1 1 auto',
  // THE FLOOR, AND IT BELONGS ON THIS BOX. This is the only flex-GROW child of the Field
  // Day column — banner, header, operator row, score tiles, Bonuses and the log are all
  // `flex: 0 0 auto` — so with `minHeight: 0` the board was the single unfloored absorber
  // and paid for every sibling's growth down to nothing. Opening the 15-row Bonuses list
  // at 1200×750 turned the board into a blank strip; `.fd-bonuses-list`'s 0.3×--vh-eff cap
  // could never prevent that, because a cap bounds the LIST and says nothing about the
  // height it does allow displacing someone. 180px is what the board already effectively
  // reserved (the 120 that used to sit on SECTIONS_GRID, plus this box's 30px of vertical
  // padding and the ~30px header), moved to the box the outer flex actually sizes.
  //
  // A HARD floor rather than a `min(Xem, share)` yielding one — legal ONLY because the
  // column above now scrolls (`.fieldday { overflow-y: auto }`, styles.css ~7960); it is
  // the same bargain `.cockpit-panes`' 18em floor states in cockpit-panes.css. Remove that
  // valve and this floor becomes a clip. Computed in layout-single-deficit.test.tsx.
  minHeight: 180,
  display: 'flex',
  flexDirection: 'column',
  padding: '14px 16px 16px',
  borderBottom: '1px solid var(--border-soft)',
}
const SECTIONS_HEADER: CSSProperties = {
  display: 'flex',
  alignItems: 'baseline',
  justifyContent: 'space-between',
  gap: 8,
  marginBottom: 10,
}
const SECTIONS_GRID: CSSProperties = {
  flex: '1 1 auto',
  // NO floor: this box is the board's interposed SCROLLER, and its old 120px one was
  // invisible to the outer column (which sizes the wrap, not this) while being unable to
  // shrink — so the moment the wrap was squeezed the grid painted straight through the
  // Bonuses section below it. The 120 moved up into SECTIONS_BOARD_WRAP's floor, where the
  // column can see it; at that floor this grid still computes to ~120px and scrolls.
  minHeight: 0,
  display: 'flex',
  flexWrap: 'wrap',
  alignContent: 'flex-start',
  gap: '14px 22px',
  overflowY: 'auto',
}
const DIVISION_BLOCK: CSSProperties = {
  display: 'flex',
  flexDirection: 'column',
  gap: 6,
}
const DIVISION_LABEL: CSSProperties = {
  fontSize: textPx(11),
  fontWeight: 700,
  letterSpacing: '0.04em',
  textTransform: 'uppercase',
  color: 'var(--text-faint)',
}
const DIVISION_CELLS: CSSProperties = {
  display: 'flex',
  flexWrap: 'wrap',
  gap: 6,
}
const CELL_BASE: CSSProperties = {
  display: 'inline-flex',
  alignItems: 'center',
  gap: 4,
  padding: '4px 9px',
  borderRadius: 'var(--radius-sm)',
  fontFamily: 'var(--font-mono)',
  fontSize: textPx(14),
  lineHeight: 1.4,
}
const CELL_WORKED: CSSProperties = {
  ...CELL_BASE,
  fontWeight: 700,
  color: 'var(--status-confirmed)',
  background: 'color-mix(in srgb, var(--status-confirmed) 16%, transparent)',
  border: '1px solid color-mix(in srgb, var(--status-confirmed) 55%, transparent)',
}
const CELL_UNWORKED: CSSProperties = {
  ...CELL_BASE,
  fontWeight: 500,
  color: 'var(--text-faint)',
  background: 'var(--bg-elev)',
  border: '1px solid var(--border-soft)',
  opacity: 0.5,
}
/** A cell THIS laptop worked, on a board that also shows the club's: the look the club grid gives
 *  this laptop (`.club-grid-pos.link-self`) — its ⌂ mark, an accent edge and a tint of its face,
 *  the code in ink — so it reads apart from the green ✓ of a cell another position worked by its
 *  colour and by its mark, never the colour alone. A board with no club list keeps CELL_WORKED. */
const CELL_OWN: CSSProperties = {
  ...CELL_BASE,
  fontWeight: 700,
  color: 'var(--text)',
  background: 'color-mix(in srgb, var(--accent) 14%, transparent)',
  border: '1px solid var(--accent)',
}
/** The legend's two keys, each in the look of the cells it names, on the header's line after the
 *  board's name (its auto margin keeps the count at the right), so the board keeps its rows of
 *  cells where it has least room; the line wraps where it is too narrow for all three. */
const BOARD_LEGEND: CSSProperties = {
  display: 'flex',
  flexWrap: 'wrap',
  alignItems: 'center',
  gap: 8,
  marginRight: 'auto',
}
const LEGEND_KEY: CSSProperties = {
  fontFamily: 'inherit',
  fontSize: textPx(12),
  fontWeight: 600,
  padding: '2px 8px',
}

/** The colored worked/unworked section grid, grouped by ARRL division. */
export function SectionsBoard({ workedSet }: { workedSet: Set<string> }) {
  const workedCount = useMemo(
    () =>
      ARRL_SECTIONS_BY_DIVISION.reduce(
        (n, d) => n + d.sections.filter((s) => workedSet.has(s.code)).length,
        0,
      ),
    [workedSet],
  )
  return (
    <div style={SECTIONS_BOARD_WRAP} aria-label={t('fieldDay.sections.aria')}>
      <div style={SECTIONS_HEADER}>
        <span style={{ fontSize: textPx(15), fontWeight: 700, color: 'var(--text)' }}>
          {t('fieldDay.sections.head')}
        </span>
        <span style={{ fontSize: textPx(13), color: 'var(--text-dim)' }}>
          {t('fieldDay.sections.count', { worked: workedCount, total: ARRL_SECTION_TOTAL })}
        </span>
      </div>
      <div style={SECTIONS_GRID}>
        {ARRL_SECTIONS_BY_DIVISION.map((div) => (
          <div style={DIVISION_BLOCK} key={div.division}>
            <span style={DIVISION_LABEL}>{div.division}</span>
            <div style={DIVISION_CELLS}>
              {div.sections.map((s) => {
                const worked = workedSet.has(s.code)
                return (
                  <span
                    key={s.code}
                    style={worked ? CELL_WORKED : CELL_UNWORKED}
                    title={
                      worked
                        ? t('fieldDay.sections.cell.worked.title', {
                            code: s.code,
                            name: s.name,
                            division: div.division,
                          })
                        : t('fieldDay.sections.cell.notWorked.title', {
                            code: s.code,
                            name: s.name,
                            division: div.division,
                          })
                    }
                    aria-label={
                      worked
                        ? t('fieldDay.sections.cell.worked.aria', { name: s.name })
                        : t('fieldDay.sections.cell.notWorked.aria', { name: s.name })
                    }
                  >
                    {worked && <span aria-hidden="true">✓</span>}
                    {s.code}
                  </span>
                )
              })}
            </div>
          </div>
        ))}
      </div>
    </div>
  )
}

/**
 * ⭐ **THE MULTIPLIER DISPLAY (spec §9) — one block per board the session declares.**
 *
 * The generalisation of [`SectionsBoard`] above: CQ WW shows two blocks (zone,
 * country), a QSO party shows one, and Field Day shows the one it has always shown.
 *
 * The board LIST comes from the engine (`FieldDayStatus.boards`), which builds it from
 * the ruleset's `MultiplierRule`s or — for a ruleset that declares none, which is both
 * Field Day events — from the received slots that have a closed value set. The UI
 * never re-derives what counts as a multiplier.
 *
 * The CELL UNIVERSE comes from `features/contestDomains.ts`, keyed by the domain id on
 * the board. It is not on the wire: 85 section codes on every 300 ms snapshot to draw
 * a static grid is a cost with no buyer.
 *
 * ⚠️ A board whose domain this build has no value set for renders WHAT WAS WORKED and
 * says nothing about a total — an honest empty universe, not a fabricated one.
 *
 * ⭐ **At a club the board is the CLUB's.** A board that carries `club` (the host's club log, or
 * what the host sent this laptop) lights every value any position worked, so everyone sees what
 * the club still needs, and marks this laptop's own (`worked`) apart: ⌂ in the accent this laptop
 * has on the club grid, against the green ✓ of another position's, with a legend saying which is
 * which and a count of both. A board without it (no club sync, an older host, Remote) is this
 * laptop's own, exactly as it always was.
 */
export function MultiplierBoards({ boards }: { boards: ContestBoard[] }) {
  return (
    <>
      {boards.map((b) => (
        <MultiplierBoard board={b} key={b.id} />
      ))}
    </>
  )
}

/** What a cell of a multiplier board shows: worked here, worked by another position of the club
 *  (only on a board that carries the club's list), or not worked yet. */
type CellWorked = 'own' | 'club' | 'none'

function MultiplierBoard({ board }: { board: ContestBoard }) {
  const domain = contestDomain(board.domain)
  // Each value as the multiplier it counts as: a place its list names under two spellings
  // (`ContestDomain.same`, ILQP's PE and PEI) lights both cells and counts once.
  const workedSet = useMemo(
    () => new Set(board.worked.map((v) => multCode(domain, v))),
    [board.worked, domain],
  )
  // The club's, this laptop's included — present only while club sync runs and the host sends it.
  const clubSet = useMemo(
    () => new Set((board.club ?? []).map((v) => multCode(domain, v))),
    [board.club, domain],
  )
  const club = clubSet.size > 0
  const cellWorked = (code: string): CellWorked => {
    const mult = multCode(domain, code)
    return workedSet.has(mult) ? 'own' : clubSet.has(mult) ? 'club' : 'none'
  }
  // Field Day's board IS the sections board, down to its strings — the block an
  // operator has been reading for releases. Nothing about it moves here.
  const isSections = board.domain === 'fd_sections' || board.domain === 'arrl_sections'
  const groups: DomainGroup[] = domain
    ? domain.groups
    : // No value set for this domain: the universe is what has been worked, by anyone.
      [
        {
          label: '',
          values: [...new Set([...workedSet, ...clubSet])].sort().map((c) => ({ code: c, name: c })),
        },
      ]
  const total = domain ? multTotal(domain) : groups[0].values.length
  const countOf = (pick: (w: CellWorked) => boolean) =>
    new Set(
      groups.flatMap((g) => g.values.filter((v) => pick(cellWorked(v.code))).map((v) => multCode(domain, v.code))),
    ).size
  const workedCount = countOf((w) => w !== 'none')
  const ownCount = countOf((w) => w === 'own')
  const count = club
    ? isSections
      ? t('fieldDay.sections.countClub', { worked: workedCount, total, own: ownCount })
      : t('fieldDay.board.countClub', { worked: workedCount, total, own: ownCount })
    : isSections
      ? t('fieldDay.sections.count', { worked: workedCount, total })
      : t('fieldDay.board.count', { worked: workedCount, total })
  return (
    <div
      style={SECTIONS_BOARD_WRAP}
      aria-label={
        isSections ? t('fieldDay.sections.aria') : t('fieldDay.board.aria', { name: board.id })
      }
    >
      <div style={club ? { ...SECTIONS_HEADER, flexWrap: 'wrap' } : SECTIONS_HEADER}>
        <span style={{ fontSize: textPx(15), fontWeight: 700, color: 'var(--text)' }}>
          {/* A board id that is not Field Day's is a SLOT ID — an invariant token, shown
              as itself rather than run through a catalog that has no word for it. */}
          {isSections ? t('fieldDay.sections.head') : board.id}
        </span>
        {club && (
          // For the eye: every cell already says its own words to a screen reader.
          <div style={BOARD_LEGEND} data-board-legend="" aria-hidden="true">
            <span style={{ ...CELL_OWN, ...LEGEND_KEY }}>⌂ {t('fieldDay.board.legend.own')}</span>
            <span style={{ ...CELL_WORKED, ...LEGEND_KEY }}>✓ {t('fieldDay.board.legend.club')}</span>
          </div>
        )}
        <span style={{ fontSize: textPx(13), color: 'var(--text-dim)' }}>{count}</span>
      </div>
      <div style={SECTIONS_GRID}>
        {groups.map((g) => (
          <div style={DIVISION_BLOCK} key={g.label}>
            {g.label !== '' && <span style={DIVISION_LABEL}>{g.label}</span>}
            <div style={DIVISION_CELLS}>
              {g.values.map((v) => {
                const worked = cellWorked(v.code)
                const { code, name } = v
                const division = g.label
                const divided = division !== ''
                return (
                  <span
                    key={v.code}
                    data-worked={worked}
                    style={
                      worked === 'none' ? CELL_UNWORKED : worked === 'own' && club ? CELL_OWN : CELL_WORKED
                    }
                    title={
                      worked === 'none'
                        ? divided
                          ? t('fieldDay.sections.cell.notWorked.title', { code, name, division })
                          : t('fieldDay.board.cell.notWorked.title', { code, name })
                        : !club
                          ? divided
                            ? t('fieldDay.sections.cell.worked.title', { code, name, division })
                            : t('fieldDay.board.cell.worked.title', { code, name })
                          : worked === 'own'
                            ? divided
                              ? t('fieldDay.sections.cell.own.title', { code, name, division })
                              : t('fieldDay.board.cell.own.title', { code, name })
                            : divided
                              ? t('fieldDay.sections.cell.club.title', { code, name, division })
                              : t('fieldDay.board.cell.club.title', { code, name })
                    }
                    aria-label={
                      worked === 'none'
                        ? t('fieldDay.sections.cell.notWorked.aria', { name: v.name })
                        : !club
                          ? t('fieldDay.sections.cell.worked.aria', { name: v.name })
                          : worked === 'own'
                            ? t('fieldDay.sections.cell.own.aria', { name: v.name })
                            : t('fieldDay.sections.cell.club.aria', { name: v.name })
                    }
                  >
                    {worked !== 'none' && (
                      <span aria-hidden="true">{worked === 'own' && club ? '⌂' : '✓'}</span>
                    )}
                    {v.code}
                  </span>
                )
              })}
            </div>
          </div>
        ))}
      </div>
    </div>
  )
}

// ---------------------------------------------------------------------------
// Shared scoreboard (spec: the score tiles + sections board + settable operator),
// rendered by BOTH the docked ContestView and the torn-off scoreboard window.
// ---------------------------------------------------------------------------

/** Worked-section set for the board — prefer the authoritative DTO field, fall back
 * to deriving it from the log's sections. Uppercased for a case-insensitive match. */
function workedSectionSet(fieldDay: FieldDayStatus | null): Set<string> {
  const set = new Set<string>()
  const src = fieldDay?.workedSections ?? (fieldDay?.log ?? []).map((q) => q.section)
  for (const s of src) {
    const code = s.trim().toUpperCase()
    if (code) set.add(code)
  }
  return set
}

interface FdScore {
  fdPowerMult: number
  qsoPts: number
  poweredPoints: number
  claimedBonusIds: string[]
  bonusPoints: number
  totalScore: number
  /** ⭐ How many multipliers the ruleset counted — `null` when it declares none, which
   *  is not the same as having counted zero of them. */
  multCount: number | null
}

/** Score components from the snapshot (new fields); fall back to computed if absent. */
function computeFdScore(fieldDay: FieldDayStatus | null, settings: FdDisplaySettings | null): FdScore {
  const fdPowerMult = settings?.fdPowerMult ?? 1
  const qsoPts = fieldDay?.points ?? 0
  const poweredPoints = fieldDay?.poweredPoints ?? qsoPts * fdPowerMult
  const claimedBonusIds = settings?.fdBonuses ?? []
  const bonusPoints = fieldDay?.bonusPoints ?? FD_BONUSES
    .filter((b) => claimedBonusIds.includes(b.id))
    .reduce((sum, b) => sum + b.points, 0)
  const totalScore = fieldDay?.totalScore ?? poweredPoints + bonusPoints
  // ⭐ The multiplier count off the ruleset — `null` for a contest that declares none,
  // which is what switches the tile and the score line between Field Day's arithmetic
  // and a QSO party's. A contest that HAS multipliers and has worked none is `0`, and
  // must still get the QSO-party display.
  const multCount = fieldDay?.multCount ?? null
  return {
    fdPowerMult,
    qsoPts,
    poweredPoints,
    claimedBonusIds,
    bonusPoints,
    totalScore,
    multCount,
  }
}

// ---------------------------------------------------------------------------
// Club sync (the Nexus↔Nexus event sync). Rendered only while the club block
// rides the snapshot (hosting or joined) — a solo Field Day never sees it.
// Inline styles off the shared tokens, the SectionsBoard idiom.
// ---------------------------------------------------------------------------

const CLUB_WRAP: CSSProperties = {
  flex: '0 0 auto',
  display: 'flex',
  flexDirection: 'column',
  gap: 8,
  padding: '10px 16px 12px',
  borderBottom: '1px solid var(--border-soft)',
}
const CLUB_HEADER: CSSProperties = {
  display: 'flex',
  alignItems: 'center',
  flexWrap: 'wrap',
  gap: 10,
}
const CLUB_CHIP_BASE: CSSProperties = {
  display: 'inline-flex',
  alignItems: 'center',
  gap: 5,
  padding: '3px 10px',
  borderRadius: 'var(--radius-sm)',
  fontSize: textPx(12),
  fontWeight: 700,
  letterSpacing: '0.03em',
}
/** The sync chip's ink per state — every token exists in BOTH themes. */
function clubChipStyle(state: string): CSSProperties {
  const ink =
    state === 'synced'
      ? 'var(--status-confirmed)'
      : state === 'behind'
        ? 'var(--status-new-band)'
        : 'var(--status-new-entity)'
  return {
    ...CLUB_CHIP_BASE,
    color: ink,
    background: `color-mix(in srgb, ${ink} 14%, transparent)`,
    border: `1px solid color-mix(in srgb, ${ink} 50%, transparent)`,
  }
}
const CLUB_BOARD_GRID: CSSProperties = {
  display: 'grid',
  gridTemplateColumns: 'minmax(0,1.6fr) 0.8fr 0.7fr minmax(0,1fr) 0.6fr 0.6fr',
  columnGap: 10,
  rowGap: 3,
  fontSize: textPx(13),
  alignItems: 'baseline',
}
/** The host's board has one more column: each position's clock. */
const CLUB_BOARD_COLUMNS_HOST = 'minmax(0,1.6fr) 0.8fr 0.7fr minmax(0,1fr) 0.6fr 0.6fr minmax(0,0.9fr)'
/** …and, while its rows carry the host's link words, the Status column after the position and,
 *  on the host's own contest screen, one at the end for each row's Remove or Allow back. */
function clubBoardColumns(link: boolean, manage: boolean): string {
  const [position, ...rest] = CLUB_BOARD_COLUMNS_HOST.split(' ')
  return [position, ...(link ? ['minmax(0,1.7fr)'] : []), ...rest, ...(manage ? ['auto'] : [])].join(' ')
}
/** Each link word's ink, the grid's own three (both themes carry them): green, the warning amber,
 *  and Lost in the alert red, which the sync chip's orange is not in the dark theme. The host's own
 *  row and a removed one read in the dim ink, as no alarm. */
const CLUB_LINK_INK: Record<NonNullable<FdClubBoardRow['link']>, string> = {
  host: 'var(--text-dim)',
  connected: 'var(--status-confirmed)',
  reconnecting: 'var(--alert-warning)',
  lost: 'var(--alert-critical)',
  removed: 'var(--text-dim)',
}
/** Each half of a link cell keeps to one line, and the cell wraps between them; a half wider than
 *  the whole column still ends in an ellipsis rather than spill into the next. */
const LINK_CHUNK: CSSProperties = {
  display: 'inline-block',
  maxWidth: '100%',
  whiteSpace: 'nowrap',
  overflow: 'hidden',
  textOverflow: 'ellipsis',
  verticalAlign: 'top',
}
const CLUB_COL_HEAD: CSSProperties = {
  fontSize: textPx(10),
  fontWeight: 700,
  letterSpacing: '0.04em',
  textTransform: 'uppercase',
  color: 'var(--text-faint)',
}
/** Column heads, at the docked size or the torn-off window's glance size. */
function colHead(big: boolean): CSSProperties {
  return big ? { ...CLUB_COL_HEAD, fontSize: textPx(13) } : CLUB_COL_HEAD
}
/** A club sentence keeps a measure of its own where the screen is wider than its old 1600px cap:
 *  the panel sets `--fd-prose-measure` on lg/xl (styles.css), and elsewhere it is unset. */
const CLUB_WARN: CSSProperties = {
  fontSize: textPx(12),
  color: 'var(--status-new-entity)',
  maxWidth: 'var(--fd-prose-measure, none)',
}
const CLUB_NOTE: CSSProperties = {
  fontSize: textPx(12),
  color: 'var(--text-dim)',
  maxWidth: 'var(--fd-prose-measure, none)',
}
/** A club code, in a box of its own: set apart from the names and calls the laptops send, so no
 *  name can pass for one. */
const CLUB_CODE: CSSProperties = {
  display: 'inline-block',
  padding: '1px 6px',
  border: '1px solid var(--border-soft)',
  borderRadius: 'var(--radius-sm)',
  fontWeight: 700,
  letterSpacing: '0.06em',
  color: 'var(--text)',
  whiteSpace: 'nowrap',
}
/** The host's check before it gives a laptop a position, under the entry it asks about. */
const CLUB_CONFIRM: CSSProperties = {
  display: 'flex',
  flexDirection: 'column',
  alignItems: 'flex-start',
  gap: 6,
  padding: '8px 10px',
  border: '1px solid var(--status-new-entity)',
  borderRadius: 'var(--radius-sm)',
  fontSize: textPx(12),
  color: 'var(--text)',
}

/** The club line says this PC's clock against the host's from here, in whole seconds… */
const CLOCK_SHOW_SECS = 2
/** …and past here says it as the warning it has always been. */
const CLOCK_WARN_SECS = 30

/** A board row's clock, in ms, as whole seconds rounded half away from zero (the
 *  position's own line rounds the same way, so the two never say different numbers). */
function clockSecs(ms: number): number {
  return Math.sign(ms) * Math.round(Math.abs(ms) / 1000)
}

/** A board row's clock in the club line's own terms: in step under 2 s, ahead or behind
 *  from there, a dash when the position has not measured one. */
function clockCell(ms: number | null | undefined): string {
  if (ms == null) return '—'
  const secs = clockSecs(ms)
  if (Math.abs(secs) < CLOCK_SHOW_SECS) return t('fieldDay.club.board.clock.ok')
  return secs > 0
    ? t('fieldDay.club.board.clock.ahead', { secs })
    : t('fieldDay.club.board.clock.behind', { secs: -secs })
}

/** A board row's position as its line names it: an unnamed position falls back to who is sitting
 *  at it, and only then to a translated placeholder — never to the position id, which is internal
 *  plumbing an operator should never be shown. */
function boardName(row: FdClubBoardRow): string {
  return row.posName || row.operator || t('fieldDay.club.board.unnamed')
}

/** A host board row's link, in the Status column's word. */
function linkWord(link: NonNullable<FdClubBoardRow['link']>): string {
  return link === 'host'
    ? t('fieldDay.club.board.link.host')
    : link === 'connected'
      ? t('fieldDay.club.board.link.connected')
      : link === 'reconnecting'
        ? t('fieldDay.club.board.link.reconnecting')
        : link === 'lost'
          ? t('fieldDay.club.board.link.lost')
          : t('fieldDay.club.board.link.removed')
}

/** When the host last heard a position, as the Status column says it after the word. */
function heardWords(row: FdClubBoardRow): string {
  return row.heardSecs == null
    ? t('fieldDay.club.board.heard.never')
    : t('fieldDay.club.board.heard', { ago: heardAgo(row.heardSecs) })
}

/** A position the host has not heard lately, on a board with no link words to say so (a position's,
 *  Remote's): last heard more than the club link's dead timeout ago (`fdsync::DEAD_SECS`). The table
 *  dims its line and marks its name ⚠, and the grid greys its name and marks it ⚠, both by this. On
 *  the host's own board every line carries its link word, which says it instead (Lost). */
function notHeard(row: FdClubBoardRow): boolean {
  return !row.link && row.lastSeenSecs > CLUB_DEAD_SECS
}

/** A host board row's link word and when the host last heard the position, as one cell: on one
 *  line where the column has room, and the heard part on a line of its own where it has not (a
 *  1024-wide window), never cut. */
function LinkCell({ row }: { row: FdClubBoardRow }) {
  const link = row.link
  if (!link) return <span />
  const word = linkWord(link)
  const heard = link === 'host' ? '' : heardWords(row)
  return (
    <span data-club-row={row.posid} style={{ minWidth: 0 }} title={heard ? `${word} ${heard}` : word}>
      <span style={{ ...LINK_CHUNK, color: CLUB_LINK_INK[link], fontWeight: 600 }}>{word}</span>
      {heard && (
        <>
          {' '}
          <span style={{ ...LINK_CHUNK, color: 'var(--text-dim)' }}>{heard}</span>
        </>
      )}
    </span>
  )
}

/** The club link's lines (`features/clubLink.ts`) in the club block's two styles: the addresses
 *  and the club code each in a box a reader can copy from. */
function ClubLinkRows({ lines }: { lines: ClubLinkLine[] }) {
  return (
    <>
      {lines.map((l) => (
        <div key={l.key} style={l.alert ? CLUB_WARN : CLUB_NOTE} role={l.alert ? 'alert' : 'status'} data-club-link={l.key}>
          {l.text}
          {l.addrs?.map((a, i) => (
            <Fragment key={a}>
              {i > 0 ? ADDR_SEP : ' '}
              <span className="mono">{a}</span>
            </Fragment>
          ))}
          {l.code && (
            <>
              {' '}
              <span className="mono" style={{ ...CLUB_CODE, fontSize: textPx(12) }}>
                {t('fieldDay.club.code', { code: l.code })}
              </span>
            </>
          )}
        </div>
      ))}
    </>
  )
}

/** The chip's text for each derived state (honesty rule: the queue is IN the
 * label, so "connected but behind" can never masquerade as synced). */
function clubChipText(club: FdClubStatus): string {
  switch (club.syncState) {
    case 'synced':
      return t('fieldDay.club.state.synced')
    case 'behind':
      return t('fieldDay.club.state.behind', { queued: club.queued })
    default:
      return t('fieldDay.club.state.offline', { queued: club.queued })
  }
}

/**
 * The club band board — who is on what band, across every position on site.
 *
 * Exported because it is also the whole content of the `fdclub` pop-out
 * (DetachedPanel), which the rail opens directly: a multi-station club watches
 * this continuously — "to see where people are so they can move to the right
 * bands when multiple stations are operating" — and the `fieldday` pop-out is
 * already taken by the scoreboard. `onExport` is absent in the torn-off copy,
 * because the export buttons report where the file landed through a toast and a
 * detached window hosts none.
 *
 * `detached` is therefore TWO things: it drops the pop-out button and the export
 * pair, and it sets the whole board in bigger type. The size is the point of the
 * window — this board is watched from the operating position across the tent, not
 * read at the keyboard — so it is a design choice here, NOT the hand-compensation
 * the torn-off Needed window used to carry for the 65% zoom floor (see the
 * `.np-grid` note in styles.css). It is honest because `PANEL_NATURAL.fdclub`
 * declares the larger box, so auto-fit opens the window big enough for it instead
 * of shrinking the type straight back.
 */
/** Bands Field Day is actually worked on. 60/30/17/12 are barred by both ARRL FD and WFD,
 *  so a board that offered them as "free" would be inviting an illegal contact. */
const FD_BOARD_BANDS = ['160m', '80m', '40m', '20m', '15m', '10m', '6m', '2m', '1.25m', '70cm']

/** The dead-man for a presence reading, matching the club board's own stale mark: past this
 *  many seconds the host has not heard the position and the row is a memory, not a fact. */
const FD_PRESENCE_DEAD_SECS = 15

/**
 * The FD SCORING CLASS a position is running — "CW" | "PH" | "DIG", the three modes Field Day
 * scores by, and the only thing this board can honestly compare.
 *
 * ⚠️ WHY THE CLASS AND NOT THE SUBMODE. `Engine::fd_position_report` puts the class on the wire
 * (`OperatingMode` → PH/CW/DIG) and never the submode, so FT8 and RTTY arrive here as the same
 * three letters — there is no finer comparison available even in principle. It is also the
 * comparison that matches the rules an operator is judged by: ARRL FD scores one QSO per band
 * per MODE CLASS (a station worked on 20m RTTY pays nothing again on 20m FT8) and Class A
 * permits one transmitted signal per band/mode, so two digital positions on one band are
 * splitting a single pool of contacts — a real conflict, not a false alarm. Comparing the raw
 * string instead would be the useless end of the trade: it would call "SSB" and "USB" a clash
 * and let the pool-splitting pair through.
 *
 * A peer on an older or third-party build can still report a raw on-air mode; route that through
 * the app's own classifier rather than dropping it into DIG by accident.
 */
function fdModeClass(mode: string): 'CW' | 'PH' | 'DIG' {
  const m = (mode || '').trim().toUpperCase()
  if (m === 'CW' || m === 'PH' || m === 'DIG') return m
  const cls = modeClassOf(m)
  return cls === 'CW' ? 'CW' : cls === 'Phone' ? 'PH' : 'DIG'
}

/**
 * THE COLLISION — the FD mode classes on this band that more than one LIVE position is running.
 * Empty means no conflict, and that is the common case.
 *
 * ⚠️ STALE POSITIONS DO NOT PARTICIPATE. Past `FD_PRESENCE_DEAD_SECS` the reading says where a
 * tent WAS; it may have moved and the report simply has not reached the host. Raising an alarm
 * off a reading we know is out of date is how a board earns the habit of being ignored, and the
 * alarm that matters is the one at 2 AM after that habit forms. The stale position is still
 * listed and still ⚠-marked — nothing is hidden, it just cannot ring the bell.
 */
function bandClashClasses(here: FdClubStatus['board']): string[] {
  const seen = new Map<string, number>()
  for (const r of here) {
    if (r.lastSeenSecs > FD_PRESENCE_DEAD_SECS) continue
    // ⚠️ AND NEITHER DOES A POSITION THAT HAS NOT WORKED ANYBODY. Presence carries DEFAULTS:
    // a freshly launched Nexus reports band "20m", mode DIG before a rig is even plugged in,
    // and the pump sends that first report the instant it connects. Three club laptops opened
    // on the Friday afternoon — or one logging-only seat, or a spare with no CAT — therefore
    // all announce 20m/DIG at once, and a detector that counted them would paint ⛔ CLASH
    // across the board before anyone had keyed a transmitter. That is the alarm-that-cries-
    // wolf failure exactly: by 2 AM, when a real collision costs contacts, nobody is looking
    // at it any more. A contact in the log is the cheapest available proof that a position is
    // genuinely on that band and mode rather than merely switched on.
    if (r.qsos === 0) continue
    const cls = fdModeClass(r.mode)
    seen.set(cls, (seen.get(cls) ?? 0) + 1)
  }
  return [...seen.entries()].filter(([, n]) => n > 1).map(([cls]) => cls)
}

/**
 * WHO IS ON WHICH BAND — one row per BAND, not per position.
 *
 * ⚠️ THE INVERSION IS THE WHOLE POINT, and the reason the club board did not answer this.
 * That board lists positions and puts each one's band in a column, which answers "what is
 * the CW tent doing?". The question an operator actually has, four times over, is the other
 * one: "which band can I move to?" — and to answer it from a position list you have to hold
 * every row in your head and invert it yourself, at 2 AM, while somebody waits. Here an
 * empty row IS the answer.
 *
 * Two stations on the same band is shown rather than hidden: at a multi-transmitter club
 * that is either a mistake about to cost a QSO or a deliberate CW/phone split, and the
 * operator is the one who can tell which.
 *
 * COLOUR carries the two readings this board is glanced at for. A busy band takes its own
 * `BAND_COLOR` — the same ink the map spots and the band picker use, so 20m looks like 20m
 * everywhere — while a free band stays dim and uncoloured, because the gap is what the eye is
 * hunting for. (In the light theme, where that palette is too pale to read as lettering, a busy
 * band's name takes the theme's ink and keeps its colour on an underline: the band chip's
 * light-theme fix, on a name with no border to carry it.) A band where two live positions share a
 * mode class goes to the alert ink with a ⛔ and the word CLASH: it is read from across a tent, so
 * it has to be wrong at a glance, and the word (not the colour) is what carries it to a
 * colour-blind operator or a screen reader.
 */
export function FdBandOccupancy({ club, big = false }: { club: FdClubStatus; big?: boolean }) {
  const byBand = new Map<string, typeof club.board>()
  for (const row of club.board) {
    const b = (row.band || '').trim()
    if (!b) continue
    const list = byBand.get(b) ?? []
    list.push(row)
    byBand.set(b, list)
  }
  // Bands somebody is on that are not in the standard list (an off-plan band, or one of the
  // barred ones worked by mistake) still appear — never hide a station that is transmitting.
  const extra = [...byBand.keys()].filter((b) => !FD_BOARD_BANDS.includes(b)).sort(
    (a, b) => bandRank(a) - bandRank(b),
  )
  const cell: CSSProperties = { fontSize: textPx(big ? 20 : 13), padding: big ? '4px 0' : '2px 0' }
  return (
    <div data-band-occupancy="" style={{ display: 'grid', gridTemplateColumns: 'auto 1fr', columnGap: big ? 18 : 10 }}>
      <span style={colHead(big)}>{t('fieldDay.club.bands.column.band')}</span>
      <span style={colHead(big)}>{t('fieldDay.club.bands.column.who')}</span>
      {[...FD_BOARD_BANDS, ...extra].map((band) => {
        const here = byBand.get(band) ?? []
        const busy = here.length > 0
        const clash = bandClashClasses(here)
        // The FD mode codes are invariant (FD_MODE_CODES), so the description names them as
        // they are logged — a club board that said "digital" where the log says DIG is a
        // second vocabulary to learn at 2 AM.
        const why = clash.length
          ? t('fieldDay.club.bands.clash.why', { band, mode: clash.join(' / ') })
          : undefined
        // Lettered in its band's colour: busy and no clash. The light theme re-inks exactly these
        // names and underlines them in that colour instead (styles.css, the band chip's rule), so
        // the attribute is what keeps a clash's alarm ink out of its reach.
        const bandInk = busy && clash.length === 0
        return (
          <Fragment key={band}>
            <span
              className="mono"
              data-band-ink={bandInk ? '' : undefined}
              style={{
                ...cell,
                fontWeight: 800,
                opacity: busy ? 1 : 0.5,
                color: clash.length
                  ? 'var(--alert-critical)'
                  : busy
                    ? bandColor(band)
                    : undefined,
                textDecorationColor: bandInk ? bandColor(band) : undefined,
              }}
            >
              {band}
            </span>
            <span
              className="mono"
              data-band-clash={clash.length ? band : undefined}
              title={why}
              style={{
                ...cell,
                opacity: busy ? 1 : 0.45,
                ...(clash.length
                  ? {
                      color: 'var(--alert-critical)',
                      fontWeight: 700,
                      background: 'color-mix(in srgb, var(--alert-critical) 18%, transparent)',
                      padding: big ? '4px 8px' : '2px 6px',
                      borderRadius: 4,
                    }
                  : null),
              }}
            >
              {clash.length > 0 && (
                <span aria-hidden="true">⛔ {t('fieldDay.club.bands.clash.mark')} · </span>
              )}
              {busy
                ? here
                    .map((r) => {
                      const who = r.posName || r.operator || t('fieldDay.club.board.unnamed')
                      const stale = r.lastSeenSecs > FD_PRESENCE_DEAD_SECS ? ' ⚠' : ''
                      return `${who} · ${r.mode}${stale}`
                    })
                    .join('   |   ')
                : t('fieldDay.club.bands.free')}
              {/* The alarm in words, for a reader who gets neither the ink nor the glyph. */}
              {why && <span className="sr-only"> {why}</span>}
            </span>
          </Fragment>
        )
      })}
    </div>
  )
}

/** Each link's mark beside its colour in the grid, so no state is told by colour alone, and the ⚠
 *  after a name the host has not heard lately ([`notHeard`]), as the table marks its line; the legend
 *  says what each is. A mark rather than the word: the word in every name would double each name's
 *  width in a grid that has a column for every band. */
const GRID_MARK = { connected: '✓', reconnecting: '↻', lost: '✕', stale: '⚠', self: '⌂' } as const
type GridLook = keyof typeof GRID_MARK

/** The grid's columns: the bands the contest runs on, from its rules data, or Field Day's own for
 *  the two Field Days (their rules name none), then any band a position is on that the list leaves
 *  out, in a column of its own, as the band occupancy board does: a station on the air never hides. */
function gridBands(named: string[] | undefined, fieldDay: boolean, rows: FdClubBoardRow[]): string[] {
  const listed = named?.length ? named : fieldDay ? FD_BOARD_BANDS : []
  const extra = [...new Set(rows.map((r) => r.band.trim()))]
    .filter((b) => b !== '' && !listed.includes(b))
    .sort((a, b) => bandRank(a) - bandRank(b) || a.localeCompare(b))
  return [...listed, ...extra]
}

/** One position in the grid: its name, coloured by its link where the board says one (the host's
 *  own screen), with the link's mark and, for a screen reader, its word; this laptop's own marked as
 *  this laptop. A board without the host's link words (a position's, Remote's) colours nothing, and
 *  there a position the host has not heard lately is greyed with ⚠ after its name and its word, as
 *  its line in the table is ([`notHeard`]); this laptop's own keeps its mark and edge as well. */
function GridPos({ row, self }: { row: FdClubBoardRow; self: boolean }) {
  const link = row.link === 'connected' || row.link === 'reconnecting' || row.link === 'lost' ? row.link : undefined
  const look: GridLook | undefined = self ? 'self' : link
  const word = look === 'self' ? linkWord('host') : link ? linkWord(link) : ''
  const stale = notHeard(row)
  return (
    <span
      className={`club-grid-pos${look ? ` link-${look}` : ''}${stale ? ' stale' : ''}`}
      title={
        stale
          ? t('fieldDay.club.board.stale', { secs: row.lastSeenSecs })
          : look === 'self'
            ? word
            : link
              ? `${word} ${heardWords(row)}`
              : undefined
      }
    >
      {/* The space is the line's break point: the name drops under its mark whole, where a mark
          joined to it made one word the line could only cut in two. */}
      {look && (
        <span className="club-grid-mark" aria-hidden="true">
          {`${GRID_MARK[look]} `}
        </span>
      )}
      <span className="club-grid-name">{boardName(row)}</span>
      {/* After the name, as the table puts it; the space lets it drop to a line of its own. */}
      {stale && (
        <span className="club-grid-mark" aria-hidden="true">
          {` ${GRID_MARK.stale}`}
        </span>
      )}
      {word && <span className="sr-only">{word}</span>}
      {stale && <span className="sr-only">{t('fieldDay.club.grid.notHeard', { secs: CLUB_DEAD_SECS })}</span>}
    </span>
  )
}

/**
 * ⭐ **WHERE EVERY POSITION IS, AT A GLANCE** — the club board as a band × mode grid, above its table:
 * the contest's bands across, its own mode classes down, each position's name in the cell of its
 * band and class. Two on one band and class are both in that cell, which at a club is a split pool
 * of contacts or a deliberate pair, and the operator is the one who can tell which.
 *
 * The rows are the Summary's idea of the contest's classes ([`modeColumns`]), not a second one: the
 * Illinois QSO Party's CW/DIG and PH, Field Day's CW, DIG and PH, and for a contest that counts no
 * class the ones its positions are on. Each name sits in the class its mode scores in
 * ([`fdModeClass`]), the only mode a position's report carries.
 *
 * On the host's own screen, where the board carries its link words, each name is coloured by its
 * link — green Connected, amber Reconnecting, red Lost — with a mark beside the colour and the word
 * for a reader, never the colour alone, and the legend under the grid says what each is. A position
 * the host removed is not in the grid (its line in the table keeps Allow back); one with no band yet
 * is named on a line under it. A position's board and Remote's carry no link words (Remote's capture
 * leaves them out), so there the grid places every name and colours none, and a position the host
 * has not heard lately is greyed and marked ⚠, exactly when its line in the table is ([`notHeard`]),
 * with a key in the legend. The two never meet on one name: on the host's own screen the link word
 * says it (Lost), and the table marks no line ⚠ there either.
 */
function ClubBandModeGrid({
  board,
  bands: named,
  dupeRule,
  fieldDay,
  linked,
  ownPosid,
  big,
}: {
  board: FdClubBoardRow[]
  bands?: string[]
  dupeRule?: DupeRule
  fieldDay: boolean
  /** The board carries the host's link words, so the legend says what their colours are; without
   *  them, what the ⚠ of a position not heard lately is, which only such a board shows. */
  linked: boolean
  ownPosid?: string
  big: boolean
}) {
  const shown = board.filter((r) => r.link !== 'removed')
  const placed = shown.filter((r) => r.band.trim() !== '')
  const waiting = shown.filter((r) => r.band.trim() === '')
  const bands = gridBands(named, fieldDay, placed)
  const modes = modeColumns(dupeRule, placed.map((r) => ({ mode: fdModeClass(r.mode) })))
  const isSelf = (r: FdClubBoardRow) => r.link === 'host' || (!!ownPosid && r.posid === ownPosid)
  const legend: GridLook[] = [
    ...(linked ? (['connected', 'reconnecting', 'lost'] as const) : (['stale'] as const)),
    ...(shown.some(isSelf) ? (['self'] as const) : []),
  ]
  if (shown.length === 0) return null
  return (
    <div className="club-grid-block" data-glance={big ? '' : undefined}>
      {bands.length > 0 && (
        <div className="club-grid">
          <table className="club-grid-table">
            <caption className="sr-only">{t('fieldDay.club.grid.caption')}</caption>
            <thead>
              <tr>
                <th scope="col" className="club-grid-mode">
                  {t('fieldDay.club.board.column.mode')}
                </th>
                {bands.map((b) => (
                  <th scope="col" key={b} className="club-grid-band">
                    {b}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {modes.map((m) => {
                const label = m.classes.join('/')
                return (
                  <tr key={label}>
                    <th scope="row" className="club-grid-mode">
                      {label}
                    </th>
                    {bands.map((b) => (
                      <td key={b} className="club-grid-cell">
                        {placed
                          .filter((r) => r.band.trim() === b && m.classes.includes(fdModeClass(r.mode)))
                          .map((r) => (
                            <GridPos key={r.posid} row={r} self={isSelf(r)} />
                          ))}
                      </td>
                    ))}
                  </tr>
                )
              })}
            </tbody>
          </table>
        </div>
      )}
      {waiting.length > 0 && (
        <div className="club-grid-waiting">
          <span className="club-grid-waiting-label">{t('fieldDay.club.grid.noBand')}</span>
          {waiting.map((r) => (
            <GridPos key={r.posid} row={r} self={isSelf(r)} />
          ))}
        </div>
      )}
      {legend.length > 0 && (
        // For the eye: every name already says its word to a screen reader.
        <div className="club-grid-legend" aria-hidden="true">
          {legend.map((look) => (
            <span key={look} className={look === 'stale' ? 'club-grid-pos stale' : `club-grid-pos link-${look}`}>
              <span className="club-grid-mark">{`${GRID_MARK[look]} `}</span>
              {look === 'stale'
                ? t('fieldDay.club.grid.notHeard', { secs: CLUB_DEAD_SECS })
                : look === 'self'
                  ? linkWord('host')
                  : linkWord(look)}
            </span>
          ))}
        </div>
      )}
    </div>
  )
}

/** The log table's columns for the contest a club runs, as the contest screen draws its own log. */
export interface ClubLogColumns {
  /** The contest's received slots (`FieldDayStatus.receives`): one column each, but at Field Day. */
  receives: readonly ContestFieldSpec[]
  /** The session's role, which a column's caption depends on. */
  role?: string
  isWfd: boolean
}

/** How often the host's own screen reads its club log while the list shows, besides the moment the
 *  club's count moves: a contact taken back and another merged between two snapshots leave the count
 *  where it was, and a position renamed moves no count at all. */
const CLUB_LOG_READ_MS = 2000

/**
 * ⭐ THE CLUB LOG, ON THE HOST'S OWN SCREEN: every position's contacts as this host merged them,
 * newest first, with the position that logged each, so the host can watch them arrive (the operator,
 * benching two laptops: "I can see the number on the board on the host computer here but it doesnt
 * show in the host master log").
 *
 * The rows are the engine's (`fdClubLogNewest`), read from the rows the Club ADIF and Club Cabrillo are
 * written from, by the same reading: the newest hundred at most, with how many the club holds, so a
 * long contest costs this screen what a short one does. A duplicate is the engine's verdict, the
 * files' own, drawn by the row this station's own log is drawn by, so it is marked as a dupe is there.
 * Each position is named as its line on the board above names it. Read-only: nothing here is pressed.
 *
 * The host's alone (`FdClubSection`'s `clubLog`): a joined laptop holds no rows, and Remote's page is
 * sent none of them.
 */
function ClubLogList({
  club,
  fieldDay,
  columns,
  big,
}: {
  club: FdClubStatus
  fieldDay: boolean
  columns: ClubLogColumns
  big: boolean
}) {
  const [log, setLog] = useState<FdClubLog | null>(null)
  // Read now, the moment the club's count moves, and every CLUB_LOG_READ_MS between.
  useEffect(() => {
    let live = true
    const read = () => {
      fdClubLogNewest().then(
        (got) => {
          if (live) setLog(got)
        },
        () => {},
      )
    }
    read()
    const id = window.setInterval(read, CLUB_LOG_READ_MS)
    return () => {
      live = false
      window.clearInterval(id)
    }
  }, [club.qsos])
  const names = useMemo(() => new Map(club.board.map((b) => [b.posid, boardName(b)])), [club.board])
  if (!log) return null
  const slotColumns = fieldDay ? [] : columns.receives
  return (
    <div className={big ? 'fd-log fd-club-log-big' : 'fd-log'} role="region" aria-label={t('fieldDay.club.log.head')}>
      <div style={{ ...CLUB_HEADER, gap: 8, fontSize: textPx(big ? 15 : 12) }} title={t('fieldDay.club.log.title')}>
        <span style={{ fontWeight: 700, color: 'var(--text)' }}>{t('fieldDay.club.log.head')}</span>
        <span style={{ color: 'var(--text-dim)' }}>
          {log.total > log.rows.length
            ? t('fieldDay.club.log.newest', { shown: log.rows.length, total: log.total })
            : t('fieldDay.club.log.count', { count: log.total })}
        </span>
      </div>
      <ContestLogHead
        fieldDayColumns={fieldDay}
        isWfd={columns.isWfd}
        slotColumns={slotColumns}
        role={columns.role}
        position
      />
      <div className="fd-log-scroll">
        {log.rows.length === 0 && <p className="empty">{t('fieldDay.club.log.empty')}</p>}
        {log.rows.map((r) => (
          <ContestLogRow
            key={`${r.posid}:${r.seq}`}
            row={{ qso: r, isNewSection: false, isDupe: r.dupe === true }}
            fieldDayColumns={fieldDay}
            slotColumns={slotColumns}
            position={names.get(r.posid) ?? (r.posName || t('fieldDay.club.board.unnamed'))}
          />
        ))}
      </div>
    </div>
  )
}

export function FdClubSection({
  club,
  onExport,
  onGivePosition,
  onRemovePosition,
  onAllowBack,
  busy = false,
  detached = false,
  readOnly = false,
  fieldDay = true,
  keepsDupes = false,
  link = null,
  bands,
  dupeRule,
  ownPosid,
  clubLog,
}: {
  club: FdClubStatus
  onExport?: (format: 'club-cabrillo' | 'club-adif') => void
  /** The host's own contest screen only: give a club position to the laptop a turned-away
   *  entry names, by the entry's `handle`. Not passed on Remote, in the pop-out or on a
   *  position, so no Give button shows there. */
  onGivePosition?: (handle: number) => Promise<FdGiveAnswer>
  /** The host's own contest screen only, as `onGivePosition` is: remove a position from the club
   *  event by its id, and let a removed one back. Each row of the host's board gets the button
   *  that applies, but the host's own. */
  onRemovePosition?: (posid: string) => Promise<FdRemoveAnswer>
  onAllowBack?: (posid: string) => Promise<FdRemoveAnswer>
  busy?: boolean
  detached?: boolean
  readOnly?: boolean
  /** The club runs one of the two Field Days. Any other contest has no sections, so its
   *  counters leave them out rather than show a zero that means nothing. */
  fieldDay?: boolean
  /** The contest logs its duplicates (`DupeRule::log_dupes`), so the club file keeps a
   *  repeat as a zero-point line instead of keeping only the earliest — except a Field Day
   *  club's, which keeps only the earliest whatever the contest logs (`export_indices`). */
  keepsDupes?: boolean
  /** This laptop's club link: the lines the block does not already say. The contest screen's
   *  own block only, never the pop-out or Remote. */
  link?: ClubLink | null
  /** The running contest's bands and dupe rule (`FieldDayStatus.bands`, `.dupeRule`): the grid's
   *  columns and its mode classes. */
  bands?: string[]
  dupeRule?: DupeRule
  /** This laptop's own position id (`Settings.fdPositionId`), whose name the grid marks as this
   *  laptop on a position's board. Not on Remote, which is no laptop of the club's. */
  ownPosid?: string
  /** ⭐ The host's club log under the board (`ClubLogList`), and the log table's columns of the
   *  contest the club runs: the host's contest screen and its club board's own window pass it. Not
   *  Remote's contest view, whose page is sent none of the club's rows, and shown only while this
   *  laptop hosts: a joined laptop holds the club's counts and keys, never its rows. */
  clubLog?: ClubLogColumns
}) {
  // The glance scale. One flag, applied at the handful of places that carry a
  // px size, so the docked board is byte-for-byte what it was.
  const big = detached
  // What the club FILE keeps, which is what its export buttons say: every contest logs its
  // dupes, but a Field Day club writes its earliest-wins unique rows, as 1.x wrote them.
  const fileKeepsDupes = keepsDupes && !fieldDay
  // What the last Give press did, until the next one.
  const [giveNote, setGiveNote] = useState<{ text: string; alert: boolean } | null>(null)
  // The entry a Give press asks about, until the host answers: the position goes only to the
  // laptop whose own screen shows this code. Pinned to the entry's handle, so the list moving
  // under the pointer cannot change which laptop is asked about, and asked only while that entry
  // is on the list.
  const [asking, setAsking] = useState<{ handle: number; name: string; call: string; code: string } | null>(null)
  const askingListed = asking != null && (club.refused ?? []).some((r) => r.handle === asking.handle)
  // The codes are this laptop's screen's and the host's, never the pop-out's or Remote's.
  const showCodes = !detached && !readOnly
  function ask(handle: number, r: { posName: string; call: string; clubCode?: string }) {
    // Only at the host: a press that comes through a stream gives no position, nor one from a
    // computer driving the station (the station refuses it there). Read before anything awaits,
    // while the stream's dispatch is still the one running.
    if (isStreamInput() || drivesAnotherStation()) {
      setGiveNote({ text: t('fieldDay.club.give.refused.remote'), alert: true })
      return
    }
    setGiveNote(null)
    setAsking({ handle, name: r.posName, call: r.call, code: r.clubCode ?? '' })
  }
  function give() {
    if (isStreamInput() || drivesAnotherStation()) {
      setGiveNote({ text: t('fieldDay.club.give.refused.remote'), alert: true })
      return
    }
    if (!asking) return
    const { handle, name: posName, call } = asking
    const name = posName || call
    setAsking(null)
    onGivePosition?.(handle).then(
      (answer) =>
        setGiveNote(
          answer.outcome === 'given'
            ? { text: t('fieldDay.club.give.given', { name }), alert: false }
            : {
                text:
                  answer.refusal === 'notHosting'
                    ? t('fieldDay.club.give.refused.notHosting')
                    : t('fieldDay.club.give.refused.stale'),
                alert: true,
              },
        ),
      () => setGiveNote({ text: t('fieldDay.club.give.failed'), alert: true }),
    )
  }
  // The host's link words ride its own board alone (a position's board and Remote's copy have
  // none), and its Remove and Allow back are on its own contest screen alone.
  const showLink = club.hosting && club.board.some((r) => r.link != null)
  const manage = showLink && !detached && !readOnly && onRemovePosition != null && onAllowBack != null
  // What the last Remove or Allow back did, until the next press.
  const [removeNote, setRemoveNote] = useState<{ text: string; alert: boolean } | null>(null)
  // The position a Remove press asks about, by id, until it is answered: asked only while that
  // position is on the board and not removed already.
  const [removing, setRemoving] = useState<{ posid: string; name: string } | null>(null)
  const removingListed =
    removing != null && club.board.some((r) => r.posid === removing.posid && r.link !== 'removed' && r.link !== 'host')
  const remoteNote = { text: t('fieldDay.club.remove.refused.remote'), alert: true }
  function refusedNote(refusal: 'notHosting' | 'own' | 'stale') {
    return {
      text:
        refusal === 'notHosting'
          ? t('fieldDay.club.remove.refused.notHosting')
          : refusal === 'own'
            ? t('fieldDay.club.remove.refused.own')
            : t('fieldDay.club.remove.refused.stale'),
      alert: true,
    }
  }
  function askRemove(row: FdClubBoardRow) {
    // Only at the host, as Give is: read before anything awaits.
    if (isStreamInput() || drivesAnotherStation()) {
      setRemoveNote(remoteNote)
      return
    }
    setRemoveNote(null)
    setRemoving({ posid: row.posid, name: boardName(row) })
  }
  function remove() {
    if (isStreamInput() || drivesAnotherStation()) {
      setRemoveNote(remoteNote)
      return
    }
    if (!removing) return
    const { posid, name } = removing
    setRemoving(null)
    onRemovePosition?.(posid).then(
      (answer) =>
        setRemoveNote(
          answer.outcome === 'done'
            ? { text: t('fieldDay.club.remove.done', { name }), alert: false }
            : refusedNote(answer.refusal),
        ),
      () => setRemoveNote({ text: t('fieldDay.club.remove.failed'), alert: true }),
    )
  }
  function allowBack(row: FdClubBoardRow) {
    if (isStreamInput() || drivesAnotherStation()) {
      setRemoveNote(remoteNote)
      return
    }
    const name = boardName(row)
    setRemoveNote(null)
    onAllowBack?.(row.posid).then(
      (answer) =>
        setRemoveNote(
          answer.outcome === 'done'
            ? { text: t('fieldDay.club.allowBack.done', { name }), alert: false }
            : refusedNote(answer.refusal),
        ),
      () => setRemoveNote({ text: t('fieldDay.club.remove.failed'), alert: true }),
    )
  }
  return (
    <div
      style={
        big
          ? // Torn off, the board IS the window: it takes the height and scrolls, rather
            // than sitting flex:0 above five sibling blocks that are not here.
            { ...CLUB_WRAP, flex: '1 1 auto', minHeight: 0, overflowY: 'auto', gap: 12, padding: '16px 22px 18px', borderBottom: 'none' }
          : CLUB_WRAP
      }
      aria-label={t('fieldDay.club.aria')}
    >
      <div style={big ? { ...CLUB_HEADER, gap: 14 } : CLUB_HEADER}>
        <span style={{ fontSize: textPx(big ? 17 : 13), fontWeight: 700, color: 'var(--text)' }}>
          {t('fieldDay.club.head')}
        </span>
        <span
          style={big ? { ...clubChipStyle(club.syncState), fontSize: textPx(14), padding: '4px 12px' } : clubChipStyle(club.syncState)}
          title={t('fieldDay.club.state.title')}
        >
          {clubChipText(club)}
        </span>
        {(club.event || club.hostCall) && (
          <span style={{ fontSize: textPx(big ? 15 : 12), color: 'var(--text-dim)' }}>
            {t('fieldDay.club.hostLine', { event: club.event || '—', call: club.hostCall || '—' })}
          </span>
        )}
        {/* This laptop's own club code: what the host checks before it gives it a position. */}
        {showCodes && club.clubCode && (
          <span className="mono" data-club-code="" style={{ ...CLUB_CODE, fontSize: textPx(12) }} title={t('fieldDay.club.code.title')}>
            {t('fieldDay.club.code', { code: club.clubCode })}
          </span>
        )}
        <span style={{ flex: '1 1 auto' }} />
        <span style={{ fontSize: textPx(big ? 16 : 13), color: 'var(--text-dim)' }}>
          {fieldDay
            ? t('fieldDay.club.counters', {
                score: club.score,
                qsos: club.qsos,
                sections: club.sections,
              })
            : t('fieldDay.club.countersContest', { score: club.score, qsos: club.qsos })}
        </span>
        {club.hosting && onExport && (
          <>
            <button
              type="button"
              className="export-btn"
              disabled={busy}
              onClick={() => onExport('club-cabrillo')}
              title={
                fileKeepsDupes
                  ? t('fieldDay.club.export.cabrillo.titleKept')
                  : t('fieldDay.club.export.cabrillo.title')
              }
            >
              {t('fieldDay.club.export.cabrillo.label')}
            </button>
            <button
              type="button"
              className="export-btn"
              disabled={busy}
              onClick={() => onExport('club-adif')}
              title={
                fileKeepsDupes
                  ? t('fieldDay.club.export.adif.titleKept')
                  : t('fieldDay.club.export.adif.title')
              }
            >
              {t('fieldDay.club.export.adif.label')}
            </button>
          </>
        )}
        {!detached && !readOnly && (
          <button
            type="button"
            className="export-btn"
            onClick={() => void openPanelWindow('fdclub')}
            title={t('fieldDay.club.popOut.title')}
          >
            {t('fieldDay.club.popOut.label')}
          </button>
        )}
      </div>
      {link && <ClubLinkRows lines={clubLinkLines(link, true)} />}
      {/* This PC's clock against the host's: said from 2 s, a warning past 30 s. Shown
          only — Nexus never changes a clock. */}
      {Math.abs(club.skewSecs) > CLOCK_WARN_SECS ? (
        <div style={CLUB_WARN} role="alert">
          {t('fieldDay.club.skew', { secs: Math.abs(club.skewSecs) })}
        </div>
      ) : Math.abs(club.skewSecs) >= CLOCK_SHOW_SECS ? (
        <div style={CLUB_NOTE}>
          {club.skewSecs > 0
            ? t('fieldDay.club.clock.ahead', { secs: club.skewSecs })
            : t('fieldDay.club.clock.behind', { secs: -club.skewSecs })}
        </div>
      ) : null}
      {club.lastError && (
        <div style={CLUB_WARN} role="alert">
          {t('fieldDay.club.error', { msg: club.lastError })}
        </div>
      )}
      {/* The host's alone: the laptops it turned away, each with the sentence it was sent,
          which that laptop's own screen shows as its host error. One turned away because another
          laptop holds its position has a Give button, on the host's own contest screen alone. */}
      {club.refused?.map(({ handle, ...r }, i) => (
        <div key={handle !== undefined ? `h${handle}` : `i${i}`} style={CLUB_WARN} role="alert">
          {r.posName
            ? t('fieldDay.club.refusedPosition', { name: r.posName, call: r.call, reason: r.reason })
            : t('fieldDay.club.refusedCall', { call: r.call, reason: r.reason })}
          {/* The laptop's club code, the one thing on the line it cannot choose: the same code
              that laptop's own screen shows. */}
          {showCodes && r.clubCode && (
            <>
              {' '}
              <span className="mono" data-club-code="" style={CLUB_CODE}>
                {t('fieldDay.club.code', { code: r.clubCode })}
              </span>
            </>
          )}
          {club.hosting && onGivePosition && handle !== undefined && (
            <>
              {' '}
              <button
                type="button"
                className="export-btn"
                onClick={() => ask(handle, r)}
                title={t('fieldDay.club.give.title')}
              >
                {t('fieldDay.club.give.label')}
              </button>
            </>
          )}
        </div>
      ))}
      {/* Before the host gives a position: the code of the laptop it would go to, to check
          against the code on that laptop's own screen. */}
      {asking && askingListed && (
        <div role="group" aria-label={t('fieldDay.club.give.confirm.aria')} style={CLUB_CONFIRM}>
          <div>{t('fieldDay.club.give.confirm.head')}</div>
          <span className="mono" data-club-code="" style={{ ...CLUB_CODE, fontSize: textPx(22), padding: '2px 10px' }}>
            {asking.code}
          </span>
          <div>
            {t('fieldDay.club.give.confirm.check')}{' '}
            {asking.name
              ? t('fieldDay.club.give.confirm.says', { name: asking.name, call: asking.call })
              : t('fieldDay.club.give.confirm.saysCall', { call: asking.call })}
          </div>
          <div style={{ display: 'flex', flexWrap: 'wrap', gap: 8 }}>
            <button type="button" className="export-btn" onClick={give}>
              {t('fieldDay.club.give.confirm.yes')}
            </button>
            <button type="button" className="export-btn" onClick={() => setAsking(null)}>
              {t('fieldDay.club.give.confirm.no')}
            </button>
          </div>
        </div>
      )}
      {giveNote && (
        <div style={giveNote.alert ? CLUB_WARN : CLUB_NOTE} role={giveNote.alert ? 'alert' : 'status'}>
          {giveNote.text}
        </div>
      )}
      {removeNote && (
        <div style={removeNote.alert ? CLUB_WARN : CLUB_NOTE} role={removeNote.alert ? 'alert' : 'status'}>
          {removeNote.text}
        </div>
      )}
      {/* The host's alone, for the rest of the event: every contact it kept out of the club's
          log, how many and the newest of them, each with the position that sent it and why. */}
      {club.keptOut && (
        <div style={CLUB_WARN} role="status">
          <div>{t('fieldDay.club.keptOut.total', { count: club.keptOut.total })}</div>
          {club.keptOut.latest.map((k, i) => (
            <div key={i}>
              {k.posName
                ? t('fieldDay.club.keptOut.position', { name: k.posName, call: k.call, reason: k.reason })
                : t('fieldDay.club.keptOut.call', { call: k.call, reason: k.reason })}
            </div>
          ))}
          {club.keptOut.total > club.keptOut.latest.length && (
            <div>{t('fieldDay.club.keptOut.more', { count: club.keptOut.total - club.keptOut.latest.length })}</div>
          )}
        </div>
      )}
      {/* The host's alone: each position whose newest contact sent another county (or state,
          province or section) than the club sends, named as its line on the board names it, with
          both. A warning: its contacts are still taken, as sent. */}
      {club.otherLocation?.map((o, i) => (
        <div key={`${o.posName}-${i}`} style={CLUB_WARN} role="status">
          {t('fieldDay.club.otherLocation', {
            name: o.posName || o.operator || t('fieldDay.club.board.unnamed'),
            sent: o.sent,
            club: o.club,
          })}
        </div>
      ))}
      {/* The host's alone: its board is as big as the club line every position is sent can
          carry — said before one more position might not fit, and once it is cut. */}
      {club.boardFull && (
        <div style={CLUB_WARN} role="status">
          {club.boardFull.shown < club.boardFull.positions
            ? t('fieldDay.club.boardCut', { shown: club.boardFull.shown, positions: club.boardFull.positions })
            : t('fieldDay.club.boardFull', { positions: club.boardFull.positions })}
        </div>
      )}
      {/* Who is on which band and mode, at a glance, above the table that details each position. */}
      <ClubBandModeGrid
        board={club.board}
        bands={bands}
        dupeRule={dupeRule}
        fieldDay={fieldDay}
        linked={showLink}
        ownPosid={ownPosid}
        big={big}
      />
      {club.board.length === 0 ? (
        // Sync IS on here (the block only rides the snapshot when it is), so this
        // says what it is waiting for and never sends anyone to Settings — the
        // torn-off window's own copy covers the sync-off case.
        <span style={{ fontSize: textPx(big ? 16 : 12), color: 'var(--text-faint)' }}>
          {t('fieldDay.club.board.empty')}
        </span>
      ) : (
        <div
          data-club-board=""
          style={{
            ...(big ? { ...CLUB_BOARD_GRID, fontSize: textPx(20), columnGap: 18, rowGap: 8 } : CLUB_BOARD_GRID),
            ...(club.hosting ? { gridTemplateColumns: clubBoardColumns(showLink, manage) } : {}),
          }}
        >
          <span data-head="" style={colHead(big)}>{t('fieldDay.club.board.column.position')}</span>
          {/* The host's: where each position's link stands, and when the host last heard it. */}
          {showLink && (
            <span
              data-head=""
              style={colHead(big)}
              title={t('fieldDay.club.board.link.title', {
                beat: CLUB_PING_SECS,
                window: 2 * CLUB_PING_SECS,
                dead: CLUB_DEAD_SECS,
              })}
            >
              {t('fieldDay.club.board.column.link')}
            </span>
          )}
          <span data-head="" style={colHead(big)}>{t('fieldDay.club.board.column.band')}</span>
          <span data-head="" style={colHead(big)}>{t('fieldDay.club.board.column.mode')}</span>
          <span data-head="" style={colHead(big)}>{t('fieldDay.club.board.column.operator')}</span>
          <span data-head="" style={colHead(big)}>{t('fieldDay.club.board.column.qsos')}</span>
          <span data-head="" style={colHead(big)}>{t('fieldDay.club.board.column.rate')}</span>
          {/* The host's column: each position's clock, as it reported it to the host. */}
          {club.hosting && (
            <span data-head="" style={colHead(big)} title={t('fieldDay.club.board.clock.title')}>
              {t('fieldDay.club.board.column.clock')}
            </span>
          )}
          {/* The host's own contest screen: each row's Remove or Allow back. */}
          {manage && <span data-head="" aria-hidden="true" />}
          {club.board.map((row) => {
            // A host's row says where its link stands, so it is dimmed when that is lost or removed;
            // a position's board stale-marks past 15 s (the DEAD_SECS threshold). Either way a
            // reading stays on screen but is never silently stale.
            const stale = row.link
              ? row.link === 'lost' || row.link === 'removed'
              : notHeard(row)
            const marked = stale && !row.link
            // A position's board has no Status column, and one link word: a position its host's
            // operator removed. The word goes after the name, as the host's column says it.
            const removedHere = !showLink && row.link === 'removed'
            const dim: CSSProperties = stale ? { opacity: 0.45 } : {}
            return (
              <Fragment key={row.posid}>
                <span
                  className="mono"
                  data-club-row={row.posid}
                  style={{ ...dim, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}
                  title={
                    marked
                      ? t('fieldDay.club.board.stale', { secs: row.lastSeenSecs })
                      : removedHere
                        ? linkWord('removed')
                        : undefined
                  }
                >
                  {boardName(row)}
                  {marked && <span aria-hidden="true"> ⚠</span>}
                  {removedHere && <span> · {linkWord('removed')}</span>}
                </span>
                {showLink && <LinkCell row={row} />}
                <span className="mono" data-club-row={row.posid} style={dim}>{row.band}</span>
                <span className="mono" data-club-row={row.posid} style={dim}>{row.mode}</span>
                <span className="mono" data-club-row={row.posid} style={{ ...dim, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{row.operator}</span>
                <span className="mono" data-club-row={row.posid} style={dim}>{row.qsos}</span>
                <span className="mono" data-club-row={row.posid} style={dim}>
                  {t('fieldDay.club.board.rate', { rate: row.rate })}
                </span>
                {club.hosting && (
                  <span
                    className="mono"
                    data-club-row={row.posid}
                    style={{
                      ...dim,
                      overflow: 'hidden',
                      textOverflow: 'ellipsis',
                      whiteSpace: 'nowrap',
                      ...(row.clockMs != null && Math.abs(clockSecs(row.clockMs)) > CLOCK_WARN_SECS
                        ? { color: 'var(--status-new-entity)' }
                        : {}),
                    }}
                    title={row.clockMs == null ? t('fieldDay.club.board.clock.unknown') : undefined}
                  >
                    {clockCell(row.clockMs)}
                  </span>
                )}
                {manage && (
                  <span data-club-row={row.posid}>
                    {row.link === 'removed' ? (
                      <button
                        type="button"
                        className="export-btn"
                        onClick={() => allowBack(row)}
                        title={t('fieldDay.club.allowBack.title')}
                      >
                        {t('fieldDay.club.allowBack.label')}
                      </button>
                    ) : row.link && row.link !== 'host' ? (
                      <button
                        type="button"
                        className="export-btn"
                        onClick={() => askRemove(row)}
                        title={t('fieldDay.club.remove.title')}
                      >
                        {t('fieldDay.club.remove.label')}
                      </button>
                    ) : null}
                  </span>
                )}
                {/* The question a Remove press asks, under the row it was pressed on. */}
                {manage && removingListed && removing?.posid === row.posid && (
                  <div
                    role="group"
                    aria-label={t('fieldDay.club.remove.confirm.aria')}
                    style={{ ...CLUB_CONFIRM, gridColumn: '1 / -1', fontSize: textPx(big ? 16 : 12) }}
                  >
                    <div>{t('fieldDay.club.remove.confirm.head', { name: removing.name })}</div>
                    <div>{t('fieldDay.club.remove.confirm.body')}</div>
                    <div style={{ display: 'flex', flexWrap: 'wrap', gap: 8 }}>
                      <button type="button" className="export-btn" onClick={remove}>
                        {t('fieldDay.club.remove.confirm.yes')}
                      </button>
                      <button type="button" className="export-btn" onClick={() => setRemoving(null)}>
                        {t('fieldDay.club.remove.confirm.no')}
                      </button>
                    </div>
                  </div>
                )}
              </Fragment>
            )
          })}
        </div>
      )}
      {clubLog && club.hosting && !readOnly && (
        <ClubLogList club={club} fieldDay={fieldDay} columns={clubLog} big={big} />
      )}
    </div>
  )
}

/**
 * The contacts in this computer's contest journal that entering the contest did not load,
 * by reason, and that nothing was deleted — the journal is one file per position whatever
 * contest it last ran, and the session loads only its own contest's rows from this running.
 */
function FdKeptOutNote({
  keptOut,
  contest,
}: {
  keptOut: { otherContest: number; otherRunning: number }
  contest: string
}) {
  return (
    <p className="fd-score-note" role="note">
      {keptOut.otherRunning > 0 && <>{t('fieldDay.keptOut.running', { count: keptOut.otherRunning, contest })} </>}
      {keptOut.otherContest > 0 && <>{t('fieldDay.keptOut.contest', { count: keptOut.otherContest })} </>}
      {t('fieldDay.keptOut.kept')}
    </p>
  )
}

/**
 * ⭐ The contacts in this computer's contest journal that no session loads any more (logged more
 * than four days ago) and the logbook does not hold — how many, from which contest and when —
 * with the merge for each contest. The journal keeps them until they are merged; the merge is the
 * end-of-contest one's rule, so a press never writes a contact twice. The report stays when the
 * offer it answered goes.
 */
function FdUnmergedNote({
  unmerged,
  busy,
  report,
  error,
  onMerge,
}: {
  unmerged: ContestUnmerged[]
  busy: string | null
  report: FdMergeReport | null
  error: string | null
  onMerge: (event: string) => void
}) {
  return (
    <>
      {unmerged.map((u) => {
        const contest = contestName(u.event)
        const [first, last] = [utcDay(u.firstUnix), utcDay(u.lastUnix)]
        return (
          <div className="fd-merge" key={u.event}>
            <p className="fd-score-note" role="note">
              {first === last
                ? t('fieldDay.unmerged.day', { count: u.count, contest, date: first })
                : t('fieldDay.unmerged.days', { count: u.count, contest, first, last })}
            </p>
            <button
              type="button"
              className="export-btn fd-merge-btn"
              disabled={busy !== null}
              onClick={() => onMerge(u.event)}
              title={t('fieldDay.unmerged.title')}
            >
              {busy === u.event ? t('fieldDay.merge.busy') : t('fieldDay.unmerged.merge', { count: u.count })}
            </button>
          </div>
        )
      })}
      {(report || error) && (
        <div className="fd-merge">
          {report && (
            <span className="fd-merge-report" role="status">
              {t('fieldDay.merge.added', { count: report.added })}
              {' · '}
              {t('fieldDay.merge.already', { count: report.already })}
              {report.refused > 0 && <> {' · '}{t('fieldDay.merge.refused', { count: report.refused })}</>}
            </span>
          )}
          {error && <span className="log-export-error" role="alert">{error}</span>}
        </div>
      )}
    </>
  )
}

/** A Unix time's UTC day, YYYY-MM-DD: the date a contest log gives a contact. */
function utcDay(unix: number): string {
  return new Date(unix * 1000).toISOString().slice(0, 10)
}

/**
 * Club sync switched on for a contest it cannot run, said where the club block would be.
 *
 * The engine refuses it (`clubSyncRefusal`, `Engine::club_sync_refusal`): the club log runs
 * the picked contest's own rules, and refuses only a contest whose merged log would be wrong
 * whoever built it — a serial-number exchange, or a sponsor template with a transmitter
 * column. With nothing syncing there is no club block, so without this the screen would
 * simply go quiet about a switch the operator turned on.
 */
export function FdClubRefused({ contest, why }: { contest: string; why: ClubSyncRefusal }) {
  return (
    <div style={CLUB_WRAP} aria-label={t('fieldDay.club.aria')}>
      <div style={CLUB_HEADER}>
        <span style={{ fontSize: textPx(13), fontWeight: 700, color: 'var(--text)' }}>
          {t('fieldDay.club.head')}
        </span>
        <span style={clubChipStyle('refused')}>{t('fieldDay.club.refused.chip')}</span>
      </div>
      <div style={CLUB_WARN} role="status">
        {clubSyncRefusalText(why, contest)}
      </div>
    </div>
  )
}

/**
 * Where this laptop's club link stands, said where the club block would be while the Field Day
 * mode is off: there is no club block then, and a laptop told to join a host said nothing at
 * all about whether it had.
 */
export function FdClubLink({ link }: { link: ClubLink }) {
  return (
    <div style={CLUB_WRAP} aria-label={t('fieldDay.club.aria')}>
      <div style={CLUB_HEADER}>
        <span style={{ fontSize: textPx(13), fontWeight: 700, color: 'var(--text)' }}>
          {t('fieldDay.club.head')}
        </span>
      </div>
      <ClubLinkRows lines={clubLinkLines(link)} />
    </div>
  )
}

// Scoreboard header (operator + pop-out) — inline off the shared tokens so it stays
// theme-aware without touching styles.css, like SectionsBoard above.
const SCOREBOARD_HEADER: CSSProperties = {
  display: 'flex',
  alignItems: 'center',
  gap: 12,
  padding: '10px 14px',
  borderBottom: '1px solid var(--border-soft)',
}
const OP_FIELD: CSSProperties = {
  display: 'flex',
  alignItems: 'center',
  gap: 8,
  flex: '1 1 auto',
  minWidth: 0,
  // A lone control: on a screen wider than its old 1600px cap it stops at a field's width
  // (`--fd-control-measure`, set by the panel on lg/xl) rather than crossing the window.
  maxWidth: 'var(--fd-control-measure, none)',
}
const OP_LABEL: CSSProperties = {
  fontSize: textPx(10),
  fontWeight: 700,
  letterSpacing: '0.04em',
  textTransform: 'uppercase',
  color: 'var(--text-faint)',
  whiteSpace: 'nowrap',
}
const OP_INPUT: CSSProperties = {
  flex: '1 1 auto',
  minWidth: 0,
  padding: '7px 11px',
  fontFamily: 'var(--font-mono)',
  fontSize: textPx(15),
  fontWeight: 700,
  letterSpacing: '0.04em',
  textTransform: 'uppercase',
  color: 'var(--text)',
  background: 'var(--bg-elev)',
  border: '1px solid var(--border)',
  borderRadius: 'var(--radius-sm)',
}
const POPOUT_BTN: CSSProperties = {
  flex: '0 0 auto',
  display: 'inline-flex',
  alignItems: 'center',
  gap: 6,
  padding: '7px 12px',
  fontSize: textPx(13),
  fontWeight: 600,
  color: 'var(--text-dim)',
  background: 'var(--bg-elev)',
  border: '1px solid var(--border-soft)',
  borderRadius: 'var(--radius-sm)',
  cursor: 'pointer',
  whiteSpace: 'nowrap',
}

// ---------------------------------------------------------------------------
// THE RATE METER — the tiles. The arithmetic and its provenance are in
// `features/contestRate.ts`; this is only how it reads on the scoreboard.
// ---------------------------------------------------------------------------

/** No reading yet. An em dash, not a zero: "0/h" is a rate an operator has been shown to
 *  be running, and one contact is not a rate at all. A named constant because it is a
 *  symbol rather than prose — the same escape hatch the invariant tokens above use. */
const RATE_NONE = '—'

/**
 * The meter's own clock.
 *
 * Every window is measured TO NOW, so the reading has to move when nothing is logged —
 * a meter that only updates when a contact lands is the frozen rate the module was
 * written to avoid, and it would sit at the run rate all through a two-hour break. Five
 * seconds is finer than the numbers it feeds and far cheaper than the snapshot poll
 * already re-rendering this view; it does not depend on that poll, because the torn-off
 * scoreboard must decay too.
 */
function useNowUnix(): number {
  const [now, setNow] = useState(() => Math.floor(Date.now() / 1000))
  useEffect(() => {
    const id = setInterval(() => setNow(Math.floor(Date.now() / 1000)), 5000)
    return () => clearInterval(id)
  }, [])
  return now
}

/**
 * ⭐ **THE RATE METER** — contacts per hour, on the scoreboard beside the
 * score tiles, for every contest the picker offers including both Field Day events. A
 * Field Day operator watches rate as closely as a contester does.
 *
 * Three `.fd-score` tiles in the scoreboard's own wrap row — the shipped tile idiom, no
 * new structure and no new CSS, so the pane grid is untouched.
 *
 * ⚠️ **THE LABEL NAMES THE REAL SAMPLE, NOT THE NOMINAL WINDOW.** With three contacts
 * logged the tile reads "Last 3", because "Last 10" over three contacts is a claim about
 * seven contacts that do not exist. And two windows over the SAME contacts are one
 * reading, not two: below eleven contacts the 100-window can only repeat the 10-window's
 * answer, so it is not drawn at all rather than drawn as a second identical tile.
 */
export function RateTiles({ log, nowUnix }: { log: FieldDayQso[]; nowUnix: number }) {
  const { windows, lastHour } = useMemo(() => contestRate(log, nowUnix), [log, nowUnix])
  const shown = windows
    .map((w, i) => ({ w, size: RATE_WINDOWS[i] }))
    .filter((e, i, all) => i === 0 || e.w.sample > all[i - 1].w.sample)
  return (
    <>
      {shown.map(({ w, size }) => (
        <div
          className="fd-score"
          key={size}
          /* `role="group"` is what makes the aria-label authoritative: an aria-label on a
             bare <div> with no role is ignored by screen readers, and "60" alone is not a
             reading of anything. */
          role="group"
          title={t('fieldDay.rate.window.title', { n: w.sample })}
          aria-label={t('fieldDay.rate.window.aria', { n: w.sample })}
        >
          <span className="fd-score-val">
            {w.perHour === null ? RATE_NONE : Math.round(w.perHour)}
          </span>
          <span className="fd-score-label">{t('fieldDay.rate.lastN', { n: w.sample })}</span>
        </div>
      ))}
      {/* The rolling hour is a COUNT, so 0 is a real answer and never an em dash. */}
      <div
        className="fd-score"
        role="group"
        title={t('fieldDay.rate.hour.title')}
        aria-label={t('fieldDay.rate.hour.aria')}
      >
        <span className="fd-score-val">{lastHour}</span>
        <span className="fd-score-label">{t('fieldDay.rate.hour')}</span>
      </div>
    </>
  )
}

/**
 * ⭐ **ON TIME AND OFF TIME** — two `.fd-score` tiles beside the rate meter, the shipped tile idiom
 * as `RateTiles` uses it: a break of the contest's own minimum or more between two contacts is off
 * time (`offTimeRule`: 60 minutes in CQ WW and WPX, else 30), the rest from the first contact to the
 * last is on time (`features/contestOpTime`). Nothing before the first contact.
 */
export function OperatingTimeTiles({ log, minOffSec = OFF_TIME_MIN_SEC }: { log: FieldDayQso[]; minOffSec?: number }) {
  const op = useMemo(() => operatingTime(log, minOffSec), [log, minOffSec])
  if (!op) return null
  const minutes = minOffSec / 60
  return (
    <>
      <div
        className="fd-score"
        role="group"
        data-optime="on"
        title={t('fieldDay.opTime.on.title')}
        aria-label={t('fieldDay.opTime.on.aria', { time: formatOpTime(op.onSec) })}
      >
        <span className="fd-score-val">{formatOpTime(op.onSec)}</span>
        <span className="fd-score-label">{t('fieldDay.opTime.on')}</span>
      </div>
      <div
        className="fd-score"
        role="group"
        data-optime="off"
        title={t('fieldDay.opTime.off.title', { minutes })}
        aria-label={t('fieldDay.opTime.off.aria', { time: formatOpTime(op.offSec) })}
      >
        <span className="fd-score-val">{formatOpTime(op.offSec)}</span>
        <span className="fd-score-label">{t('fieldDay.opTime.off')}</span>
      </div>
    </>
  )
}

/**
 * ⭐ THE OPERATOR BOX — who is at the key. One component wherever it is drawn, so the places cannot
 * drift: the scoreboard's row (`FieldDayScoreboard`: the torn-off scoreboard, the Remote page's
 * read-only copy, and the Contest screen while no contest runs) and, while a contest runs, the large
 * log line's title line beside Sending and I moved (`ContestLargeEntry`, in the main window and the
 * logger window — the operator, 2026-10-12: "I want to move the field to change operators up near
 * the top").
 *
 * A local draft so typing is smooth, uppercase as it is typed, committed on blur / Enter: a
 * per-keystroke save would fire the heavyweight apply repeatedly. `onSave` is the host's NARROW write
 * (`setFdOperator`), never the whole-settings save, which a seat swap must not run mid-contact (#54).
 * Enter only blurs the box, so on a log line it never reaches the line's own Enter and logs nothing.
 * A save that fails puts the stored operator back in the box.
 */
export function OperatorBox({
  value,
  onSave,
  readOnly = false,
  operatorAsk,
  onOperatorFocused,
  onLogLine = false,
}: {
  /** The operator as stored (`fdOperator`), or null until the host has read its settings: until
   *  then the box waits, disabled. */
  value: string | null
  onSave: (call: string) => void | Promise<unknown>
  readOnly?: boolean
  /** Ctrl+O from a contest strip: the asks the box has not answered (its host's). */
  operatorAsk?: number
  onOperatorFocused?: () => void
  /** Drawn on the log line's title line (styles.css `.le-fd-operator`), not in the scoreboard's row. */
  onLogLine?: boolean
}) {
  const [draft, setDraft] = useState(value ?? '')
  useEffect(() => {
    setDraft(value ?? '')
  }, [value])
  const commit = () => {
    if (readOnly) return
    const v = draft.trim()
    if (v === (value ?? '')) return
    void Promise.resolve(onSave(v)).catch(() => setDraft(value ?? ''))
  }
  // ⭐ CTRL+O FROM A CONTEST STRIP (`features/contestKeys.ts`): at each ask the box takes the caret
  // as soon as it can be typed in — its settings read — and says so, so an ask is answered once.
  // A read-only box never takes it.
  const box = useRef<HTMLInputElement>(null)
  const typable = value !== null && !readOnly
  useEffect(() => {
    const el = box.current
    if (!operatorAsk || !typable || !el) return
    el.focus({ preventScroll: true })
    el.scrollIntoView?.({ block: 'nearest' })
    el.select()
    onOperatorFocused?.()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [operatorAsk, typable])
  return (
    <label className={onLogLine ? 'le-fd-operator' : undefined} style={onLogLine ? undefined : OP_FIELD}>
      <span className={onLogLine ? 'le-fd-operator-cap' : undefined} style={onLogLine ? undefined : OP_LABEL}>
        {t('fieldDay.operator.label')}
      </span>
      <input
        ref={box}
        className={onLogLine ? 'settings-input mono le-fd-operator-input' : undefined}
        style={onLogLine ? undefined : OP_INPUT}
        value={draft}
        disabled={value === null}
        readOnly={readOnly}
        onChange={(e) => { if (!readOnly) setDraft(upperTyped(e.target)) }}
        onBlur={commit}
        onKeyDown={(e) => {
          if (e.key === 'Enter') e.currentTarget.blur()
        }}
        placeholder={t('fieldDay.operator.placeholder')}
        aria-label={t('fieldDay.operator.aria')}
        spellCheck={false}
        autoCapitalize="characters"
      />
    </label>
  )
}

/**
 * The reusable Field Day scoreboard: the settable operator, the score tiles, and the
 * worked-sections board. `onSaveOperator` persists the operator (optimistic, parent-
 * owned) so a torn-off window and the docked view can't clobber each other's writes.
 * `detached` hides the pop-out button in the already-torn-off window. `operatorBox` false: the
 * box is on the log line above this scoreboard (ContestView's `operatorBoxAbove`), so its row
 * holds the buttons alone and the screen has one box.
 */
/**
 * The score's arithmetic in one line, as the contest scores: points × multipliers, Winter Field
 * Day's objectives, or Field Day's power and bonuses. The scoreboard and the Stats tab draw this one.
 */
export function ScoreMath({ fieldDay, settings }: { fieldDay: FieldDayStatus | null; settings: FdDisplaySettings | null }) {
  const isWfd = (fieldDay?.event ?? '') === 'wfd'
  const { fdPowerMult, qsoPts, poweredPoints, bonusPoints, totalScore, multCount } = computeFdScore(fieldDay, settings)
  return multCount !== null ? (
      /* ⭐ A CONTEST WITH MULTIPLIERS scores points × mults. Showing Field Day's
         power×+bonus line here would print `× power ×1` for a contest that has no
         power multiplier and omit the factor that actually decides the total. */
      <div className="fd-score-math">
        <span className="fd-score-math-line">
          <T
            k="fieldDay.score.mathMults"
            tags={{ b: <strong />, total: <strong className="fd-score-total" /> }}
            vals={{ qsoPts, multCount, totalScore }}
          />
        </span>
      </div>
    ) : isWfd && typeof fieldDay?.objectiveMultiplier === 'number' ? (
      /* ⭐ WINTER FIELD DAY'S CLAIMED TOTAL — the sponsor's QSO points × (OM + 1), all
         three numbers the engine's, so the line and the total cannot disagree. */
      <div className="fd-score-math">
        <span className="fd-score-math-line">
          <T
            k="fieldDay.score.wfdObjectives"
            tags={{ b: <strong />, total: <strong className="fd-score-total" /> }}
            vals={{ qsoPts, om: fieldDay.objectiveMultiplier, totalScore }}
          />
        </span>
      </div>
    ) : isWfd ? (
      /* A station that does not score Winter Field Day's objectives (a Remote view of an
         older one): its total is raw QSO points, so the honest line says so rather than
         claiming ARRL power×+bonus arithmetic WFD never produces. */
      <div className="fd-score-math">{t('fieldDay.score.wfd', { points: qsoPts })}</div>
    ) : (
      <div className="fd-score-math">
        <span className="fd-score-math-line">
          {/* The whole sum as ONE message — the eight fragments it used to be could not be
              reordered by a language that words the arithmetic differently. */}
          <T
            k="fieldDay.score.math"
            tags={{ b: <strong />, total: <strong className="fd-score-total" /> }}
            vals={{
              qsoPts,
              powerMult: fdPowerMult,
              poweredPoints,
              bonusPoints,
              totalScore,
            }}
          />
        </span>
      </div>
    )
}

export function FieldDayScoreboard({
  fieldDay,
  settings,
  onSaveOperator,
  detached = false,
  readOnly = false,
  operatorBox = true,
  operatorAsk,
  onOperatorFocused,
}: {
  fieldDay: FieldDayStatus | null
  settings: FdDisplaySettings | null
  onSaveOperator: (call: string) => void
  detached?: boolean
  readOnly?: boolean
  operatorBox?: boolean
  /** Ctrl+O from a contest strip: the asks the operator box has not answered (ContestView's). */
  operatorAsk?: number
  onOperatorFocused?: () => void
}) {
  const log = fieldDay?.log ?? []
  const nowUnix = useNowUnix()
  const modes = useMemo(() => modeCounts(log), [log])
  const workedSet = useMemo(() => workedSectionSet(fieldDay), [fieldDay])
  const { multCount } = computeFdScore(fieldDay, settings)

  return (
    <>
      {/* OPERATOR + POP-OUT — the buttons stay at the row's end when the box is on the log line. */}
      <div style={operatorBox ? SCOREBOARD_HEADER : { ...SCOREBOARD_HEADER, justifyContent: 'flex-end' }}>
        {operatorBox && (
          <OperatorBox
            value={settings ? (settings.fdOperator ?? '') : null}
            onSave={onSaveOperator}
            readOnly={readOnly}
            operatorAsk={operatorAsk}
            onOperatorFocused={onOperatorFocused}
          />
        )}
        {!detached && !readOnly && (
          <button
            type="button"
            style={POPOUT_BTN}
            onClick={() => void openPanelWindow('fieldday')}
            title={t('fieldDay.popOut.title')}
          >
            {t('fieldDay.popOut.label')}
          </button>
        )}
        {/* ⭐ THE CONTEST LOGGER WINDOW, from the torn-off scoreboard — the window with the
            operator box, the sections and who is on what band, which is where a club opens it
            from. The docked screen's header has its own. */}
        {detached && !readOnly && (
          <button
            type="button"
            style={POPOUT_BTN}
            onClick={() => void openContestLogger('full')}
            title={t('fieldDay.logger.title')}
          >
            {t('fieldDay.logger.label')}
          </button>
        )}
      </div>

      {/* ⭐ WHAT THIS SCORE LEAVES OUT, when the ruleset says it leaves something out.
          It sits ABOVE the tiles, because a caveat under a total is read after the
          number has already been believed. TNQP and TXQP both award bonus points the
          sponsor works out FROM THE LOG, and this build does not compute them; the
          key comes off the ruleset so the UI never has to hold a second copy of which
          contests are affected. Empty for both Field Day events and for OhQP and CQP,
          whose scores are complete. */}
      {fieldDay?.scoreNoteKey && SCORE_NOTE_KEYS.has(fieldDay.scoreNoteKey) && (
        <p className="fd-score-note" role="note">
          {t(fieldDay.scoreNoteKey as MessageKey)}
        </p>
      )}

      {/* SCORE TILES */}
      <div className="fd-scoreboard">
        <div className="fd-score">
          <span className="fd-score-val">{fieldDay?.qsoCount ?? 0}</span>
          <span className="fd-score-label">{t('fieldDay.score.qsos')}</span>
        </div>
        {/* ⭐ MULTIPLIERS when the ruleset has them, SECTIONS when it does not. The
            sections tile is Field Day's worked-sections count, which is a display count
            and not a multiplier (neither FD event has one); a QSO party's multiplier
            total IS its score's other factor, so the tile shows the number that is doing
            the work rather than one that means nothing for the contest on screen. */}
        {multCount !== null ? (
          <div className="fd-score">
            <span className="fd-score-val">{multCount}</span>
            <span className="fd-score-label">{t('fieldDay.score.mults')}</span>
          </div>
        ) : (
          <div className="fd-score">
            <span className="fd-score-val">{fieldDay?.sections ?? 0}</span>
            <span className="fd-score-label">{t('fieldDay.score.sections')}</span>
          </div>
        )}
        {/* ⭐ THE RATE METER — beside the counts it is read against, so "how many" and
            "how fast" are one glance. Every contest gets it; the data is the log's own
            timestamps and nothing about the score touches it. */}
        <RateTiles log={log} nowUnix={nowUnix} />
        {/* …and how long the station has been on, by the sponsors' off-time rule. */}
        <OperatingTimeTiles log={log} minOffSec={offTimeRule(fieldDay?.event).minOffSec} />
        {/* Per-mode chips — a count and a mode code, both invariant. */}
        <div className="fd-mode-chips">
          {modes.dig > 0 && <span className="fd-mode-chip dig">{modes.dig} {FD_MODE_CODES.dig}</span>}
          {modes.cw > 0 && <span className="fd-mode-chip cw">{modes.cw} {FD_MODE_CODES.cw}</span>}
          {modes.ph > 0 && <span className="fd-mode-chip ph">{modes.ph} {FD_MODE_CODES.ph}</span>}
        </div>
        {/* Score math */}
        <ScoreMath fieldDay={fieldDay} settings={settings} />
        <div className="fd-state-chip" title={t('fieldDay.state.title')}>
          {fieldDay?.state ?? t('fieldDay.state.idle')}
        </div>
      </div>

      {/* SECTIONS BOARD */}
      {/* THE MULTIPLIER DISPLAY — one block per board the session declares. A snapshot
          from a build older than `boards` still has its sections board: the fallback is
          the shipped component, not an empty screen. */}
      {fieldDay?.boards?.length ? (
        <MultiplierBoards boards={fieldDay.boards} />
      ) : (
        <SectionsBoard workedSet={workedSet} />
      )}

      {/* PER-OPERATOR STATISTICS — with two or more operators (ContestOperators). */}
      <OperatorStatsBoard fieldDay={fieldDay} />
    </>
  )
}

export function ContestView({
  fieldDay,
  clubLink,
  onSetMode,
  fdActive = false,
  fdRuleset = null,
  tier,
  section,
  keyboardMode: keyboardModeProp,
  observation,
  onOpenSettings,
  onSettingsSaved,
  logger = false,
  contestSwitch = null,
  operatorAsk,
  onOperatorFocused,
  operatorBoxAbove = false,
  entryAbove = false,
  onPopOut,
  onWorkIn,
}: Props) {
  const observed = observation !== undefined
  // The mode the banned-mode chip names: the station's own, given its section.
  const keyboardMode = useKeyboardMode(observed ? undefined : section, keyboardModeProp)
  const activeMode = section === undefined ? tier : stationMode(section, tier, keyboardMode)
  // The clock the spot advisory judges the event window by.
  const nowUnix = useNowUnix()
  // Log tail: bottom-pinned via the shared discipline. The old unconditional
  // snap on every logged QSO undid a mid-run scroll-back (checking a call two
  // contacts up) the moment the next contact landed. Pinned follows the run;
  // scrolled-up checking is never yanked.
  const logPin = usePinnedScroll<HTMLDivElement>()
  // The log's box, for the divider above it that sizes it.
  const logBox = useRef<HTMLDivElement>(null)
  const running = fieldDay?.running ?? false
  const log = fieldDay?.log ?? []
  const [exportError, setExportError] = useState<string | null>(null)
  const [busy, setBusy] = useState<ExportFormat | null>(null)
  // The 16 bonuses are worth ~1550 points — more than most clubs' QSO points — and they
  // sat behind a collapsed disclosure an operator had to know was there. Once the event is
  // running the panel opens itself, ONCE: `openedForRun` means a club that deliberately
  // closes it is not fought on the next snapshot.
  const [bonusOpen, setBonusOpen] = useState(false)
  // The work-in hint, until it is dismissed on this computer (features/contestWorkIn).
  const [workInHint, setWorkInHint] = useState(() => !workInHintSeen())
  // The route to the cockpits that send: only with Contest mode on, and never on a page that watches;
  // one button per cockpit this station shows (Settings ▸ Features, read as the rail reads it), and
  // per cockpit whose mode the contest allows (RTTY alone for the RTTY Roundup, `workInViewsFor`).
  const features = useFeatures()
  const workInViews = workInViewsFor(fdRuleset?.bannedModes, (v) => features.enabled[v] !== false)
  const workIn = fdActive && !observed && workInViews.length > 0
  const openedForRun = useRef(false)

  useEffect(() => {
    if (running && !openedForRun.current) {
      openedForRun.current = true
      setBonusOpen(true)
    }
  }, [running])

  // The settings as they were when the view opened: what the scoring panel and the operator field
  // DISPLAY. Never the base of a save — see `saveScoringPatch`.
  const [nativeSettings, setSettingsState] = useState<Settings | null>(null)
  const settings = observation ?? nativeSettings
  useEffect(() => {
    if (observed) return
    let live = true
    getSettings().then((s) => live && setSettingsState(s)).catch(() => {})
    return () => { live = false }
  }, [observed])

  const rows = useMemo(() => annotate(log), [log])
  const numbered = logIsNumbered(log)

  // ⭐ REMOVE THE NEWEST CONTACT, AND THE REMOVED LIST WITH RESTORE — the screen's half of the
  // strip's Ctrl+D. Remove sits on the newest row alone and asks before it acts (a second click
  // within REMOVE_CONFIRM_MS), as the strip does. Nothing is deleted: a removed contact is kept,
  // left out of the score, the dupe check and every export, and Restore puts it back exactly as
  // it was. Never offered when observing (`observed`): removal is refused from afar.
  const removable = !observed && fieldDay != null
  const newest = newestContact(log)
  const [removeAsk, setRemoveAsk] = useState<{ call: string; whenUnix: number; at: number } | null>(null)
  const [removedList, setRemovedList] = useState<RemovedContest[]>([])
  const [removedNote, setRemovedNote] = useState<{ text: string; alert: boolean } | null>(null)
  // The list follows the log: a removal or a restore anywhere (the strip, another window)
  // changes its length.
  const logLen = log.length
  useEffect(() => {
    if (!removable) {
      setRemovedList([])
      return
    }
    let live = true
    contestRemoved().then((list) => live && setRemovedList(list)).catch(() => {})
    return () => { live = false }
  }, [removable, logLen])
  useEffect(() => {
    if (!removeAsk) return
    const id = window.setTimeout(() => setRemoveAsk(null), Math.max(0, removeAsk.at + REMOVE_CONFIRM_MS - Date.now()))
    return () => window.clearTimeout(id)
  }, [removeAsk])
  // ⭐ CLEAR TEST CONTACTS — every contact of this running out of the contest log at once, each the
  // way Remove takes the newest: kept under Removed, Restore brings each back, the club seqs stay
  // spent; in a contest that sends a serial the run starts again at 001, and only there do the button's
  // words say so (`sendsSerial`). For the contacts logged as a test before the start. It asks first, on the button itself,
  // naming how many, and a second click within REMOVE_CONFIRM_MS clears exactly the log it named: a
  // contact logged in between asks again, and the engine refuses a log that changed. At a club only
  // this laptop's contacts go (its log holds no other), and the club drops them as it drops a removed
  // one. The button sits with the exports, far from the log and the Removed list (at 1024×768 they
  // are out of view below it), so the question is the button's own label and the answer is a toast
  // as well as the Removed list's note.
  const [clearAsk, setClearAsk] = useState<{ rows: number; contacts: number; call: string; whenUnix: number; at: number } | null>(null)
  useEffect(() => {
    if (!clearAsk) return
    const id = window.setTimeout(() => setClearAsk(null), Math.max(0, clearAsk.at + REMOVE_CONFIRM_MS - Date.now()))
    return () => window.clearTimeout(id)
  }, [clearAsk])
  const removeNewest = () => {
    if (!newest) return
    // Only at the station: a press that comes through the Remote stream removes nothing, as
    // removal is refused from afar. Read before anything awaits, inside the stream's dispatch.
    if (isStreamInput()) {
      setRemoveAsk(null)
      setRemovedNote({ text: t('logEntry.remove.remote'), alert: true })
      return
    }
    setClearAsk(null)
    const now = Date.now()
    const asked = removeAsk != null && removeAsk.call === newest.call && removeAsk.whenUnix === newest.whenUnix
    if (!asked || now - removeAsk.at > REMOVE_CONFIRM_MS) {
      setRemovedNote(null)
      setRemoveAsk({ call: newest.call, whenUnix: newest.whenUnix, at: now })
      return
    }
    setRemoveAsk(null)
    const label = contactLabel(newest.rows)
    contestRemoveLast(newest.call, newest.whenUnix)
      .then(
        (answer) => {
          setRemovedNote({ text: removalText(answer, label), alert: answer.outcome === 'refused' })
          return contestRemoved().then(setRemovedList)
        },
        () => setRemovedNote({ text: t('logEntry.remove.failed'), alert: true }),
      )
      .catch(() => {})
  }
  const clearAll = () => {
    if (!newest) return
    const say = (text: string, alert: boolean) => {
      setRemovedNote({ text, alert })
      pushToast(text, alert ? 'error' : 'info', CLEAR_TOAST_MS)
    }
    // Only at the station, as removal is: a press through the Remote stream clears nothing, nor one
    // from a computer driving the station (the station refuses it there). Read before anything
    // awaits, inside the stream's dispatch.
    if (isStreamInput() || drivesAnotherStation()) {
      setClearAsk(null)
      say(t('contest.clear.remote'), true)
      return
    }
    setRemoveAsk(null)
    const now = Date.now()
    const asked =
      clearAsk != null &&
      clearAsk.rows === log.length &&
      clearAsk.call === newest.call &&
      clearAsk.whenUnix === newest.whenUnix
    if (!asked || now - clearAsk.at > REMOVE_CONFIRM_MS) {
      setRemovedNote(null)
      setClearAsk({ rows: log.length, contacts: contactCount(log), call: newest.call, whenUnix: newest.whenUnix, at: now })
      return
    }
    setClearAsk(null)
    contestClear(clearAsk.rows, clearAsk.call, clearAsk.whenUnix)
      .then(
        (answer) => {
          say(clearText(answer, fieldDay?.sendsSerial === true), answer.outcome === 'refused')
          return contestRemoved().then(setRemovedList)
        },
        () => say(t('contest.clear.failed'), true),
      )
      .catch(() => {})
  }
  const restore = (entry: RemovedContest) => {
    // Only at the station, as removal is: a press that comes through the Remote stream restores
    // nothing. Read before anything awaits, inside the stream's dispatch.
    if (isStreamInput()) {
      setRemovedNote({ text: t('fieldDay.removed.restore.remote'), alert: true })
      return
    }
    const call = entry.rows[0]?.call ?? ''
    contestRestore(entry.id)
      .then(
        (answer) => {
          setRemovedNote({ text: restoreText(answer, call), alert: answer.outcome === 'refused' })
          return contestRemoved().then(setRemovedList)
        },
        () => setRemovedNote({ text: t('fieldDay.removed.failed'), alert: true }),
      )
      .catch(() => {})
  }

  // ⭐ EDIT A CONTACT IN PLACE — any row's Edit opens it where it stands (a county line as one
  // contact), and the strip's Ctrl+Y opens the newest the same way. It keeps its time, serial and
  // number. Never offered when observing, as removal is not; refused through the Remote stream;
  // and at a club the screen says how a contact is fixed there today instead: club sync cannot
  // carry an edit to the other laptops.
  const [editing, setEditing] = useState<{ call: string; whenUnix: number } | null>(null)
  const editRows = editing
    ? log.filter((q) => q.call === editing.call && q.whenUnix === editing.whenUnix)
    : []
  // A contact this window no longer finds while its edit is open — its call changed elsewhere, the
  // commonest edit, or it was removed: the edit closes and says which (`closedText`). This edit's
  // own save changing the call answers for itself. One edited elsewhere with its call kept closes
  // itself and says so (`ContestContactEdit`).
  const saving = useRef(false)
  useEffect(() => {
    if (!editing || editRows.length > 0) return
    setEditing(null)
    if (!saving.current) setRemovedNote({ text: closedText(log, editing.whenUnix), alert: true })
  }, [editing, editRows.length, log])
  const editRow = (q: FieldDayQso) => {
    if (isStreamInput()) {
      setRemovedNote({ text: t('contest.edit.remote'), alert: true })
      return
    }
    setRemoveAsk(null)
    if (fieldDay?.club) {
      setRemovedNote({ text: t('contest.edit.club'), alert: true })
      return
    }
    // Another edit open in this window (the strip's Ctrl+Y, a last contact's): it keeps the
    // keyboard and its typing, and no second one opens.
    if (!editing && focusOpenContestEdit()) {
      setRemovedNote({ text: t('contest.edit.open'), alert: false })
      return
    }
    setRemovedNote(null)
    saving.current = false
    setEditing({ call: q.call, whenUnix: q.whenUnix ?? 0 })
  }
  const editDone = (note: EditNote | null) => {
    saving.current = false
    setEditing(null)
    if (note) setRemovedNote(note)
  }

  // The §18.1 upload control. The DESTINATIONS are mirrored locally so a click reads
  // back instantly — the snapshot that carries the engine's answer arrives on the next
  // poll — and re-synced from the snapshot whenever it moves, so the mirror can never
  // outlive the session it describes.
  const uploadFromSnap = (fieldDay?.upload?.destinations ?? []).join(',')
  const [uploadDests, setUploadDests] = useState<string[]>(
    fieldDay?.upload?.destinations ?? [],
  )
  useEffect(() => {
    setUploadDests(uploadFromSnap === '' ? [] : uploadFromSnap.split(','))
  }, [uploadFromSnap])
  const saveUpload = async (enabled: boolean, destinations: string[]) => {
    setUploadDests(destinations)
    try {
      await fdSetUpload(enabled, destinations)
    } catch (e) {
      // Put the mirror back where the engine still has it, then say so.
      setUploadDests(uploadFromSnap === '' ? [] : uploadFromSnap.split(','))
      pushToast(String(e), 'error')
    }
  }

  // The end-of-contest merge (§11 item 5). The BEFORE half is the button's own label —
  // how many contacts, to which logbook — read off the live log so it cannot go stale;
  // the AFTER half is the report, which is the only thing that tells a second press
  // apart from a broken button.
  const [merging, setMerging] = useState(false)
  const [mergeReport, setMergeReport] = useState<FdMergeReport | null>(null)
  const [mergeError, setMergeError] = useState<string | null>(null)
  const mergeCount = log.length
  const runMerge = async () => {
    // Only at the station: a press that comes through the Remote stream writes nothing into the
    // logbook. Read before anything awaits, inside the stream's dispatch.
    if (isStreamInput()) {
      setMergeReport(null)
      setMergeError(t('fieldDay.merge.remote'))
      return
    }
    setMerging(true)
    setMergeError(null)
    try {
      setMergeReport(await fdMergeToGeneral())
    } catch (e) {
      // The engine's own sentence — it refuses when the mode is not live, and that
      // refusal is its wording, not a second copy here that could drift from it.
      setMergeReport(null)
      setMergeError(String(e))
    } finally {
      setMerging(false)
    }
  }

  // ⭐ The contest contacts no session loads any more that the logbook does not hold: the journal
  // keeps each until it is merged. Asked of the station when the view opens and when the contest
  // changes (asking also lets go of the ones already merged), never by a Remote observer, and
  // merged only at the station, as the end-of-contest merge is.
  const [unmerged, setUnmerged] = useState<ContestUnmerged[]>([])
  const [unmergedBusy, setUnmergedBusy] = useState<string | null>(null)
  const [unmergedReport, setUnmergedReport] = useState<FdMergeReport | null>(null)
  const [unmergedError, setUnmergedError] = useState<string | null>(null)
  const contestOn = fieldDay != null
  const shownContest = fieldDay?.event
  useEffect(() => {
    if (observed || !contestOn) {
      setUnmerged([])
      return
    }
    let live = true
    contestUnmerged().then((u) => live && setUnmerged(u)).catch(() => {})
    return () => { live = false }
  }, [observed, contestOn, shownContest])
  const mergeUnmerged = async (event: string) => {
    if (isStreamInput()) {
      setUnmergedReport(null)
      setUnmergedError(t('fieldDay.merge.remote'))
      return
    }
    setUnmergedBusy(event)
    setUnmergedError(null)
    try {
      setUnmergedReport(await contestMergeUnmerged(event))
    } catch (e) {
      // The engine's own sentence: outside the contest, or a contest whose session Settings
      // cannot build, in the words entering that contest would use.
      setUnmergedReport(null)
      setUnmergedError(String(e))
      return
    } finally {
      setUnmergedBusy(null)
    }
    // Asked again, so that what the merge wrote is no longer offered.
    contestUnmerged().then(setUnmerged).catch(() => {})
  }

  // One optimistic writer for every scoring field on this panel (earned list, plan list,
  // power multiplier). Each edit names ONE field, so the three controls cannot write over
  // each other's state, and the power chips edit the very same `fdPowerMult` the Settings
  // panel does rather than a second copy of it.
  //
  // ⚠️ SAVED OVER THE LIVE SETTINGS, NEVER OVER THIS VIEW'S COPY. The copy is as old as the
  // view, and a save of `{ ...copy, ...edit }` wrote every field changed elsewhere since back
  // to what it was: simultaneous radios on again after "use one radio", the previous operator
  // after a seat swap in the pop-out scoreboard, the old dial after the rig was retuned. The
  // copy only mirrors the edit, so the display does not wait on a round trip.
  const saveScoringPatch = async (edit: (s: Settings) => Partial<Settings>) => {
    if (observed || logger || !nativeSettings) return
    setSettingsState({ ...nativeSettings, ...edit(nativeSettings) })
    try {
      await patchSettings(edit)
    } catch {
      // Revert optimistic update on failure
      setSettingsState(nativeSettings)
    }
  }
  const toggle = (list: string[], id: string) =>
    list.includes(id) ? list.filter((b) => b !== id) : [...list, id]
  /** Confirm / un-confirm a bonus. THIS is the list the score is made of. */
  const toggleEarned = (id: string) =>
    saveScoringPatch((s) => ({ fdBonuses: toggle(s.fdBonuses ?? [], id) }))
  /** Put a bonus on the chase list (or take it off). Never touches the score. */
  const togglePlanned = (id: string) =>
    saveScoringPatch((s) => ({ fdBonusesPlanned: toggle(s.fdBonusesPlanned ?? [], id) }))
  const setPowerMult = (mult: number) => saveScoringPatch(() => ({ fdPowerMult: mult }))
  /** Tick / untick a Winter Field Day objective: the list the score multiplies by. */
  const toggleObjective = (id: string) =>
    saveScoringPatch((s) => ({ fdObjectives: toggle(s.fdObjectives ?? [], id) }))
  /** Put an objective on the plan (or take it off). Never touches the score. */
  const toggleObjectivePlanned = (id: string) =>
    saveScoringPatch((s) => ({ fdObjectivesPlanned: toggle(s.fdObjectivesPlanned ?? [], id) }))

  // Persist the settable Field Day operator (optimistic). NOT the whole-struct save that
  // toggleBonus uses: a seat swap happens mid-QSO, and the heavyweight path drops the TX
  // queue and re-derives the TX cycle from the saved struct (#54). Since
  // #100 it no longer resets the operating mode, but those two still land on a live contact.
  // The engine trims + uppercases.
  const saveOperator = async (call: string) => {
    if (observed || !nativeSettings) return
    const op = call.trim().toUpperCase()
    setSettingsState({ ...nativeSettings, fdOperator: op })
    try {
      await setFdOperator(op)
    } catch {
      setSettingsState(nativeSettings)
    }
  }

  // ⭐ THE CONTEST MODE SWITCH. The same `fdActive` Settings writes, through the same patch
  // seam as the scoring panel: the settings the backend holds at save time, this one field
  // changed. What it shows is App's copy (`fdActive`), which App re-reads after the save; the
  // value just written bridges the moment between the two, so the switch does not flick back.
  const [modeWritten, setModeWritten] = useState<boolean | null>(null)
  useEffect(() => setModeWritten(null), [fdActive])
  const modeOn = modeWritten ?? fdActive
  const [modeBusy, setModeBusy] = useState(false)
  // A refused turn-on: what was missing at the moment the operator asked, read fresh then.
  const [refusal, setRefusal] = useState<StartBlocker | null>(null)
  // ⭐ THE CONTEST-OVER CHECKLIST (ContestOverDialog.tsx): what turning the mode off offers, while
  // it is open. The save that turns the mode off waits for its answer.
  const [overOffer, setOverOffer] = useState<ContestOverOffer | null>(null)
  // ⭐ THE STATS TAB (ContestStats.tsx): the scoreboard's place, or the contest's stats in it. The
  // log below stays either way.
  const [statsTab, setStatsTab] = useState(false)
  const [setupOpen, setSetupOpen] = useState<'pick' | 'sends' | null>(null)
  const toggleMode = async () => {
    if (observed || logger || modeBusy) return
    const next = !modeOn
    setModeBusy(true)
    try {
      if (next) {
        // Against what is saved NOW, not the copy this screen opened with: a class typed in
        // Settings a minute ago counts, and so does a county.
        const [s, preview] = await Promise.all([getSettings(), getFdRuleset().catch(() => null)])
        const blocked = startBlocker(s, preview)
        if (blocked) {
          setRefusal(blocked)
          return
        }
      } else {
        // OFF: offer the checklist BEFORE the mode goes, because the merge and the export need the
        // session it holds. Read against what is saved NOW, the engine's note of "before" included,
        // with the club chip's count of contacts the host does not have yet. With nothing to offer
        // or say, the switch goes off as it always did.
        const offer = contestOverOffer(await getSettings(), mergeCount, fieldDay?.event, fieldDay?.club?.queued ?? 0, {
          missing: fieldDay?.cabrilloMissing,
          choices: fdRuleset,
        })
        if (hasOffer(offer)) {
          setOverOffer(offer)
          return
        }
      }
      await patchSettings(() => ({ fdActive: next }))
      setRefusal(null)
      setModeWritten(next)
      onSettingsSaved?.()
    } catch (e) {
      pushToast(String(e), 'error')
    } finally {
      setModeBusy(false)
    }
  }

  // The checklist's "Turn off": ONE save, the mode off with the ticked settings put back (the merge,
  // the export and Unassisted are done by then). A refusal rejects back into the checklist, which
  // shows it and stays open with the mode still on.
  const turnOffWithChecklist = async (putBack: Partial<Settings>) => {
    await patchSettings(() => ({ ...putBack, fdActive: false }))
    setOverOffer(null)
    setRefusal(null)
    setModeWritten(false)
    onSettingsSaved?.()
  }

  // Event header: the window arrives Rust-computed on the DTO (fd_rules data —
  // real 27 h SFD / 30 h WFD durations; the old TS date math hardcoded 24 h and
  // called WFD over with six hours left). Snapshots refresh it, so the year
  // rollover and the active→next transition need no client-side clock walk.
  // The contest picked in Settings, as the preview resolved it (an id the rules data does not
  // carry resolves to the ARRL Field Day mode entry would fall back to).
  const pickedEvent = fdRuleset?.event ?? (nativeSettings?.fdEvent?.trim() || undefined)
  // The running session's contest, and with no session (Contest mode off, or a station that
  // cannot enter yet) the picked one: no session used to read as ARRL Field Day whatever the
  // picker said.
  const shownEvent = fieldDay ? fieldDay.event : pickedEvent
  const eventKind: FdKind = (shownEvent === 'wfd' ? 'wfd' : 'arrlfd')
  const isWfd = eventKind === 'wfd'
  // ⭐ The station scores Winter Field Day by its objectives: the engine says so by sending
  // the multiplier. A Remote view of an older station has none, and keeps the bonus checklist
  // that station still scores.
  const wfdOm = isWfd && typeof fieldDay?.objectiveMultiplier === 'number' ? fieldDay.objectiveMultiplier : null
  // ⭐ WHICH CONTEST THIS IS. Every Field Day word on this screen — the banner, the header,
  // the class/section, the log table's columns, the summary — used to be printed whatever
  // the picker said, so a CQ WW log sat under an "ARRL Field Day" banner with Class and
  // Section columns empty on every row.
  const fdEventIsFieldDay = isFieldDay(shownEvent)
  // Why club sync is not running, when it is switched on for a contest it cannot run — read
  // from the station's own settings, which a Remote observation does not carry, and the
  // station's preview of that pick (a contest the loaded rules dropped resolves elsewhere).
  const clubRefusal = clubSyncRefusal(nativeSettings, fdRuleset)
  const eventName = fdEventIsFieldDay ? FD_EVENT_NAMES[eventKind] : contestName(shownEvent)
  // The window: the session's, else the preview's. Both are the rules data's running-or-next
  // window, so the banner reads the same before the contest starts as during it.
  const windowStart = fieldDay ? fieldDay.eventStartUnix : fdRuleset?.eventStartUnix
  const windowEnd = fieldDay ? fieldDay.eventEndUnix : fdRuleset?.eventEndUnix
  const fdEvent = useMemo(
    () => fdEventFromWindow(eventKind, windowStart, windowEnd, eventName),
    [eventKind, windowStart, windowEnd, eventName],
  )
  // The contest's received slots, one log-table column each (Field Day keeps its own two).
  const slotColumns = fdEventIsFieldDay ? [] : (fieldDay?.receives ?? [])
  const subtitle = useMemo(() => (fdEvent ? fdHeaderSubtitle(new Date(), fdEvent) : ''), [fdEvent])

  // Score components (shared with the scoreboard tiles) — needed here for the bonuses count.
  const { fdPowerMult, claimedBonusIds: claimedBonuses, bonusPoints } = computeFdScore(fieldDay, settings)

  // The chase: earned vs planned. `computeFdScore` above is untouched by any of this —
  // it reads `fdBonuses` and nothing else, which is what makes a plan unscoreable.
  const plannedBonuses = settings?.fdBonusesPlanned ?? []
  const tally = useMemo(
    () => fdBonusTally(claimedBonuses, plannedBonuses),
    [claimedBonuses, plannedBonuses],
  )

  // Cabrillo/ADIF come from the backend; Summary/Dupe sheet are derived client-side
  // from the same log the board renders (no backend command). Defined here so it can
  // read the score components computed just above.
  const handleExport = async (format: ExportFormat) => {
    if (observed) return
    // Only at the station: a press that comes through the Remote stream writes no file to the
    // station's disk, the station log's own or the club's. Read before anything awaits, inside
    // the stream's dispatch.
    if (isStreamInput()) {
      setExportError(format.startsWith('club-') ? t('fieldDay.club.export.remote') : t('fieldDay.export.remote'))
      return
    }
    setExportError(null)
    setBusy(format)
    try {
      let text: string
      if (format === 'summary') {
        text = contestSummaryText(fieldDay, settings, shownEvent)
      } else if (format === 'dupesheet') {
        text = buildDupeSheetText(rows, fdEventIsFieldDay ? undefined : eventName)
      } else if (format === 'club-cabrillo' || format === 'club-adif') {
        // The MERGED club log from the host, deduped earliest-wins — the
        // submittable club artifact (host role only; the backend refuses
        // elsewhere and the buttons only render while hosting).
        text = await fdClubExport(format === 'club-cabrillo' ? 'cabrillo' : 'adif')
      } else {
        text = await exportLog(format)
      }
      const stamp = new Date().toISOString().slice(0, 10)
      // Named for the contest whose log it is, the one the banner and the tooltips name.
      const stem = contestFileStem(shownEvent)
      const base =
        format === 'cabrillo' || format === 'adif'
          ? `${stem}-log`
          : format.startsWith('club-')
            ? `${stem}-club-log`
            : `${stem}-${format}`
      // Real Rust write to Downloads, same as the Logbook exports — a `<a download>` blob is
      // silently CANCELLED by wry on macOS (no download handler is wired), so all four FD
      // export buttons produced no file there while looking successful. The toast fires only
      // after the write actually happened, and names the path it landed at.
      const path = await saveTextToDownloads(`${base}-${stamp}.${EXT[format]}`, text)
      pushToast(t('fieldDay.export.done', { path }), 'success')
    } catch (err) {
      setExportError(typeof err === 'string' ? err : err instanceof Error ? err.message : String(err))
    } finally {
      setBusy(null)
    }
  }

  // ⭐ THE LINES UNDER THE SWITCH. One reason the contest cannot start, when there is one: a
  // refused turn-on shows the reason it read at that moment, otherwise App's preview (re-pulled
  // after every save) and the settings this screen opened with. With a session running there
  // is nothing to start.
  const blocker = observed
    ? null
    : (refusal ?? (fieldDay ? null : startBlocker(nativeSettings, fdRuleset)))
  const modeLines: { key: string; text: string }[] = []
  if (fieldDay) {
    // ⭐ A SAVED PICK SWITCHED THE CONTEST: said first, naming the one it ended. Its contacts
    // are the journal's, kept out of this session (the kept-out note counts them).
    if (contestSwitch) {
      modeLines.push({
        key: 'switched',
        text: t('fieldDay.mode.switched', { contest: eventName, previous: contestName(contestSwitch.from) }),
      })
    }
    // Which rules apply now, plainly. The ruleset facts are the PICKED contest's, so they are
    // read only while that is the contest running; the line below says when it is not.
    modeLines.push({ key: 'on', text: t('fieldDay.mode.on', { contest: eventName }) })
    const running = fdRuleset && fdRuleset.event === (fieldDay.event || 'arrlfd') ? fdRuleset : null
    if (running && running.bannedModes.length > 0) {
      modeLines.push({
        key: 'banned',
        text: t('fieldDay.mode.rule.banned', { modes: running.bannedModes.join(', ') }),
      })
    }
    if (running && !running.clusterAllowed) {
      modeLines.push({ key: 'cluster', text: t('fieldDay.mode.rule.cluster') })
    }
    if (running && !running.spottingAllowed) {
      modeLines.push({ key: 'spotting', text: t('fieldDay.mode.rule.spotting') })
    }
    // ⭐ A contest that allows spots only over amateur RF (Winter Field Day): inside its window
    // the station posts none over the internet, judged by the same window the engine's spot
    // block judges. Plainly, beside the switch; the banner's advisory names what is held back.
    if (
      running?.spotsRfOnly &&
      fieldDay.eventStartUnix !== undefined &&
      fieldDay.eventEndUnix !== undefined &&
      fieldDay.eventStartUnix <= nowUnix &&
      nowUnix < fieldDay.eventEndUnix
    ) {
      modeLines.push({ key: 'spotsRfOnly', text: t('fieldDay.mode.rule.spotsRfOnly') })
    }
  } else if (!observed && !blocker && !modeOn) {
    modeLines.push({ key: 'off', text: t('fieldDay.mode.off', { contest: eventName }) })
  }
  // ⭐ WHAT THE SWITCH DOES TO A CLUB, said before it is pressed. Club sync follows its own
  // settings, not this switch (`Engine::fd_sync_targets`): with the mode off the station stays
  // in the club event, and its outbox is empty until a session is back, so the contacts it
  // owes wait in the journal and go when the mode returns.
  const clubOn =
    !observed &&
    !clubRefusal &&
    !!nativeSettings &&
    (nativeSettings.fdHostEnable === true || (nativeSettings.fdJoinAddr ?? '').trim() !== '')
  if (clubOn) modeLines.push({ key: 'club', text: t('fieldDay.mode.club') })
  // In the logger window: where the controls that are read-only here are.
  if (logger) modeLines.push({ key: 'logger', text: t('fieldDay.logger.note') })
  // ⭐ WHAT THE SPONSOR REQUIRES AND THE ENTRY HAS LEFT OUT, named before the export: the running
  // session's list (the header its export writes now), else the preview's for a session started
  // from the saved settings, so a declaration the session reads as it starts is made in time. Rust
  // names the lines; each is worded here as the Settings control that fills it. Not on a Remote
  // view, which exports nothing.
  const cabrilloMissing = observed
    ? []
    : ((fieldDay ? fieldDay.cabrilloMissing : fdRuleset?.cabrilloMissing) ?? [])
  // Running / S&P with nothing to switch between and the master off: see the buttons.
  const roleLocked = !fieldDay && !modeOn

  return (
    <section className="conversation panel fieldday">
      <ContestOverDialog
        offer={overOffer}
        onNotNow={() => setOverOffer(null)}
        onTurnOff={turnOffWithChecklist}
        onStats={() => {
          setOverOffer(null)
          setStatsTab(true)
        }}
        summaryText={fieldDay ? () => contestSummaryText(fieldDay, settings, fieldDay.event) : undefined}
      />
      {/* ⭐ THE CONTEST SETUP (ContestSetupWizard.tsx): "Set up a contest…" with none running, "Change
          setup…" on one that runs. It saves through the patch seam and tells App, as the switch does. */}
      {setupOpen && (
        <ContestSetupWizard
          startAt={setupOpen}
          onClose={() => setSetupOpen(null)}
          onSaved={() => onSettingsSaved?.()}
          onWorkIn={onWorkIn}
        />
      )}
      {/* EVENT BANNER: the Contest mode switch, beside the contest and its window. */}
      <div className="fd-event-banner">
        {/* ⭐ THE MODE SWITCH, with Settings' own label and accessible names so every place
            that offers it reads the same words. Not a transmit control: on enters the contest
            passive (S&P), off leaves it, and either is the save Settings makes. Read-only for
            a Remote observer, who is watching a station, not operating it. */}
        <label className="fd-mode-switch">
          <button
            type="button"
            role="switch"
            aria-checked={modeOn}
            className={`toggle${modeOn ? ' on' : ''}`}
            disabled={observed || logger || modeBusy}
            onClick={() => void toggleMode()}
            aria-label={
              modeOn
                ? t('settings.fieldDay.mode.aria.disable', { contest: eventName })
                : t('settings.fieldDay.mode.aria.enable', { contest: eventName })
            }
          >
            <span className="toggle-knob" />
          </button>
          <span className="fd-mode-label">{t('settings.fieldDay.mode.label')}</span>
        </label>
        <span className="fd-event-name">{eventName}</span>
        <span className="fd-event-subtitle">{subtitle}</span>
        {/* Warn-only rule advisories (banned mode + assistance) — passive status
            lines; nothing is ever removed or disabled by rule. */}
        <FdAdvisories
          fdActive={fdActive}
          ruleset={fdRuleset}
          activeMode={activeMode}
          assistanceOn={fieldDay?.assistanceOn ?? []}
          showAssistance
          eventWindow={
            fieldDay?.eventStartUnix !== undefined && fieldDay?.eventEndUnix !== undefined
              ? { start: fieldDay.eventStartUnix, end: fieldDay.eventEndUnix }
              : undefined
          }
          nowUnix={nowUnix}
        />
        {fieldDay?.rulesYear ? (
          <span className="fd-event-rules">
            {t('fieldDay.rules.line', {
              year: fieldDay.rulesYear,
              date: (fieldDay.rulesGenerated ?? '').slice(0, 10),
            })}
          </span>
        ) : null}
        {!observed && !logger && (
          <button type="button" className="fd-setup-btn" onClick={() => setSetupOpen(fieldDay ? 'sends' : 'pick')}>
            {fieldDay ? t('contestSetup.change') : t('contestSetup.open')}
          </button>
        )}
        {/* ⭐ THE CLOCK, large (the Illinois QSO Party, 2026-10-18: the operator squinted at the top bar's):
            UTC first, the time a contest and its log run on, then this computer's local time beside it.
            The top bar's own clock (`UtcClock`), so the two can never read differently. On the banner, so
            the main window's screen, its pop-out and the logger window each show it. */}
        <div className="contest-clock" role="group" aria-label={t('fieldDay.clock.aria')}>
          <UtcClock />
          <UtcClock local />
        </div>
        {/* ⭐ THIS SCREEN IN A WINDOW OF ITS OWN, for another monitor (App's, in the main window). The
            scoreboard's own ⧉ Pop out, lower down, tears off the scoreboard alone, so this one is
            named for the screen to a screen reader. */}
        {onPopOut && (
          <button
            type="button"
            className="fd-screen-popout"
            onClick={onPopOut}
            title={t('contestScreen.popOut.title')}
            aria-label={t('contestScreen.popOut.aria')}
          >
            {t('fieldDay.popOut.label')}
          </button>
        )}
      </div>
      {(blocker || modeLines.length > 0 || cabrilloMissing.length > 0) && (
        <div className="fd-mode-status">
          {blocker && (
            <p className="fd-mode-line warn" role={refusal ? 'alert' : 'status'}>
              {blockerText(blocker, eventName)}
              {onOpenSettings && (
                <>
                  {' '}
                  <button
                    type="button"
                    className="settings-linkbtn"
                    onClick={() => onOpenSettings(blockerTarget(blocker))}
                  >
                    {t('fieldDay.mode.needs.open')}
                  </button>
                </>
              )}
            </p>
          )}
          {cabrilloMissing.length > 0 && (
            <p className="fd-mode-line warn" role="status">
              {t('fieldDay.cabrillo.missing', {
                contest: eventName,
                lines: cabrilloMissing.map(cabrilloLineWords).join(', '),
              })}
              {fieldDay && cabrilloMissing.some((l) => READ_AS_THE_SESSION_STARTS.includes(l))
                ? ` ${t('fieldDay.cabrillo.restart')}`
                : ''}
              {onOpenSettings && (
                <>
                  {' '}
                  <button
                    type="button"
                    className="settings-linkbtn"
                    onClick={() => onOpenSettings('contest-category')}
                  >
                    {t('fieldDay.mode.needs.open')}
                  </button>
                </>
              )}
            </p>
          )}
          {modeLines.map((l) => (
            <p key={l.key} className={`fd-mode-line ${l.key}`}>
              {l.text}
            </p>
          ))}
        </div>
      )}

      {/* ⭐ "<contest> this weekend — Set up…": a quiet line when a contest starts within a week and
          none runs. No pop-up. */}
      {!fieldDay && !observed && !logger && <ContestSoonLine onWorkIn={onWorkIn} />}
      <div className="panel-header fd-header">
        <div className="fd-ident">
          <h2 className="conv-peer">
            {fdEventIsFieldDay
              ? isWfd ? FD_SHORT_NAMES.wfd : FD_SHORT_NAMES.arrlfd
              : contestShortName(fieldDay?.event)}
          </h2>
          {/* What the SESSION is composing — the exchange the next contact will get.
              A logged row's own exchange is in the log table below, off `mex`. */}
          {fdEventIsFieldDay ? (
            <span className="fd-class">
              {composingSlot(fieldDay?.composing, 'CLASS') || '—'}
              <span className="fd-section">
                {' '}
                {composingSlot(fieldDay?.composing, 'SECTION') || '—'}
              </span>
            </span>
          ) : (
            <span className="fd-class">{fieldDay?.sentExchange || '—'}</span>
          )}
        </div>
        {/* ⚠️ Not with Contest mode off and no session: either button ENTERS the contest
            (`set_mode`), and an entry the master switch does not know about is one the
            snapshot hides, so a Running started from here would call CQ unseen. */}
        <div className="fd-role-toggle" role="group" aria-label={t('fieldDay.role.aria')}>
          <button
            type="button"
            className={`fd-role-btn${running ? ' active' : ''}`}
            aria-pressed={running}
            disabled={observed || logger || roleLocked}
            title={t('fieldDay.role.running.title')}
            onClick={() => { if (!observed && !logger && !roleLocked) onSetMode?.('fieldday-run') }}
          >
            {t('fieldDay.role.running')}
          </button>
          <button
            type="button"
            className={`fd-role-btn${!running ? ' active' : ''}`}
            aria-pressed={!running}
            disabled={observed || logger || roleLocked}
            onClick={() => { if (!observed && !logger && !roleLocked) onSetMode?.('fieldday-sp') }}
          >
            {t('fieldDay.role.sp')}
          </button>
        </div>
        {/* ⭐ WORK THIS CONTEST IN CW / IN PHONE (a tester at the Illinois QSO Party, 2026-10-18, looked
            for Enter Sends Message here): this screen never transmits, so it offers the way to the
            cockpit that does, with the contest's strip on it (Enter Sends Message, Running / S&P, the
            F-keys). In the main window that is its own switch; from another window a press asks the
            main window, and sends the station nothing (features/contestWorkIn). */}
        {workIn && (
          <div className="fd-work-in" role="group" aria-label={t('fieldDay.workIn.aria')}>
            {workInViews.map((view) => (
              <button
                key={view}
                type="button"
                className="export-btn"
                title={workInTitle(view)}
                onClick={() => (onWorkIn ?? askMainToWorkIn)(view)}
              >
                {workInLabel(view)}
              </button>
            ))}
          </div>
        )}
        {/* Spot me: one DX cluster spot of this station per press (ContestSpotMe.tsx). Not on a page
            that only watches. */}
        {fdActive && !observed && (
          <ContestSpotMe
            contestShort={fdEventIsFieldDay ? (isWfd ? FD_SHORT_NAMES.wfd : FD_SHORT_NAMES.arrlfd) : contestShortName(fieldDay?.event)}
          />
        )}
        {/* Export buttons */}
        {!observed && <div className="fd-export">
          {exportError && (
            <span className="log-export-error" role="alert">{exportError}</span>
          )}
          {/* ⭐ THE CONTEST LOGGER WINDOW: this screen and the log line in a window of their own,
              for a second person logging at a second monitor and keyboard. Not in that window
              itself, which is already it. */}
          {!logger && (
            <button
              type="button"
              className="export-btn"
              onClick={() => void openContestLogger('full')}
              title={t('fieldDay.logger.title')}
            >
              {t('fieldDay.logger.label')}
            </button>
          )}
          <button
            type="button"
            className="export-btn"
            disabled={busy !== null}
            onClick={() => handleExport('cabrillo')}
            title={t('fieldDay.export.cabrillo.title', { contest: eventName })}
          >
            {busy === 'cabrillo' ? t('fieldDay.export.busy') : t('fieldDay.export.cabrillo.label')}
          </button>
          <button
            type="button"
            className="export-btn"
            disabled={busy !== null}
            onClick={() => handleExport('adif')}
            title={t('fieldDay.export.adif.title', { contest: eventName })}
          >
            {busy === 'adif' ? t('fieldDay.export.busy') : t('fieldDay.export.adif.label')}
          </button>
          <button
            type="button"
            className="export-btn"
            disabled={busy !== null}
            onClick={() => handleExport('summary')}
            title={t('fieldDay.export.summary.title')}
          >
            {busy === 'summary' ? t('fieldDay.export.busy') : t('fieldDay.export.summary.label')}
          </button>
          <button
            type="button"
            className="export-btn"
            disabled={busy !== null}
            onClick={() => handleExport('dupesheet')}
            title={t('fieldDay.export.dupeSheet.title')}
          >
            {busy === 'dupesheet' ? t('fieldDay.export.busy') : t('fieldDay.export.dupeSheet.label')}
          </button>
          {/* Clear test contacts: beside the exports rather than on a row of its own, so the log
              keeps every pixel. Asked, it says what the second click does. */}
          {removable && (
            <button
              type="button"
              className="export-btn"
              aria-pressed={clearAsk != null}
              disabled={log.length === 0}
              onClick={clearAll}
              title={fieldDay?.sendsSerial ? t('contest.clear.title.serial') : t('contest.clear.title')}
            >
              {clearAsk == null
                ? t('contest.clear.label')
                : fieldDay?.sendsSerial
                  ? t('contest.clear.confirm.serial', { count: clearAsk.contacts })
                  : t('contest.clear.confirm', { count: clearAsk.contacts })}
            </button>
          )}
          </div>}

        {/* ⭐ WHERE THIS SESSION'S MERGED CONTACTS GO — §18.1, per session, default OFF.
            It answers "Nexus sends my contacts to my general logbook — how do I change
            it for the QSO party?" as a property of the RUN, so it ends with the run
            instead of being a global the operator has to remember to change back.

            ⚠️ THE HINT RENDERS BESIDE THE SWITCH, ALWAYS, AND IT IS THE ENGINE'S OWN
            WORDS. ClubLog's catch-up sweep re-queues every contact ClubLog never
            accepted the next time a ClubLog password is saved — regardless of this
            switch. An operator who reads "upload: off" and gets a ClubLog upload anyway
            has been misled by us, not surprised by ClubLog. The text travels on
            `upload.hint`, attached to the control it qualifies, so a renderer cannot
            show one without the other; it is deliberately NOT a catalog string, because
            a second copy here could drift from the limitation the engine actually has.
            Connector ids (`clublog`, `qrz`, `wrl`) are invariant tokens. */}
        {/* ⚠️ `!observed` because the control below WRITES INTO THE OPERATOR'S GENERAL
            LOGBOOK. Codex guarded the export block beside it for the same reason —
            a remote observer is watching a station, not operating it, and must not
            be able to fire a write into somebody else's log. */}
        {!observed && fieldDay?.upload && (
          <div className="fd-upload">
            {/* ⭐ THE END-OF-CONTEST MERGE — §11 item 5's "one click", which shipped with
                commands and no control, so it was unreachable.

                It WRITES INTO THE OPERATOR'S GENERAL LOGBOOK, so it says what it will do
                before it does it (how many contacts, to which logbook) and what it did
                after (added versus already there). The merge is idempotent — every row
                carries its own identity and one already in the logbook is skipped — and
                the report is what makes a second press VISIBLY a no-op ("added 0, 42
                already there") rather than a button that looks broken.

                It sits with the upload switch below and under the same hint, because the
                switch decides whether what this button merges is also queued for upload,
                and the hint names the one thing neither of them closes. */}
            <div className="fd-merge">
              <button
                type="button"
                className="export-btn fd-merge-btn"
                disabled={merging || mergeCount === 0}
                onClick={() => void runMerge()}
                title={t('fieldDay.merge.title')}
              >
                {merging
                  ? t('fieldDay.merge.busy')
                  : t('fieldDay.merge.label', { count: mergeCount })}
              </button>
              {mergeReport && (
                <span className="fd-merge-report" role="status">
                  {t('fieldDay.merge.added', { count: mergeReport.added })}
                  {' · '}
                  {t('fieldDay.merge.already', { count: mergeReport.already })}
                  {mergeReport.refused > 0 && (
                    <> {' · '}{t('fieldDay.merge.refused', { count: mergeReport.refused })}</>
                  )}
                  {mergeReport.queued && <> {' · '}{t('fieldDay.merge.queued')}</>}
                </span>
              )}
              {mergeError && (
                <span className="log-export-error" role="alert">{mergeError}</span>
              )}
            </div>
            <div className="fd-upload-row">
              <button
                type="button"
                role="switch"
                aria-checked={fieldDay.upload.enabled}
                className={`toggle${fieldDay.upload.enabled ? ' on' : ''}`}
                onClick={() => void saveUpload(!fieldDay.upload!.enabled, uploadDests)}
                aria-label={t('fieldDay.upload.aria')}
              >
                <span className="toggle-knob" />
              </button>
              <span className="fd-upload-label">{t('fieldDay.upload.label')}</span>
              {/* Both halves are required: a switch with nowhere to send is off, and a
                  destination alone must not enqueue. */}
              {fieldDay.upload.available.map((id) => (
                <label className="fd-upload-dest" key={id}>
                  <input
                    type="checkbox"
                    checked={uploadDests.includes(id)}
                    onChange={(e) =>
                      void saveUpload(
                        fieldDay.upload!.enabled,
                        e.target.checked
                          ? [...uploadDests, id]
                          : uploadDests.filter((d) => d !== id),
                      )
                    }
                  />
                  {/* The service's own name; the tick stores its id. */}
                  <span>{SERVICE_NAMES[id] ?? id}</span>
                </label>
              ))}
            </div>
            <p className="fd-upload-hint" role="note">
              {fieldDay.upload.hint}
            </p>
          </div>
        )}
      </div>
      {/* …and, once, where sending happens, until it is dismissed on this computer. */}
      {workIn && workInHint && (
        <p className="fd-work-in-hint" role="note" aria-label={t('fieldDay.workIn.hint.aria')}>
          {t('fieldDay.workIn.hint')}{' '}
          <button
            type="button"
            className="settings-linkbtn"
            onClick={() => {
              dismissWorkInHint()
              setWorkInHint(false)
            }}
          >
            {t('fieldDay.workIn.hint.dismiss')}
          </button>
        </p>
      )}

      {/* CLUB SYNC (chip + counters + band board) — only while hosting/joined */}
      {fieldDay?.club && (
        <FdClubSection
          club={fieldDay.club}
          onExport={observed ? undefined : handleExport}
          onGivePosition={observed ? undefined : fdClubGivePosition}
          onRemovePosition={observed ? undefined : fdClubRemovePosition}
          onAllowBack={observed ? undefined : fdClubAllowBack}
          busy={busy !== null}
          readOnly={observed}
          fieldDay={fdEventIsFieldDay}
          keepsDupes={fieldDay.dupeRule?.logDupes === true}
          link={observed ? null : clubLink}
          bands={fieldDay.bands}
          dupeRule={fieldDay.dupeRule}
          ownPosid={observed ? undefined : nativeSettings?.fdPositionId}
          clubLog={observed ? undefined : { receives: fieldDay.receives ?? [], role: fieldDay.role, isWfd }}
        />
      )}
      {/* …or, with the Field Day mode off, where this laptop's club link stands: club sync
          follows its own settings, not that switch. */}
      {!fieldDay?.club && !clubRefusal && !observed && clubLink && <FdClubLink link={clubLink} />}
      {/* …or why it is not syncing: switched on for a contest club sync cannot run. The
          station's own settings, which a Remote observation does not carry. */}
      {!fieldDay?.club && clubRefusal && (
        <FdClubRefused contest={contestName(nativeSettings?.fdEvent?.trim())} why={clubRefusal} />
      )}
      {/* What the journal restore kept out of this session, and why. Nothing was deleted. */}
      {fieldDay?.keptOut && <FdKeptOutNote keptOut={fieldDay.keptOut} contest={eventName} />}
      {/* The contest journal's writes are failing: the contacts since the last good one are in
          memory only. Red until a write lands. Once per window: the entry above says it when there is one. */}
      {fieldDay?.unsaved && !entryAbove && (
        <p className="fd-score-note fd-unsaved" role="alert">
          {contestUnsavedText(fieldDay.unsaved)}
        </p>
      )}
      {/* …and the ones past its four days that the logbook does not hold, with their merge: never
          asked for by a Remote observer, so never shown there, and never pressed from afar. */}
      {(unmerged.length > 0 || unmergedReport || unmergedError) && (
        <FdUnmergedNote
          unmerged={unmerged}
          busy={unmergedBusy}
          report={unmergedReport}
          error={unmergedError}
          onMerge={(event) => void mergeUnmerged(event)}
        />
      )}

      {/* SCOREBOARD or STATS — two tabs over the same session, read-only both. */}
      <div className="filter-row contest-tabs" role="tablist" aria-label={t('contestStats.tabs.aria')}>
        <button type="button" role="tab" aria-selected={!statsTab} className={`filter-chip${statsTab ? '' : ' active'}`} onClick={() => setStatsTab(false)}>
          {t('contestStats.tab.score')}
        </button>
        <button
          type="button"
          role="tab"
          aria-selected={statsTab}
          aria-controls={statsTab ? 'contest-stats-panel' : undefined}
          className={`filter-chip${statsTab ? ' active' : ''}`}
          onClick={() => setStatsTab(true)}
        >
          {t('contestStats.tab.stats')}
        </button>
      </div>
      {statsTab ? (
        <ContestStats
          fieldDay={fieldDay}
          nowUnix={nowUnix}
          myCall={nativeSettings?.mycall ?? ''}
          myGrid={nativeSettings?.mygrid ?? ''}
          contest={eventName}
          score={
            <>
              {fieldDay?.scoreNoteKey && SCORE_NOTE_KEYS.has(fieldDay.scoreNoteKey) && (
                <p className="fd-score-note" role="note">
                  {t(fieldDay.scoreNoteKey as MessageKey)}
                </p>
              )}
              <ScoreMath fieldDay={fieldDay} settings={settings} />
            </>
          }
          mults={
            fieldDay?.boards?.length ? (
              <MultiplierBoards boards={fieldDay.boards} />
            ) : (
              <SectionsBoard workedSet={workedSectionSet(fieldDay)} />
            )
          }
        />
      ) : (
      <>
      {/* SCOREBOARD (operator + score tiles + sections board) */}
      <FieldDayScoreboard
        fieldDay={fieldDay}
        settings={settings}
        onSaveOperator={saveOperator}
        readOnly={observed}
        operatorBox={!operatorBoxAbove}
        operatorAsk={operatorAsk}
        onOperatorFocused={onOperatorFocused}
      />

      {/* SCORING: the power multiplier and the bonus chase, in one place.
          Both halves of the score used to live on different screens — the multiplier in
          Settings ▸ Contesting, the bonuses down here — so nobody could see the sum they
          make. The multiplier keeps its Settings home (that is the registry's, and the
          manual's, address for it) and is MIRRORED here on the same field: an operator
          meets scoring where the score is, and there is only ever one value. */}
      {/* Field Day's bonus menu and power tier. No other contest has either, so it is not
          offered under a score it cannot change — and Winter Field Day, which scores by its
          objectives, has the objectives in their place. */}
      {wfdOm !== null && (
        <WfdObjectivesSection
          earned={settings?.fdObjectives ?? []}
          planned={settings?.fdObjectivesPlanned ?? []}
          log={log}
          om={wfdOm}
          power={observed ? undefined : (nativeSettings?.contestCategoryPower ?? '')}
          readOnly={observed || logger}
          open={bonusOpen}
          onToggleOpen={() => setBonusOpen((v) => !v)}
          onToggleEarned={(id) => void toggleObjective(id)}
          onTogglePlanned={(id) => void toggleObjectivePlanned(id)}
        />
      )}
      {fdEventIsFieldDay && wfdOm === null && (
      <div className="fd-bonuses-section">
        <button
          type="button"
          className="fd-bonuses-toggle"
          onClick={() => setBonusOpen((v) => !v)}
          aria-expanded={bonusOpen}
        >
          <span>{t('fieldDay.bonuses.head')}</span>
          <span className="fd-scoring-power-chip">
            {t('fieldDay.scoring.power.chip', { mult: fdPowerMult })}
          </span>
          <span className="fd-bonuses-count">
            {t('fieldDay.bonuses.count', {
              claimed: claimedBonuses.length,
              total: FD_BONUSES.length,
              points: bonusPoints,
            })}
          </span>
          {tally.plannedCount > 0 && (
            <span className="fd-bonuses-planned-count">
              {t('fieldDay.bonuses.planned.count', {
                count: tally.plannedCount,
                points: tally.plannedPoints,
              })}
            </span>
          )}
          <span className="fd-bonuses-chevron">{bonusOpen ? '▲' : '▼'}</span>
        </button>
        {bonusOpen && (
          <div className="fd-bonuses-body">
            {/* POWER MULTIPLIER — the same `fdPowerMult` Settings writes. A whole-settings
                save, like the bonus checkboxes beside it: the engine keeps the contest log
                across it (engine.rs pins that), and a power tier is set once per event. */}
            <div className="fd-power-row">
              <span className="fd-power-label">{t('settings.fieldDay.power.label')}</span>
              <div
                className="fd-power-chips"
                role="group"
                aria-label={t('settings.fieldDay.power.aria')}
              >
                {FD_POWER_TIERS.map((p) => (
                  <button
                    key={p.value}
                    type="button"
                    className={`fd-power-chip${fdPowerMult === p.value ? ' active' : ''}`}
                    aria-pressed={fdPowerMult === p.value}
                    title={t(p.hintKey)}
                    disabled={observed || logger || !settings}
                    onClick={() => void setPowerMult(p.value)}
                  >
                    {t(p.labelKey)}
                  </button>
                ))}
              </div>
              <span className="fd-power-hint">{t('settings.fieldDay.power.hint')}</span>
            </div>

            {/* THE CHASE. Which number is real is the whole point of this strip: earned
                points are in the score, planned points are not, and the ceiling is what
                the club is still chasing. */}
            <div className="fd-chase" role="group" aria-label={t('fieldDay.bonuses.chase.aria')}>
              <div className="fd-chase-tile earned">
                <span className="fd-chase-val">
                  {t('fieldDay.bonuses.chase.earned', { points: tally.earnedPoints })}
                </span>
                <span className="fd-chase-note">{t('fieldDay.bonuses.chase.earned.note')}</span>
              </div>
              <div className="fd-chase-tile planned">
                <span className="fd-chase-val">
                  {t('fieldDay.bonuses.chase.planned', { points: tally.plannedPoints })}
                </span>
                <span className="fd-chase-note">{t('fieldDay.bonuses.chase.planned.note')}</span>
              </div>
              <div className="fd-chase-tile potential">
                <span className="fd-chase-val">
                  {t('fieldDay.bonuses.chase.potential', { points: tally.potentialPoints })}
                </span>
              </div>
            </div>

            <div className="fd-bonuses-list" role="group" aria-label={t('fieldDay.bonuses.aria')}>
              {FD_BONUSES.map((b) => {
                const state = fdBonusState(b.id, claimedBonuses, plannedBonuses)
                const earned = state === 'earned'
                const onPlan = plannedBonuses.includes(b.id)
                return (
                  <div
                    key={b.id}
                    className={`fd-bonus-row${earned ? ' checked' : ''}${state === 'planned' ? ' planned' : ''}`}
                    data-bonus-state={state}
                  >
                    <input
                      id={`fd-bonus-${b.id}`}
                      type="checkbox"
                      checked={earned}
                      disabled={observed || logger}
                      onChange={() => void toggleEarned(b.id)}
                      aria-label={t('fieldDay.bonus.aria', { label: b.label, points: b.points })}
                    />
                    <label className="fd-bonus-label" htmlFor={`fd-bonus-${b.id}`}>{b.label}</label>
                    <button
                      type="button"
                      className={`fd-bonus-plan${onPlan ? ' on' : ''}`}
                      aria-pressed={onPlan}
                      disabled={observed || logger}
                      title={t('fieldDay.bonus.plan.title')}
                      aria-label={t('fieldDay.bonus.plan.aria', { label: b.label })}
                      onClick={() => void togglePlanned(b.id)}
                    >
                      {onPlan ? t('fieldDay.bonus.plan.on') : t('fieldDay.bonus.plan.off')}
                    </button>
                    <span className="fd-bonus-pts">{t('fieldDay.bonus.pts', { points: b.points })}</span>
                  </div>
                )
              })}
            </div>
          </div>
        )}
      </div>
      )}
      </>
      )}

      {/* THE LOG'S DIVIDER (the Illinois QSO Party, 2026-10-18: "splitters to resize sub-windows (county
          list etc.)"). It sizes the log, and the multiplier board above takes what is left, down to its
          floor, past which the screen scrolls. It paints NOTHING until moved: the log's stock height is
          the sheet's 190 px cap (`.fd-log[data-sized]`, styles.css). Moved, the size is stored per window
          and fitted into the screen on every load and resize; Backspace or a double-click gives the stock
          log back. It stops nothing and sends nothing. */}
      <PaneSeam
        axis="y"
        varName="--fd-log-h"
        strip={logBox}
        after
        storageKey="nexus.split.contest.log"
        min={CONTEST_LOG_SPLIT_MIN}
        max={CONTEST_LOG_SPLIT_MAX}
        maxShare={CONTEST_LOG_MAX_SHARE}
        defaultPct={null}
        label={t('fieldDay.log.splitter.label')}
        className="contest-seam"
      />
      {/* LOG TABLE */}
      <div className="fd-log" ref={logBox}>
        <ContestLogHead
          fieldDayColumns={fdEventIsFieldDay}
          isWfd={isWfd}
          slotColumns={slotColumns}
          role={fieldDay?.role}
          numbered={numbered}
        />
        <div className="fd-log-scroll" ref={logPin.ref} onScroll={logPin.onScroll}>
          {rows.length === 0 && <p className="empty">{t('fieldDay.log.empty')}</p>}
          {rows.map((r, i) => {
            // The contact being edited shows its edit in place of its rows: in its first row's
            // place, and its other counties' rows not at all.
            if (editRows.includes(r.qso)) {
              return r.qso === editRows[0] ? (
                <ContestContactEdit
                  key={`edit-${r.qso.call}-${r.qso.whenUnix}`}
                  rows={editRows}
                  receives={fieldDay?.receives ?? []}
                  role={fieldDay?.role}
                  bands={fieldDay?.bands}
                  onSaving={() => {
                    saving.current = true
                  }}
                  onDone={editDone}
                />
              ) : null
            }
            return (
              <ContestLogRow
                key={`${r.qso.call}-${i}`}
                row={r}
                fieldDayColumns={fdEventIsFieldDay}
                slotColumns={slotColumns}
                numbered={numbered}
              >
                {removable && r.qso.whenUnix ? (
                  <button
                    type="button"
                    className="fd-edit"
                    title={t('contest.edit.title')}
                    aria-label={t('contest.edit.aria', { contact: contactLabel([r.qso]) })}
                    onClick={() => editRow(r.qso)}
                  >
                    {t('contest.edit.label')}
                  </button>
                ) : null}
                {removable && i === rows.length - 1 && (
                  <button
                    type="button"
                    className="fd-remove"
                    aria-pressed={removeAsk != null}
                    title={t('fieldDay.log.remove.title')}
                    onClick={removeNewest}
                  >
                    {t('fieldDay.log.remove')}
                  </button>
                )}
              </ContestLogRow>
            )
          })}
        </div>
      </div>

      {/* THE REMOVED LIST — kept contacts, with Restore. Only while there is something to say,
          so the log keeps every pixel otherwise. */}
      {removable && (removedList.length > 0 || removeAsk != null || removedNote != null) && (
        <div className="fd-removed" role="region" aria-label={t('fieldDay.removed.aria')}>
          {removeAsk != null && newest && (
            <p className="fd-removed-note" role="alert">
              {t('fieldDay.log.remove.ask', { contact: contactLabel(newest.rows) })}
            </p>
          )}
          {removeAsk == null && removedNote && (
            <p className="fd-removed-note" role={removedNote.alert ? 'alert' : 'status'}>
              {removedNote.text}
            </p>
          )}
          {removedList.length > 0 && (
            <details className="fd-removed-list">
              <summary>{t('fieldDay.removed.title', { count: removedList.length })}</summary>
              <p className="fd-removed-hint">{t('fieldDay.removed.note')}</p>
              {removedList.map((entry) => (
                <div className="fd-removed-row" key={entry.id}>
                  <span className="mono">{contactLabel(entry.rows)}</span>
                  <span className="fd-removed-at">
                    {t('fieldDay.removed.at', { time: utcHhmm(entry.removedUnix) })}
                  </span>
                  <button
                    type="button"
                    className="fd-remove"
                    title={t('fieldDay.removed.restore.title')}
                    onClick={() => restore(entry)}
                  >
                    {t('fieldDay.removed.restore')}
                  </button>
                </div>
              ))}
            </details>
          )}
        </div>
      )}
    </section>
  )
}
