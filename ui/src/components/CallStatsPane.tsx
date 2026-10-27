// THE STATS PANE — what the log holds for the call typed in a log entry, before the contact is
// logged (the request: "I need the DXCC, WAS and WAZ statistics in the same window as I enter a
// callsign"). It follows the call the cockpit's log entry reports (LogEntry `onEntryCall`, settled)
// and shows, for that station:
//   · the NEW ONE / NEW BAND / NEW MODE flags for the band the cockpit is on and the mode its log
//     entry logs (FT8 and FT4 two modes, as everywhere a new mode is shown);
//   · which channels the log holds a confirmation through — LoTW, card, eQSL, QRZ — for its
//     country, and for this very call;
//   · one grid per award, CW / Phone / Digital × 160 m to 2 m: its DXCC entity, its state (WAS,
//     the United States, Alaska and Hawaii only) and its CQ zone (WAZ). A slot is unworked, worked,
//     or confirmed by what THAT award credits — DXCC and WAS a LoTW or paper QSL, WAZ also an
//     eQSL from an Authenticity-Guaranteed member — and DXCC's 60 m and 4 m slots earn no credit.
//
// COUNTED IN THE STATION, BY THE AWARD FOLD ITSELF (`get_call_stats`): every slot is one the Awards
// view counts, by each contact's own DXCC number and CQ zone. Nothing here counts the log a second
// way. The station typed has no record, so it is placed by its call, as a station heard is.
//
// ONE HEIGHT, WHATEVER IT SAYS. The pane draws the same lines in every state — before a call is
// typed, while it is asked about, for a station in no state or no entity, when the station cannot
// answer — and fills them in: an award the call earns no grid for keeps its rows, marked and
// explained. Measured in Chrome: in RTTY and PSK, where the pane shares its column with the log
// strip's frame, a pane that grew from one line to three grids when a call was typed moved the
// strip's call field up to 134 px on every contact, and back when it was logged; and in FT a
// changing height moved the Stations list under the operator's pointer.
//
// A slot's state is never colour alone: an unworked slot is an outline, a worked one a hollow ring
// in the warning ink, a confirmed one solid in the good ink, and a slot with no credit a dash —
// shapes that read in greyscale — and each carries its words for a screen reader.
//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Band labels, the DXCC
// number, state codes and zone numbers are data, not prose; the award names, the services' names and
// the mode classes' short forms are invariant tokens (INVARIANT, below), as the Awards view's are.
import { useEffect, useState } from 'react'
import { getCallStats } from '../api'
import type { CallStats, QslChannels, StatsGrid, StatsSlot } from '../types'
import { t } from '../i18n'
import { CockpitPaneFrame } from './panes/CockpitPaneFrame'
import type { PanelLayoutApi } from '../features/panelState'
import { useEffectiveHeightAtLeast, useEffectiveWidthAtLeast } from '../useViewport'

/** ON WHERE THERE IS ROOM (the ruling), in Phone, CW and JS8, whose log column it shares with the log
 *  form: a window with room for the pane AND for everything the log form shows there today, its Log
 *  button included, in effective CSS px. The form is taller in a narrower column, so the line is two:
 *  any window at least `minVh` tall, or one at least `wide.minVw` wide and `wide.minVh` tall. Measured in
 *  Chrome (2026-10-15) with a station in all three grids, the pane 196 to 266 px tall: under it the Log
 *  button stands in full at 1200×1430 and not at 1200×1410 (Phone, JS8); at 1600×1300 and every wider
 *  window 1300 tall, and not at 1440×1361 (JS8) or at 1280×1361 (Phone, JS8: a window snapped to half of
 *  a 2560×1440 screen). A maximized window on that screen (about 2560×1361 to 2560×1392) has 60 px and
 *  more to spare; one on a 1920×1080 screen falls 100 px and more short. Below the line it ships hidden
 *  like the other panes a vocabulary ships hidden: one tick in ⊞ Panels. */
export const STATS_ROOM = { minVh: 1440, wide: { minVw: 1600, minVh: 1320 } } as const

/** Whether this window has room for the Stats pane under the log form (`STATS_ROOM`). */
export function useStatsRoom(): boolean {
  const tall = useEffectiveHeightAtLeast(STATS_ROOM.minVh)
  const wide = useEffectiveWidthAtLeast(STATS_ROOM.wide.minVw)
  const wideTall = useEffectiveHeightAtLeast(STATS_ROOM.wide.minVh)
  return tall || (wide && wideTall)
}

/** The cockpit's panel record as its ⊞ host reads it, with the Stats pane's default where the record
 *  says nothing: on where there is room (`room`), hidden elsewhere. A tick or an untick the operator made
 *  is the record's and wins at every size; ⊞ Reset clears it back to this default. Nothing is written. */
export function withStatsRoom<P extends string>(panels: PanelLayoutApi<P>, room: boolean): PanelLayoutApi<P> {
  return {
    ...panels,
    stateOf: (id) => (id === 'stats' && panels.layout.state[id] == null && room ? 'docked' : panels.stateOf(id)),
  }
}

/** The pane as every cockpit draws it under its `stats` id: a content-height frame (a grid cannot
 *  use surplus height), its ✕ the cockpit's own ⊞ tick. */
export function CallStatsFrame({
  close,
  ...pane
}: CallStatsPaneProps & { close: { onRemove?: () => void; hideNote?: string } }) {
  return (
    <CockpitPaneFrame title={t('callStats.pane.title')} paneId="stats" fit="content" {...close}>
      <CallStatsPane {...pane} />
    </CockpitPaneFrame>
  )
}

export interface CallStatsPaneProps {
  /** The call in the cockpit's log entry, settled; empty or null while there is none. */
  call: string | null | undefined
  /** The band the cockpit is on and the mode its log entry logs: what NEW BAND and NEW MODE ask. */
  band: string
  mode: string
  /** The snapshot's log tick, which moves on every change to the log: the pane asks again. */
  logTick?: number
  /** What the pane says with no call: FT's follows a station picked, not a call typed. */
  empty?: string
}

/** One answer, and the question it answers: an answer to an older question is never shown. */
interface Held {
  key: string
  stats?: CallStats
  error?: string
}

export function CallStatsPane({ call, band, mode, logTick, empty }: CallStatsPaneProps) {
  const c = (call ?? '').trim().toUpperCase()
  const key = `${c}|${band}|${mode}`
  const [held, setHeld] = useState<Held | null>(null)
  useEffect(() => {
    if (!c) return
    let live = true
    getCallStats(c, band, mode).then(
      (stats) => live && setHeld({ key, stats }),
      (e: unknown) => live && setHeld({ key, error: e instanceof Error ? e.message : String(e) }),
    )
    return () => {
      live = false
    }
    // `logTick` asks again for the same call: the answer on screen stays until the new one lands.
  }, [c, band, mode, key, logTick])

  const answer = c && held?.key === key ? held : null
  // An answer without its grids' columns is no answer (a stand-in api's `{}` among them).
  const stats = answer?.error == null && Array.isArray(answer?.stats?.bands) ? answer!.stats! : null
  // What the pane says in an answer's place, over the same lines (ONE HEIGHT, above).
  const status = stats
    ? null
    : !c
      ? (empty ?? t('callStats.empty'))
      : !answer
        ? t('callStats.loading')
        : t('callStats.failed', { error: answer.error ?? '' })
  return <CallStatsBody stats={stats} status={status} failed={answer != null && stats == null} band={band} mode={mode} />
}

/** A slot's state in words, for a screen reader and the hover. */
function slotText(slot: StatsSlot): string {
  switch (slot) {
    case 'worked':
      return t('callStats.slot.worked')
    case 'confirmed':
      return t('callStats.slot.confirmed')
    case 'noCredit':
      return t('callStats.slot.noCredit')
    default:
      return t('callStats.slot.unworked')
  }
}

/** Names that are the same in every language: the awards' (programmes, not words — a translated one
 *  names no award anybody can apply for), the confirmation services', and the mode classes' short
 *  forms a logger's grid has always used. */
const INVARIANT = {
  dxcc: 'DXCC',
  was: 'WAS',
  waz: 'WAZ',
  lotw: 'LoTW',
  eqsl: 'eQSL',
  qrz: 'QRZ',
} as const
const MODE_ABBR: Record<string, string> = { CW: 'CW', Phone: 'PH', Digital: 'DIG' }

/** A grid row's mode class in its short form (the station sends CW / Phone / Digital). */
const modeLabel = (m: string) => MODE_ABBR[m] ?? m

/** A band's column head: its number, the metres left off (`160m` → `160`); a centimetre band keeps
 *  its unit (`70cm`), which a bare number would pass off as metres. */
const bandHead = (band: string) => (band.endsWith('cm') ? band : band.replace(/m$/, ''))

/** The grids' columns and rows while there is no answer to draw them from: the award model's bands
 *  and mode classes, as every answer carries them (an answer's own win). */
const BANDS: readonly string[] = ['160m', '80m', '60m', '40m', '30m', '20m', '17m', '15m', '12m', '10m', '6m', '4m', '2m', '70cm', '33cm', '23cm']
const MODES: readonly string[] = ['CW', 'Phone', 'Digital']

/** What an award's head shows in place of an entity, state or zone the call earns no grid for:
 *  the award does not apply to the station (no entity; not in the WAS family), or the station's
 *  place for it is not known (no state on file; no zone). Marks, not words; the words are the
 *  head's title and its screen-reader text. */
const NOT_APPLICABLE = '—'
const NOT_KNOWN = '?'

/** One award's lines: its grid, or why the call has none (`why`, with its head's mark). */
interface Block {
  award: string
  grid: StatsGrid | null
  mark: string
  why: string | null
}

function blocks(stats: CallStats | null): Block[] {
  if (!stats) return [INVARIANT.dxcc, INVARIANT.was, INVARIANT.waz].map((award) => ({ award, grid: null, mark: '', why: null }))
  const call = stats.call
  return [
    { award: INVARIANT.dxcc, grid: stats.dxcc, mark: NOT_APPLICABLE, why: t('callStats.entity.none', { call }) },
    stats.wasFamily
      ? { award: INVARIANT.was, grid: stats.was, mark: NOT_KNOWN, why: t('callStats.was.unknown', { call }) }
      : { award: INVARIANT.was, grid: stats.was, mark: NOT_APPLICABLE, why: t('callStats.was.none', { call }) },
    { award: INVARIANT.waz, grid: stats.waz, mark: NOT_KNOWN, why: t('callStats.waz.unknown', { call }) },
  ].map((b) => (b.grid ? { ...b, why: null } : b))
}

function CallStatsBody({
  stats,
  status,
  failed,
  band,
  mode,
}: {
  stats: CallStats | null
  status: string | null
  failed: boolean
  band: string
  mode: string
}) {
  const bands = stats?.bands ?? BANDS
  const modes = stats?.modes ?? MODES
  const here = bands.findIndex((b) => b.toLowerCase() === band.trim().toLowerCase())
  const who = stats
    ? stats.dxcc
      ? stats.dxcc.code != null
        ? t('callStats.entity.numbered', { entity: stats.dxcc.label, code: String(stats.dxcc.code) })
        : stats.dxcc.label
      : t('callStats.entity.none', { call: stats.call })
    : (status ?? '')
  return (
    <div className="cstats" data-call={stats?.call}>
      {/* The entity (or what the pane has to say instead) and the three flags share one line, the entity
          cut short rather than wrapped: every line here is height the log form beside it gives up. */}
      <div className="cstats-top">
        <p className={stats ? 'cstats-who' : 'cstats-who cstats-empty'} title={who} role={failed ? 'status' : undefined}>
          {who}
        </p>
        <ul className="cstats-flags" aria-label={t('callStats.flags.label')} data-idle={stats ? undefined : true} aria-hidden={stats ? undefined : true}>
          <Flag kind="one" on={!!stats?.newOne} label={t('callStats.flag.newOne')} why={t('callStats.flag.newOne.title')} />
          <Flag kind="band" on={!!stats?.newBand} label={t('callStats.flag.newBand')} why={t('callStats.flag.newBand.title', { band })} />
          <Flag kind="mode" on={!!stats?.newMode} label={t('callStats.flag.newMode')} why={t('callStats.flag.newMode.title', { mode })} />
        </ul>
      </div>
      <table className="cstats-src">
        <caption className="sr-only">{t('callStats.src.caption')}</caption>
        <thead>
          <tr>
            <td />
            <th scope="col">{INVARIANT.lotw}</th>
            <th scope="col">{t('callStats.src.card')}</th>
            <th scope="col">{INVARIANT.eqsl}</th>
            <th scope="col">{INVARIANT.qrz}</th>
          </tr>
        </thead>
        <tbody>
          <SourceRow head={t('callStats.src.country')} qsl={stats?.countryQsl ?? null} />
          <SourceRow head={t('callStats.src.call')} qsl={stats?.callQsl ?? null} />
        </tbody>
      </table>
      <table className="cstats-grid" title={t('callStats.grid.credit')}>
        {stats && <caption className="sr-only">{t('callStats.grid.caption', { call: stats.call })}</caption>}
        <thead>
          <tr>
            <td colSpan={2} />
            {bands.map((b, i) => (
              <th key={b} scope="col" data-here={i === here || undefined}>
                {bandHead(b)}
              </th>
            ))}
          </tr>
        </thead>
        {blocks(stats).map(({ award, grid, mark, why }) => (
          <tbody key={award} className="cstats-award">
            {modes.map((m, r) => (
              <tr key={m}>
                {r === 0 && (
                  <th scope="rowgroup" rowSpan={modes.length} className="cstats-award-head" title={why ?? undefined}>
                    {award}
                    <span className="cstats-award-of">{grid ? (grid.code != null ? String(grid.code) : grid.label) : mark}</span>
                    {why && <span className="sr-only">{why}</span>}
                  </th>
                )}
                <th scope="row" className="cstats-mode">
                  {modeLabel(m)}
                </th>
                {bands.map((b, i) => {
                  const slot = grid?.rows[r]?.[i]
                  return (
                    <td
                      key={b}
                      className="cstats-slot"
                      data-slot={slot ?? 'none'}
                      data-here={i === here || undefined}
                      title={slot ? t('callStats.slot.title', { award, band: b, mode: modeLabel(m), state: slotText(slot) }) : undefined}
                    >
                      <span className="cstats-mark" aria-hidden="true" />
                      {slot && <span className="sr-only">{slotText(slot)}</span>}
                    </td>
                  )
                })}
              </tr>
            ))}
          </tbody>
        ))}
      </table>
    </div>
  )
}

function Flag({ kind, on, label, why }: { kind: 'one' | 'band' | 'mode'; on: boolean; label: string; why: string }) {
  return (
    <li className="cstats-flag" data-kind={kind} data-on={on} title={on ? why : undefined}>
      {label}
      {!on && <span className="sr-only"> {t('callStats.flag.no')}</span>}
    </li>
  )
}

/** One row of the sources: what the log holds per service, or nothing to say (`qsl` null — no answer
 *  yet, or a call that names no country), its cells kept and left empty. */
function SourceRow({ head, qsl }: { head: string; qsl: QslChannels | null }) {
  const cell = (held: boolean | undefined, ag = false) => (
    <td className="cstats-src-cell" data-held={held ?? 'none'} data-ag={ag || undefined} title={ag ? t('callStats.src.ag') : undefined}>
      <span className="cstats-dot" aria-hidden="true" />
      {held != null && (
        <span className="sr-only">{!held ? t('callStats.src.no') : ag ? t('callStats.src.ag') : t('callStats.src.yes')}</span>
      )}
    </td>
  )
  return (
    <tr>
      <th scope="row">{head}</th>
      {cell(qsl?.lotw)}
      {cell(qsl?.card)}
      {cell(qsl?.eqsl, !!qsl && qsl.eqsl && qsl.eqslAg)}
      {cell(qsl?.qrz)}
    </tr>
  )
}
