// THE FIELD DAY BAND × MODE GRID — how many unique contacts each cell holds, and, while a
// callsign is being typed, which cells that station is already in.
//
// Two jobs, and the second is the one that earns the pane. The counts answer "where has this
// position been?"; the DRAFT PAINT answers the question a run operator actually asks at 2 AM —
// "he's a dupe here, but is 15m PH open?" — as a glance rather than a read. The paint is
// computed from data already in hand (the whole own log and the club-only dupe keys ride every
// 300 ms snapshot), so a keystroke costs two Set lookups per rendered cell and zero IPC.
//
// ⚠️ THE DUPE KEY IS A CROSS-LANGUAGE CONTRACT. `fdDupeKey` below must equal the key Rust
// builds, or this grid paints the wrong cells — see its own comment and
// FdBandModeGrid.test.tsx, which reads the Rust sources and checks the three normalisations
// one by one.
//
// WHICH ROWS APPEAR. Bands with contacts (own or club), plus the band the rig is on — ordered
// by `BAND_ORDER`, the same HF→VHF order the FD summary export prints. NOT all seventeen: a
// row for 23 cm that will never be worked is noise on the one screen that must stay readable
// in the dark, and it buys nothing — "15m PH is open" is only ever a useful reading of a band
// this position has actually operated.
//
// The cells are INERT. Click-to-QSY was refused for v1: retuning the rig from a board is
// band-stacking/CAT territory (reference-band-stacking-mode-window), and a mis-click during a
// run would move the radio out from under the operator.
//
// Styling is inline off the shared tokens, the SectionsBoard idiom in FieldDayView.tsx — the
// pane's SIZE is the cockpit grid's business (cockpit-panes.css), so nothing here declares one.
//
// ⚠️ ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Band names, the mode-class codes in
// `FD_MODE_CLASSES` and every count are invariant technical tokens and stay in the code; the
// prose around them is catalog keys.
import { useMemo, type CSSProperties } from 'react'
import type { FieldDayQso } from '../types'
import { t } from '../i18n'

/**
 * ⚠️ INVARIANT — the three FD scoring classes, as they are logged, scored and exported.
 * Mirrors `FD_MODE_CODES` in FieldDayView.tsx and the Rust mode class. Never translated, and
 * a named constant rather than a JSX literal so the column headers are the codes an entry is
 * submitted with.
 */
export const FD_MODE_CLASSES = ['CW', 'PH', 'DIG'] as const
export type FdModeClass = (typeof FD_MODE_CLASSES)[number]

/**
 * Canonical band order, HF → VHF, so the rows read top-down like a rig.
 *
 * ⚠️ MIRRORS `BAND_ORDER` in FieldDayView.tsx (the summary export's order). Two lists, one
 * order — checked against each other in FdBandModeGrid.test.tsx rather than exported across,
 * because FieldDayView is a hot shared file this landing touches with exactly one word.
 */
export const FD_BAND_ORDER = [
  '160m', '80m', '60m', '40m', '30m', '20m', '17m', '15m', '12m', '10m',
  '6m', '4m', '2m', '1.25m', '70cm', '33cm', '23cm',
]

/**
 * THE DUPE KEY — `(CALL, band, MODE CLASS)`, and it must equal Rust's byte for byte.
 *
 * Rust builds it in exactly two places and both apply the same three normalisations:
 *
 *   own log   `FieldDayLog::log_submode_at` (tempo-core/src/fieldday.rs)
 *             `self.worked.insert((call.to_uppercase(), self.band.clone(), mode.clone()))`
 *             where `mode` was already `mode.to_ascii_uppercase()`
 *   club log  `MergedRow::dupe_key` (tempo-app/src/fdevent.rs)
 *             `(self.call.to_uppercase(), self.band.clone(), self.mode_class.to_ascii_uppercase())`
 *
 * So: THE CALL IS UPPERCASED, THE MODE CLASS IS UPPERCASED, AND THE BAND IS VERBATIM. The band
 * is deliberately NOT normalised — Rust stamps the log's own band string on both sides, and a
 * TS-side `toUpperCase()` here would make `20M` and `20m` the same cell in the UI and two
 * different keys in the engine, which is the shape of a dupe check that silently disagrees
 * with the commit that follows it.
 *
 * The `trim()` is the UI's own, and it changes nothing Rust produced: it normalises a DRAFT
 * field the operator is still typing into, exactly as LogEntry's verdict does
 * (`logCall.trim().toUpperCase()`), and every call that reached the engine was trimmed by that
 * same call site before `fdLogManual` saw it.
 */
export function fdDupeKey(call: string, band: string, modeClass: string): string {
  return `${call.trim().toUpperCase()}|${band}|${modeClass.trim().toUpperCase()}`
}

/** What the growth-keyed pass leaves behind: the key sets, the per-cell counts, the bands. */
interface GridData {
  /** Every `(call, band, mode)` this position has worked. */
  ownKeys: Set<string>
  /** Club keys NOT in the own log (engine.rs subtracts own before it ships them). */
  clubKeys: Set<string>
  /** `band|mode` → unique contacts from this position. */
  own: Map<string, number>
  /** `band|mode` → unique contacts across the merged club event (own ∪ club-only). */
  total: Map<string, number>
  /** Bands with any activity at all, unordered. */
  active: Set<string>
}

const bump = (m: Map<string, number>, cell: string) => m.set(cell, (m.get(cell) ?? 0) + 1)

/**
 * Bucket the log and the club keys into per-cell UNIQUE counts.
 *
 * Unique, not raw: the grid says how many stations are worked in a cell, which is what the
 * cell is worth. `fd_log_manual` refuses a dupe, so the own log is normally already unique —
 * an ADIF restore or a merged event is where a repeat can appear, and counting keys rather
 * than rows means the number never quietly inflates.
 */
function buildGrid(log: FieldDayQso[], clubDupes: readonly (readonly [string, string, string])[]): GridData {
  const ownKeys = new Set<string>()
  const clubKeys = new Set<string>()
  const own = new Map<string, number>()
  const total = new Map<string, number>()
  const active = new Set<string>()
  for (const q of log) {
    const mode = (q.mode ?? '').trim().toUpperCase()
    if (!q.band || !mode) continue
    const key = fdDupeKey(q.call, q.band, mode)
    if (ownKeys.has(key)) continue
    ownKeys.add(key)
    const cell = `${q.band}|${mode}`
    bump(own, cell)
    bump(total, cell)
    active.add(q.band)
  }
  for (const [call, band, mode] of clubDupes) {
    const m = (mode ?? '').trim().toUpperCase()
    if (!band || !m) continue
    const key = fdDupeKey(call, band, m)
    // Own keys are already subtracted host-side; the guard makes the union honest anyway.
    if (ownKeys.has(key) || clubKeys.has(key)) continue
    clubKeys.add(key)
    bump(total, `${band}|${m}`)
    active.add(band)
  }
  return { ownKeys, clubKeys, own, total, active }
}

/** The bands to draw, in canonical order; an unrecognised band sorts to the end, by name. */
function orderBands(active: Set<string>, band: string): string[] {
  const set = new Set(active)
  if (band) set.add(band)
  const rank = (b: string) => {
    const i = FD_BAND_ORDER.indexOf(b)
    return i === -1 ? FD_BAND_ORDER.length : i
  }
  return [...set].sort((a, b) => rank(a) - rank(b) || a.localeCompare(b))
}

// ── styling (inline off the shared tokens — every one exists in BOTH themes) ───────────

const WRAP: CSSProperties = { display: 'flex', flexDirection: 'column', gap: 6, minWidth: 0 }
const TABLE: CSSProperties = {
  borderCollapse: 'separate',
  borderSpacing: 3,
  fontFamily: 'var(--font-mono)',
  fontVariantNumeric: 'tabular-nums',
}
const COL_HEAD: CSSProperties = {
  padding: '2px 6px',
  fontSize: 10,
  fontWeight: 700,
  letterSpacing: '0.06em',
  color: 'var(--text-faint)',
  textAlign: 'center',
}
const ROW_HEAD: CSSProperties = {
  padding: '2px 8px 2px 2px',
  fontSize: 13,
  fontWeight: 700,
  color: 'var(--text-dim)',
  textAlign: 'right',
  whiteSpace: 'nowrap',
}
const CELL_BASE: CSSProperties = {
  padding: '3px 9px',
  minWidth: 52,
  borderRadius: 'var(--radius-sm)',
  fontSize: 15,
  lineHeight: 1.3,
  textAlign: 'center',
  background: 'var(--bg-elev)',
  border: '1px solid var(--border-soft)',
  whiteSpace: 'nowrap',
}
/** Per-state ink. Own dupe is the hard one (the commit will refuse), club is the warning. */
function cellStyle(worked: boolean, ownDupe: boolean, clubDupe: boolean, here: boolean): CSSProperties {
  const s: CSSProperties = {
    ...CELL_BASE,
    fontWeight: worked ? 700 : 500,
    color: worked ? 'var(--status-confirmed)' : 'var(--text-faint)',
    opacity: worked ? 1 : 0.65,
  }
  if (here) s.boxShadow = 'inset 0 0 0 1px var(--accent)'
  if (ownDupe) {
    s.color = 'var(--alert-critical)'
    s.background = 'color-mix(in srgb, var(--alert-critical) 16%, transparent)'
    s.border = '1px solid var(--alert-critical)'
    s.opacity = 1
  } else if (clubDupe) {
    s.color = 'var(--status-new-band)'
    s.background = 'color-mix(in srgb, var(--status-new-band) 16%, transparent)'
    s.border = '1px solid var(--status-new-band)'
    s.opacity = 1
  }
  return s
}
const SUB: CSSProperties = { fontSize: 11, fontWeight: 600, color: 'var(--text-dim)' }
const HERE: CSSProperties = { marginLeft: 3, fontSize: 11, color: 'var(--accent)' }
const LEGEND: CSSProperties = {
  display: 'flex',
  flexWrap: 'wrap',
  gap: 10,
  fontSize: 11,
  color: 'var(--text-faint)',
}
const EMPTY: CSSProperties = { fontSize: 13, color: 'var(--text-faint)' }
const swatch = (ink: string): CSSProperties => ({
  display: 'inline-block',
  width: 8,
  height: 8,
  marginRight: 4,
  borderRadius: 2,
  background: ink,
})

export function FdBandModeGrid({
  log,
  clubDupes,
  band,
  modeClass,
  draftCall = '',
}: {
  /** `FieldDayStatus.log` — this position's contacts. */
  log: FieldDayQso[]
  /** `FieldDayStatus.club.dupes` — club keys with the own log already subtracted. Absent on a
   *  solo Field Day, which renders an own-only grid and is still correct. */
  clubDupes?: [string, string, string][]
  /** The band the rig is on — the ◀ marker's row, and always a visible row. */
  band: string
  /** The position's mode class — the ◀ marker's column. */
  modeClass: FdModeClass
  /** The callsign in the entry field right now. Paints the cells it is already worked in. */
  draftCall?: string
}) {
  const club = clubDupes ?? []
  // GROWTH-KEYED. The heavy pass walks the whole log and every club key; it is re-run when
  // either LENGTH changes, not on every 300 ms snapshot — a fresh array with identical
  // contents arrives on each poll, and bucketing thousands of rows sixty times a minute is
  // CPU spent to produce the same Map. The draft paint below is deliberately OUTSIDE it, so a
  // keystroke repaints without re-bucketing anything.
  // eslint-disable-next-line react-hooks/exhaustive-deps
  const grid = useMemo(() => buildGrid(log, club), [log.length, club.length])
  const bands = useMemo(() => orderBands(grid.active, band), [grid, band])
  const showClub = club.length > 0
  const draft = draftCall.trim().toUpperCase()

  if (bands.length === 0) {
    return <div style={EMPTY}>{t('fieldDay.cockpit.grid.empty')}</div>
  }

  return (
    <div style={WRAP} className="fd-bmgrid">
      <table style={TABLE} aria-label={t('fieldDay.cockpit.grid.aria')}>
        <thead>
          <tr>
            <th style={COL_HEAD} />
            {FD_MODE_CLASSES.map((m) => (
              <th key={m} style={COL_HEAD} scope="col">
                {m}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {bands.map((b) => (
            <tr key={b}>
              <th style={ROW_HEAD} scope="row">
                {b}
              </th>
              {FD_MODE_CLASSES.map((m) => {
                const cell = `${b}|${m}`
                const own = grid.own.get(cell) ?? 0
                const total = grid.total.get(cell) ?? 0
                const here = b === band && m === modeClass
                const key = draft ? fdDupeKey(draft, b, m) : ''
                const ownDupe = draft !== '' && grid.ownKeys.has(key)
                const clubDupe = draft !== '' && !ownDupe && grid.clubKeys.has(key)
                const label = ownDupe
                  ? t('fieldDay.cockpit.grid.dupe.own', { call: draft, band: b, mode: m })
                  : clubDupe
                    ? t('fieldDay.cockpit.grid.dupe.club', { call: draft, band: b, mode: m })
                    : showClub
                      ? t('fieldDay.cockpit.grid.cell.club', { band: b, mode: m, own, total })
                      : t('fieldDay.cockpit.grid.cell', { band: b, mode: m, own })
                return (
                  <td
                    key={m}
                    // A stable identifier for tests, NOT a styling hook — the same reason
                    // CockpitPaneFrame carries `data-pane` as an attribute rather than a class.
                    data-cell={cell}
                    style={cellStyle(own > 0, ownDupe, clubDupe, here)}
                    title={label}
                    aria-label={label}
                  >
                    {own > 0 ? own : '—'}
                    {showClub && total > own && <sub style={SUB}>{total}</sub>}
                    {here && (
                      <span style={HERE} aria-hidden="true">
                        ◀
                      </span>
                    )}
                  </td>
                )
              })}
            </tr>
          ))}
        </tbody>
      </table>
      <div style={LEGEND}>
        <span>
          <span style={{ ...swatch('var(--accent)'), borderRadius: 8 }} aria-hidden="true" />
          {t('fieldDay.cockpit.grid.legend.here')}
        </span>
        {showClub && <span>{t('fieldDay.cockpit.grid.legend.club')}</span>}
        {draft !== '' && (
          <>
            <span>
              <span style={swatch('var(--alert-critical)')} aria-hidden="true" />
              {t('fieldDay.cockpit.grid.legend.own')}
            </span>
            {showClub && (
              <span>
                <span style={swatch('var(--status-new-band)')} aria-hidden="true" />
                {t('fieldDay.cockpit.grid.legend.clubDupe')}
              </span>
            )}
          </>
        )}
      </div>
    </div>
  )
}
