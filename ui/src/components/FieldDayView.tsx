import { Fragment, useEffect, useMemo, useState, type CSSProperties } from 'react'
import type { FdClubStatus, FieldDayQso, FieldDayStatus, ModeRequest, Settings } from '../types'
import { exportLog, fdClubExport, getSettings, setSettings, setFdOperator, openPanelWindow, saveTextToDownloads, type FdRulesetDto } from '../api'
import { FdAdvisories } from './FdAdvisories'
import { pushToast } from '../toast'
import { fdEventFromWindow, fdHeaderSubtitle, FD_EVENT_NAMES, type FdKind } from '../fdEvent'
import { usePinnedScroll } from '../usePinnedScroll'
import { ARRL_SECTIONS_BY_DIVISION, ARRL_SECTION_TOTAL } from '../features/arrlSections'
import { t } from '../i18n'
import { T } from '../i18n/T'

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
]

// ---------------------------------------------------------------------------
// Internal types
// ---------------------------------------------------------------------------

interface Props {
  fieldDay: FieldDayStatus | null
  onSetMode: (mode: ModeRequest) => void
  /** The Field Day master switch (settings.fdActive) — gates the advisories. */
  fdActive?: boolean
  /** The active event's ruleset facts (App fetches get_fd_ruleset once per
   *  configured event) — the warn-only advisories read these. */
  fdRuleset?: FdRulesetDto | null
  /** The active digital tier (App's snap.link.tier) — the banned-mode chip
   *  checks it against the ruleset's bannedModes. */
  tier?: string
}

interface LogRowMeta {
  qso: FieldDayQso
  /** first appearance of this section in the log = a new multiplier */
  isNewSection: boolean
  /** the same (call, band, mode) appears more than once in the log = a rule dupe */
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
 * Annotate each log entry with multiplier / dupe state. Sections are marked the
 * first time they appear (scanning oldest -> newest). A QSO is a dupe only when
 * the same station is worked twice on the same band AND mode — matching the Rust
 * FD dupe key (call, band, mode class), which permits the same call once per
 * band per mode (e.g. W1AW on 20m and 40m are two legal contacts).
 */
export function annotate(log: FieldDayQso[]): LogRowMeta[] {
  const seenSections = new Set<string>()
  const dupeKey = (q: FieldDayQso) => `${q.call}|${q.band}|${q.mode ?? ''}`
  const dupeCounts = new Map<string, number>()
  for (const q of log) dupeCounts.set(dupeKey(q), (dupeCounts.get(dupeKey(q)) ?? 0) + 1)
  return log.map((q) => {
    const isNewSection = !seenSections.has(q.section)
    seenSections.add(q.section)
    return {
      qso: q,
      isNewSection,
      isDupe: (dupeCounts.get(dupeKey(q)) ?? 0) > 1,
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

/** "HH:MM" UTC for the FD log's time column. Blank the column when the QSO
 * predates the timestamp field or hasn't been logged yet. */
function qsoTimeUtc(q: FieldDayQso): string {
  const unix = q.whenUnix
  if (!unix) return ''
  const d = new Date(unix * 1000)
  const p = (n: number) => String(n).padStart(2, '0')
  return `${p(d.getUTCHours())}:${p(d.getUTCMinutes())}`
}

// ---------------------------------------------------------------------------
// Client-side summary + dupe-sheet exports (spec §6). Both derive from the same
// log + annotate()/modeCounts() the board already uses — no backend command —
// and save through saveTextToDownloads() like the Cabrillo/ADIF paths.
// ---------------------------------------------------------------------------

// Canonical band order (HF → VHF) so the summary lists bands top-down like a rig.
const BAND_ORDER = ['160m', '80m', '60m', '40m', '30m', '20m', '17m', '15m', '12m', '10m', '6m', '4m', '2m', '1.25m', '70cm', '33cm', '23cm']
function bandRank(b: string): number {
  const i = BAND_ORDER.indexOf(b)
  return i === -1 ? BAND_ORDER.length : i
}

/** Count QSOs per band, in canonical HF→VHF order. */
function bandCounts(log: FieldDayQso[]): { band: string; n: number }[] {
  const m = new Map<string, number>()
  for (const q of log) m.set(q.band, (m.get(q.band) ?? 0) + 1)
  return [...m.entries()]
    .map(([band, n]) => ({ band, n }))
    .sort((a, b) => bandRank(a.band) - bandRank(b.band) || a.band.localeCompare(b.band))
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
  L.push(`Station class ${a.myClass || '—'}   Section ${a.mySection || '—'}`)
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
  L.push('')
  const secs = [...a.workedSet].sort()
  L.push(`Sections worked (${secs.length}):  ${secs.length ? secs.join(' ') : '—'}`)
  L.push('')
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

/** Dupe / multiplier check sheet: new-section multipliers + alphabetical callsign list. */
function buildDupeSheetText(rows: LogRowMeta[]): string {
  const L: string[] = []
  L.push('FIELD DAY — DUPE & MULTIPLIER SHEET')
  L.push(`Generated ${new Date().toISOString()}`)
  L.push('')

  const mults = rows.filter((r) => r.isNewSection)
  L.push(`MULTIPLIERS — sections worked (${mults.length})`)
  if (mults.length === 0) L.push('  (none yet)')
  else for (const r of mults) {
    L.push(`  ${r.qso.section.padEnd(5)} first worked by ${r.qso.call} on ${r.qso.band}`)
  }
  L.push('')

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
  fontSize: 11,
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
  fontSize: 14,
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
        <span style={{ fontSize: 15, fontWeight: 700, color: 'var(--text)' }}>
          {t('fieldDay.sections.head')}
        </span>
        <span style={{ fontSize: 13, color: 'var(--text-dim)' }}>
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

// ---------------------------------------------------------------------------
// Shared scoreboard (spec: the score tiles + sections board + settable operator),
// rendered by BOTH the docked FieldDayView and the torn-off scoreboard window.
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
}

/** Score components from the snapshot (new fields); fall back to computed if absent. */
function computeFdScore(fieldDay: FieldDayStatus | null, settings: Settings | null): FdScore {
  const fdPowerMult = settings?.fdPowerMult ?? 1
  const qsoPts = fieldDay?.points ?? 0
  const poweredPoints = fieldDay?.poweredPoints ?? qsoPts * fdPowerMult
  const claimedBonusIds = settings?.fdBonuses ?? []
  const bonusPoints = fieldDay?.bonusPoints ?? FD_BONUSES
    .filter((b) => claimedBonusIds.includes(b.id))
    .reduce((sum, b) => sum + b.points, 0)
  const totalScore = fieldDay?.totalScore ?? poweredPoints + bonusPoints
  return { fdPowerMult, qsoPts, poweredPoints, claimedBonusIds, bonusPoints, totalScore }
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
  fontSize: 12,
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
  fontSize: 13,
  alignItems: 'baseline',
}
const CLUB_COL_HEAD: CSSProperties = {
  fontSize: 10,
  fontWeight: 700,
  letterSpacing: '0.04em',
  textTransform: 'uppercase',
  color: 'var(--text-faint)',
}
/** Column heads, at the docked size or the torn-off window's glance size. */
function colHead(big: boolean): CSSProperties {
  return big ? { ...CLUB_COL_HEAD, fontSize: 13 } : CLUB_COL_HEAD
}
const CLUB_WARN: CSSProperties = {
  fontSize: 12,
  color: 'var(--status-new-entity)',
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
export function FdClubSection({
  club,
  onExport,
  busy = false,
  detached = false,
}: {
  club: FdClubStatus
  onExport?: (format: 'club-cabrillo' | 'club-adif') => void
  busy?: boolean
  detached?: boolean
}) {
  // The glance scale. One flag, applied at the handful of places that carry a
  // px size, so the docked board is byte-for-byte what it was.
  const big = detached
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
        <span style={{ fontSize: big ? 17 : 13, fontWeight: 700, color: 'var(--text)' }}>
          {t('fieldDay.club.head')}
        </span>
        <span
          style={big ? { ...clubChipStyle(club.syncState), fontSize: 14, padding: '4px 12px' } : clubChipStyle(club.syncState)}
          title={t('fieldDay.club.state.title')}
        >
          {clubChipText(club)}
        </span>
        {(club.event || club.hostCall) && (
          <span style={{ fontSize: big ? 15 : 12, color: 'var(--text-dim)' }}>
            {t('fieldDay.club.hostLine', { event: club.event || '—', call: club.hostCall || '—' })}
          </span>
        )}
        <span style={{ flex: '1 1 auto' }} />
        <span style={{ fontSize: big ? 16 : 13, color: 'var(--text-dim)' }}>
          {t('fieldDay.club.counters', {
            score: club.score,
            qsos: club.qsos,
            sections: club.sections,
          })}
        </span>
        {club.hosting && onExport && (
          <>
            <button
              type="button"
              className="export-btn"
              disabled={busy}
              onClick={() => onExport('club-cabrillo')}
              title={t('fieldDay.club.export.cabrillo.title')}
            >
              {t('fieldDay.club.export.cabrillo.label')}
            </button>
            <button
              type="button"
              className="export-btn"
              disabled={busy}
              onClick={() => onExport('club-adif')}
              title={t('fieldDay.club.export.adif.title')}
            >
              {t('fieldDay.club.export.adif.label')}
            </button>
          </>
        )}
        {!detached && (
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
      {Math.abs(club.skewSecs) > 30 && (
        <div style={CLUB_WARN} role="alert">
          {t('fieldDay.club.skew', { secs: Math.abs(club.skewSecs) })}
        </div>
      )}
      {club.lastError && (
        <div style={CLUB_WARN} role="alert">
          {t('fieldDay.club.error', { msg: club.lastError })}
        </div>
      )}
      {club.board.length === 0 ? (
        // Sync IS on here (the block only rides the snapshot when it is), so this
        // says what it is waiting for and never sends anyone to Settings — the
        // torn-off window's own copy covers the sync-off case.
        <span style={{ fontSize: big ? 16 : 12, color: 'var(--text-faint)' }}>
          {t('fieldDay.club.board.empty')}
        </span>
      ) : (
        <div
          data-club-board=""
          style={big ? { ...CLUB_BOARD_GRID, fontSize: 20, columnGap: 18, rowGap: 8 } : CLUB_BOARD_GRID}
        >
          <span style={colHead(big)}>{t('fieldDay.club.board.column.position')}</span>
          <span style={colHead(big)}>{t('fieldDay.club.board.column.band')}</span>
          <span style={colHead(big)}>{t('fieldDay.club.board.column.mode')}</span>
          <span style={colHead(big)}>{t('fieldDay.club.board.column.operator')}</span>
          <span style={colHead(big)}>{t('fieldDay.club.board.column.qsos')}</span>
          <span style={colHead(big)}>{t('fieldDay.club.board.column.rate')}</span>
          {club.board.map((row) => {
            // Stale-mark past 15 s (the DEAD_SECS threshold): readings stay on
            // screen but never silently stale.
            const stale = row.lastSeenSecs > 15
            const dim: CSSProperties = stale ? { opacity: 0.45 } : {}
            return (
              <Fragment key={row.posid}>
                <span
                  className="mono"
                  style={{ ...dim, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}
                  title={
                    stale
                      ? t('fieldDay.club.board.stale', { secs: row.lastSeenSecs })
                      : undefined
                  }
                >
                  {/* An unnamed position falls back to who is sitting at it, and only
                      then to a translated placeholder — never to the position id, which is
                      internal plumbing an operator should never be shown. */}
                  {row.posName || row.operator || t('fieldDay.club.board.unnamed')}
                  {stale && <span aria-hidden="true"> ⚠</span>}
                </span>
                <span className="mono" style={dim}>{row.band}</span>
                <span className="mono" style={dim}>{row.mode}</span>
                <span className="mono" style={{ ...dim, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{row.operator}</span>
                <span className="mono" style={dim}>{row.qsos}</span>
                <span className="mono" style={dim}>
                  {t('fieldDay.club.board.rate', { rate: row.rate })}
                </span>
              </Fragment>
            )
          })}
        </div>
      )}
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
}
const OP_LABEL: CSSProperties = {
  fontSize: 10,
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
  fontSize: 15,
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
  fontSize: 13,
  fontWeight: 600,
  color: 'var(--text-dim)',
  background: 'var(--bg-elev)',
  border: '1px solid var(--border-soft)',
  borderRadius: 'var(--radius-sm)',
  cursor: 'pointer',
  whiteSpace: 'nowrap',
}

/**
 * The reusable Field Day scoreboard: the settable operator, the score tiles, and the
 * worked-sections board. `onSaveOperator` persists the operator (optimistic, parent-
 * owned) so a torn-off window and the docked view can't clobber each other's writes.
 * `detached` hides the pop-out button in the already-torn-off window.
 */
export function FieldDayScoreboard({
  fieldDay,
  settings,
  onSaveOperator,
  detached = false,
}: {
  fieldDay: FieldDayStatus | null
  settings: Settings | null
  onSaveOperator: (call: string) => void
  detached?: boolean
}) {
  const log = fieldDay?.log ?? []
  const isWfd = (fieldDay?.event ?? '') === 'wfd'
  const modes = useMemo(() => modeCounts(log), [log])
  const workedSet = useMemo(() => workedSectionSet(fieldDay), [fieldDay])
  const { fdPowerMult, qsoPts, poweredPoints, bonusPoints, totalScore } = computeFdScore(
    fieldDay,
    settings,
  )

  // Local draft so typing is smooth; commit (persist) on blur / Enter — a per-keystroke
  // setSettings would fire the heavyweight apply repeatedly.
  const [opDraft, setOpDraft] = useState(settings?.fdOperator ?? '')
  useEffect(() => {
    setOpDraft(settings?.fdOperator ?? '')
  }, [settings?.fdOperator])
  const commitOp = () => {
    const v = opDraft.trim()
    if (v === (settings?.fdOperator ?? '')) return
    onSaveOperator(v)
  }

  return (
    <>
      {/* OPERATOR + POP-OUT */}
      <div style={SCOREBOARD_HEADER}>
        <label style={OP_FIELD}>
          <span style={OP_LABEL}>{t('fieldDay.operator.label')}</span>
          <input
            style={OP_INPUT}
            value={opDraft}
            disabled={!settings}
            onChange={(e) => setOpDraft(e.target.value.toUpperCase())}
            onBlur={commitOp}
            onKeyDown={(e) => {
              if (e.key === 'Enter') e.currentTarget.blur()
            }}
            placeholder={t('fieldDay.operator.placeholder')}
            aria-label={t('fieldDay.operator.aria')}
            spellCheck={false}
            autoCapitalize="characters"
          />
        </label>
        {!detached && (
          <button
            type="button"
            style={POPOUT_BTN}
            onClick={() => void openPanelWindow('fieldday')}
            title={t('fieldDay.popOut.title')}
          >
            {t('fieldDay.popOut.label')}
          </button>
        )}
      </div>

      {/* SCORE TILES */}
      <div className="fd-scoreboard">
        <div className="fd-score">
          <span className="fd-score-val">{fieldDay?.qsoCount ?? 0}</span>
          <span className="fd-score-label">{t('fieldDay.score.qsos')}</span>
        </div>
        <div className="fd-score">
          <span className="fd-score-val">{fieldDay?.sections ?? 0}</span>
          <span className="fd-score-label">{t('fieldDay.score.sections')}</span>
        </div>
        {/* Per-mode chips — a count and a mode code, both invariant. */}
        <div className="fd-mode-chips">
          {modes.dig > 0 && <span className="fd-mode-chip dig">{modes.dig} {FD_MODE_CODES.dig}</span>}
          {modes.cw > 0 && <span className="fd-mode-chip cw">{modes.cw} {FD_MODE_CODES.cw}</span>}
          {modes.ph > 0 && <span className="fd-mode-chip ph">{modes.ph} {FD_MODE_CODES.ph}</span>}
        </div>
        {/* Score math */}
        {isWfd ? (
          /* WFD scores by OBJECTIVES (QSOs × (multipliers+1)) — we don't track
             operator counts/objectives, so showing ARRL power×+bonus math would
             claim a number WFD rules never produce. Show the honest raw counts. */
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
        )}
        <div className="fd-state-chip" title={t('fieldDay.state.title')}>
          {fieldDay?.state ?? t('fieldDay.state.idle')}
        </div>
      </div>

      {/* SECTIONS BOARD */}
      <SectionsBoard workedSet={workedSet} />
    </>
  )
}

export function FieldDayView({ fieldDay, onSetMode, fdActive = false, fdRuleset = null, tier }: Props) {
  // Log tail: bottom-pinned via the shared discipline. The old unconditional
  // snap on every logged QSO undid a mid-run scroll-back (checking a call two
  // contacts up) the moment the next contact landed. Pinned follows the run;
  // scrolled-up checking is never yanked.
  const logPin = usePinnedScroll<HTMLDivElement>()
  const running = fieldDay?.running ?? false
  const log = fieldDay?.log ?? []
  const [exportError, setExportError] = useState<string | null>(null)
  const [busy, setBusy] = useState<ExportFormat | null>(null)
  const [bonusOpen, setBonusOpen] = useState(false)

  // Settings round-trip for the bonus checklist (same pattern as specialOp in OperateCockpit).
  const [settings, setSettingsState] = useState<Settings | null>(null)
  useEffect(() => {
    let live = true
    getSettings().then((s) => live && setSettingsState(s)).catch(() => {})
    return () => { live = false }
  }, [])

  const rows = useMemo(() => annotate(log), [log])
  const modes = useMemo(() => modeCounts(log), [log])

  // Worked-section set for the summary/dupe exports (the board derives its own
  // inside FieldDayScoreboard).
  const workedSet = useMemo(() => workedSectionSet(fieldDay), [fieldDay])

  const toggleBonus = async (id: string) => {
    if (!settings) return
    const cur = settings.fdBonuses ?? []
    const next = cur.includes(id) ? cur.filter((b) => b !== id) : [...cur, id]
    const updated: Settings = { ...settings, fdBonuses: next }
    setSettingsState(updated)
    try {
      await setSettings(updated)
    } catch {
      // Revert optimistic update on failure
      setSettingsState(settings)
    }
  }

  // Persist the settable Field Day operator (optimistic). NOT the whole-struct save that
  // toggleBonus uses: a seat swap happens mid-QSO, and the heavyweight path drops the TX
  // queue and re-derives the TX cycle from this component's settings snapshot (#54). Since
  // #100 it no longer resets the operating mode, but those two still land on a live contact.
  // The engine trims + uppercases.
  const saveOperator = async (call: string) => {
    if (!settings) return
    const op = call.trim().toUpperCase()
    setSettingsState({ ...settings, fdOperator: op })
    try {
      await setFdOperator(op)
    } catch {
      setSettingsState(settings)
    }
  }

  // Event header: the window arrives Rust-computed on the DTO (fd_rules data —
  // real 27 h SFD / 30 h WFD durations; the old TS date math hardcoded 24 h and
  // called WFD over with six hours left). Snapshots refresh it, so the year
  // rollover and the active→next transition need no client-side clock walk.
  const eventKind: FdKind = (fieldDay?.event === 'wfd' ? 'wfd' : 'arrlfd')
  const isWfd = eventKind === 'wfd'
  const fdEvent = useMemo(
    () => fdEventFromWindow(eventKind, fieldDay?.eventStartUnix, fieldDay?.eventEndUnix),
    [eventKind, fieldDay?.eventStartUnix, fieldDay?.eventEndUnix],
  )
  const subtitle = useMemo(() => (fdEvent ? fdHeaderSubtitle(new Date(), fdEvent) : ''), [fdEvent])

  // Score components (shared with the scoreboard tiles) — needed here for the
  // Summary export + the bonuses count.
  const { fdPowerMult, qsoPts, poweredPoints, claimedBonusIds: claimedBonuses, bonusPoints, totalScore } =
    computeFdScore(fieldDay, settings)

  // Two words for two events, not one word with a variant: ARRL calls the exchange field
  // Class and WFD calls it Category.
  const classLabel = isWfd ? t('fieldDay.log.column.category') : t('fieldDay.log.column.class')

  // Cabrillo/ADIF come from the backend; Summary/Dupe sheet are derived client-side
  // from the same log the board renders (no backend command). Defined here so it can
  // read the score components computed just above.
  const handleExport = async (format: ExportFormat) => {
    setExportError(null)
    setBusy(format)
    try {
      let text: string
      if (format === 'summary') {
        text = buildSummaryText({
          eventName: isWfd ? FD_EVENT_NAMES.wfd : FD_EVENT_NAMES.arrlfd,
          isWfd,
          rulesYear: fieldDay?.rulesYear ?? 0,
          rulesGenerated: fieldDay?.rulesGenerated ?? '',
          myClass: fieldDay?.myClass ?? '',
          mySection: fieldDay?.mySection ?? '',
          log,
          modes,
          workedSet,
          powerMult: fdPowerMult,
          qsoPts,
          poweredPoints,
          bonusPoints,
          totalScore,
          claimedBonuses: FD_BONUSES.filter((b) => claimedBonuses.includes(b.id)),
        })
      } else if (format === 'dupesheet') {
        text = buildDupeSheetText(rows)
      } else if (format === 'club-cabrillo' || format === 'club-adif') {
        // The MERGED club log from the host, deduped earliest-wins — the
        // submittable club artifact (host role only; the backend refuses
        // elsewhere and the buttons only render while hosting).
        text = await fdClubExport(format === 'club-cabrillo' ? 'cabrillo' : 'adif')
      } else {
        text = await exportLog(format)
      }
      const stamp = new Date().toISOString().slice(0, 10)
      const base =
        format === 'cabrillo' || format === 'adif'
          ? 'fd-log'
          : format.startsWith('club-')
            ? 'fd-club-log'
            : `fd-${format}`
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

  return (
    <section className="conversation panel fieldday">
      {/* EVENT BANNER */}
      <div className="fd-event-banner">
        <span className="fd-event-name">{isWfd ? FD_EVENT_NAMES.wfd : FD_EVENT_NAMES.arrlfd}</span>
        <span className="fd-event-subtitle">{subtitle}</span>
        {/* Warn-only rule advisories (banned mode + assistance) — passive status
            lines; nothing is ever removed or disabled by rule. */}
        <FdAdvisories
          fdActive={fdActive}
          ruleset={fdRuleset}
          activeMode={tier}
          assistanceOn={fieldDay?.assistanceOn ?? []}
          showAssistance
        />
        {fieldDay?.rulesYear ? (
          <span className="fd-event-rules">
            {t('fieldDay.rules.line', {
              year: fieldDay.rulesYear,
              date: (fieldDay.rulesGenerated ?? '').slice(0, 10),
            })}
          </span>
        ) : null}
      </div>

      <div className="panel-header fd-header">
        <div className="fd-ident">
          <h2 className="conv-peer">{isWfd ? FD_SHORT_NAMES.wfd : FD_SHORT_NAMES.arrlfd}</h2>
          <span className="fd-class">
            {fieldDay?.myClass ?? '—'}
            <span className="fd-section"> {fieldDay?.mySection ?? '—'}</span>
          </span>
        </div>
        <div className="fd-role-toggle" role="group" aria-label={t('fieldDay.role.aria')}>
          <button
            type="button"
            className={`fd-role-btn${running ? ' active' : ''}`}
            aria-pressed={running}
            onClick={() => onSetMode('fieldday-run')}
          >
            {t('fieldDay.role.running')}
          </button>
          <button
            type="button"
            className={`fd-role-btn${!running ? ' active' : ''}`}
            aria-pressed={!running}
            onClick={() => onSetMode('fieldday-sp')}
          >
            {t('fieldDay.role.sp')}
          </button>
        </div>
        {/* Export buttons */}
        <div className="fd-export">
          {exportError && (
            <span className="log-export-error" role="alert">{exportError}</span>
          )}
          <button
            type="button"
            className="export-btn"
            disabled={busy !== null}
            onClick={() => handleExport('cabrillo')}
            title={t('fieldDay.export.cabrillo.title')}
          >
            {busy === 'cabrillo' ? t('fieldDay.export.busy') : t('fieldDay.export.cabrillo.label')}
          </button>
          <button
            type="button"
            className="export-btn"
            disabled={busy !== null}
            onClick={() => handleExport('adif')}
            title={t('fieldDay.export.adif.title')}
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
        </div>
      </div>

      {/* CLUB SYNC (chip + counters + band board) — only while hosting/joined */}
      {fieldDay?.club && (
        <FdClubSection club={fieldDay.club} onExport={handleExport} busy={busy !== null} />
      )}

      {/* SCOREBOARD (operator + score tiles + sections board) */}
      <FieldDayScoreboard fieldDay={fieldDay} settings={settings} onSaveOperator={saveOperator} />

      {/* BONUSES COLLAPSIBLE */}
      <div className="fd-bonuses-section">
        <button
          type="button"
          className="fd-bonuses-toggle"
          onClick={() => setBonusOpen((v) => !v)}
          aria-expanded={bonusOpen}
        >
          <span>{t('fieldDay.bonuses.head')}</span>
          <span className="fd-bonuses-count">
            {t('fieldDay.bonuses.count', {
              claimed: claimedBonuses.length,
              total: FD_BONUSES.length,
              points: bonusPoints,
            })}
          </span>
          <span className="fd-bonuses-chevron">{bonusOpen ? '▲' : '▼'}</span>
        </button>
        {bonusOpen && (
          <div className="fd-bonuses-list" role="group" aria-label={t('fieldDay.bonuses.aria')}>
            {FD_BONUSES.map((b) => {
              const checked = claimedBonuses.includes(b.id)
              return (
                <label key={b.id} className={`fd-bonus-row${checked ? ' checked' : ''}`}>
                  <input
                    type="checkbox"
                    checked={checked}
                    onChange={() => void toggleBonus(b.id)}
                    aria-label={t('fieldDay.bonus.aria', { label: b.label, points: b.points })}
                  />
                  <span className="fd-bonus-label">{b.label}</span>
                  <span className="fd-bonus-pts">{t('fieldDay.bonus.pts', { points: b.points })}</span>
                </label>
              )
            })}
          </div>
        )}
      </div>

      {/* LOG TABLE */}
      <div className="fd-log">
        <div className="fd-log-head">
          <span className="fd-col time">{t('fieldDay.log.column.time')}</span>
          <span className="fd-col call">{t('fieldDay.log.column.call')}</span>
          <span className="fd-col cls">{classLabel}</span>
          {/* H/I/M/O are the WFD location letters — exchange codes, never translated. */}
          <span className="fd-col sec">{t('fieldDay.log.column.section')}{isWfd && <span className="fd-wfd-hint"> (H/I/M/O)</span>}</span>
          <span className="fd-col band">{t('fieldDay.log.column.band')}</span>
          <span className="fd-col mode">{t('fieldDay.log.column.mode')}</span>
        </div>
        <div className="fd-log-scroll" ref={logPin.ref} onScroll={logPin.onScroll}>
          {rows.length === 0 && <p className="empty">{t('fieldDay.log.empty')}</p>}
          {rows.map((r, i) => (
            <div
              className={`fd-log-row${r.isNewSection ? ' mult' : ''}${r.isDupe ? ' dupe' : ''}`}
              key={`${r.qso.call}-${i}`}
              title={
                r.isDupe
                  ? t('fieldDay.log.dupe.title')
                  : r.isNewSection
                    ? t('fieldDay.log.mult.title')
                    : undefined
              }
            >
              <span className="fd-col time mono">{qsoTimeUtc(r.qso)}</span>
              <span className="fd-col call mono">{r.qso.call}</span>
              <span className="fd-col cls mono">{r.qso.class}</span>
              <span className="fd-col sec mono">
                {r.qso.section}
                {r.isNewSection && <span className="fd-mult-tag">{t('fieldDay.log.mult')}</span>}
              </span>
              <span className="fd-col band">{r.qso.band}</span>
              <span className="fd-col mode">
                {r.qso.mode && (
                  <span className={`fd-mode-chip sm ${(r.qso.mode ?? '').toLowerCase()}`}>
                    {r.qso.mode}
                  </span>
                )}
              </span>
            </div>
          ))}
        </div>
      </div>
    </section>
  )
}
