// Best times: the words of an answer (`get_best_times`), pure so every state is tested without a
// shack. The pane (BestTimesPane) and the card (BestTimesCard) draw the same lines through
// BestTimesAnswer.
//
// The rules the words keep, because a reader acts on them:
//   · NO PERCENTAGES, anywhere, in any language: the model's figures are relative, and a percent reads
//     as the chance of a contact. Only the words Closed, Marginal, Fair, Good and Excellent.
//   · NOTHING FINER THAN THE HOUR: the grid is hourly, so "until about 20:00 UTC".
//   · THE BASIS IS ALWAYS SAID: what the station's location rests on, and "Rough." for a country centre.
//   · NO PROMISE OF A CONTACT: the model describes the path, not whether they are on the air.
//
// ⚠️ ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts): every sentence comes from the catalog.
// Calls, grids, bands, modes, `P.533`, Kp figures, times and distances are data.
import { documentLang, getLocale, t } from '../i18n'
import type { BestMode, BestTimesView, BestWindow, SpotRow, Workability } from '../types'
import { fmtDistanceKm, type Units } from '../units'

export const BEST_MODES: readonly BestMode[] = ['FT8', 'FT4', 'CW', 'SSB']
/** Windows on the card; the pane shows all the shack sends (five at most). */
export const CARD_WINDOWS = 3
/** Below this width the pane stacks the answer above the chart. */
export const SIDE_BY_SIDE_PX = 560

/** The mode an answer starts from: the radio's, when it is one Best times knows, else FT8. */
export function startMode(radioMode: string | null | undefined): BestMode {
  const m = (radioMode ?? '').toUpperCase()
  if (m === 'USB' || m === 'LSB' || m === 'PHONE') return 'SSB'
  return (BEST_MODES as readonly string[]).includes(m) ? (m as BestMode) : 'FT8'
}

// Literal keys in each switch, so the catalog's checks read every one of them.
export function wordText(w: Workability): string {
  switch (w) {
    case 'Closed':
      return t('bestTimes.word.closed')
    case 'Marginal':
      return t('bestTimes.word.marginal')
    case 'Fair':
      return t('bestTimes.word.fair')
    case 'Good':
      return t('bestTimes.word.good')
    case 'Excellent':
      return t('bestTimes.word.excellent')
  }
}

/** The one line a view with no answer says; null when there is an answer. */
export function stateLine(v: BestTimesView): string | null {
  switch (v.state) {
    case 'answer':
      return null
    case 'noGrid':
      return t('bestTimes.state.noGrid')
    case 'enterCall':
      return t('bestTimes.state.enterCall')
    case 'unplaceable':
      return t('bestTimes.state.unplaceable')
    case 'sameEntity':
      return t('bestTimes.state.sameEntity', { entity: v.entity ?? v.call })
    case 'maritime':
      return t('bestTimes.state.maritime')
    case 'you':
      return t('bestTimes.state.you')
  }
}

/** The states that invite a grid typed for the call. */
export function invitesGrid(v: BestTimesView): boolean {
  return v.state === 'unplaceable' || v.state === 'sameEntity' || v.state === 'maritime' || v.basis?.kind === 'entity'
}

/** The states where a callbook lookup could place the station closer. */
export function invitesLookup(v: BestTimesView): boolean {
  return (v.state === 'answer' && v.basis?.kind === 'entity') || v.state === 'sameEntity'
}

/** "Example Island · 9,840 mi · 72°": the entity, the distance and the short-path bearing. */
export function whereLine(v: BestTimesView, units: Units): string | null {
  if (v.km == null || v.bearing == null) return null
  const parts = [fmtDistanceKm(v.km, units), `${Math.round(v.bearing) % 360}°`]
  if (v.entity) parts.unshift(v.entity)
  return parts.join(' · ')
}

/** What the location rests on. Always present with an answer. */
export function basisLine(v: BestTimesView): string | null {
  const b = v.basis
  if (!b) return null
  if (b.kind === 'entity') return t('bestTimes.basis.entity', { entity: v.entity ?? v.call })
  if (b.kind === 'position') return t('bestTimes.basis.position')
  const grid = b.grid ?? ''
  switch (b.from) {
    case 'typed':
      return t('bestTimes.basis.typed', { grid })
    case 'logForm':
      return t('bestTimes.basis.logForm', { grid })
    case 'heard':
      return t('bestTimes.basis.heard', { grid })
    case 'callbook':
      return t('bestTimes.basis.callbook', { grid })
    case 'logged':
      return t('bestTimes.basis.logged', { grid })
    default:
      return t('bestTimes.basis.typed', { grid })
  }
}

/** "20:00": the top of an hour in UTC. */
export function utcHm(unix: number): string {
  const d = new Date(unix * 1000)
  return `${String(d.getUTCHours()).padStart(2, '0')}:00`
}

/** The same instant in the operator's own time, as their system writes a time. */
export function localHm(unix: number): string {
  return new Date(unix * 1000).toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' })
}

const utcDay = (unix: number) => Math.floor(unix / 86_400)

/** "today" or "tomorrow" (UTC days) for a window's start. */
export function dayWord(unix: number, nowUnix: number): string {
  return utcDay(unix) <= utcDay(nowUnix) ? t('bestTimes.day.today') : t('bestTimes.day.tomorrow')
}

/** A UTC day's short weekday name, in the screen's language. */
export function weekdayName(dayUnix: number): string {
  return new Date(dayUnix * 1000).toLocaleDateString(documentLang(getLocale()), { weekday: 'short', timeZone: 'UTC' })
}

/** A UTC month's name, in the screen's language. */
export function monthName(unix: number): string {
  return new Date(unix * 1000).toLocaleDateString(documentLang(getLocale()), { month: 'long', timeZone: 'UTC' })
}

/** The answer for this hour: its headline and the line under it. */
export function nowLines(v: BestTimesView): { head: string; detail: string | null; notes: string[] } | null {
  const a = v.answer
  if (!a) return null
  const n = a.now
  const notes: string[] = []
  if (n.passedOver) notes.push(t('bestTimes.now.passedOver', { band: n.passedOver.band, db: n.passedOver.db }))
  if (n.absorbed) notes.push(t('bestTimes.now.absorbed', { band: n.absorbed.band, db: n.absorbed.db }))
  if (n.kind === 'notNow' || !n.band || !n.word) return { head: t('bestTimes.now.notNow'), detail: null, notes }
  if (n.kind === 'longShot') return { head: t('bestTimes.now.longShot', { band: n.band }), detail: null, notes }
  const word = wordText(n.word)
  let detail = n.allDay
    ? t('bestTimes.now.allDay', { word })
    : n.untilUnix != null
      ? t('bestTimes.now.until', { word, time: utcHm(n.untilUnix) })
      : word
  if (n.runnerUp) detail = `${detail} ${t('bestTimes.now.runnerUp', { band: n.runnerUp.band })}`
  return { head: t('bestTimes.now.try', { band: n.band }), detail, notes }
}

/** One "next best" row's parts. */
export interface WindowRow {
  key: string
  band: string
  /** "today 22:00–23:00 UTC" */
  when: string
  /** "4:00 PM your time" */
  local: string
  word: string
  /** greyline, long shot, long path, a Kp forecast: short tags. */
  tags: string[]
}

export function windowRows(v: BestTimesView, nowUnix: number, cap?: number): WindowRow[] {
  const ws: BestWindow[] = v.answer?.windows ?? []
  return ws.slice(0, cap ?? ws.length).map((w) => {
    const tags: string[] = []
    if (w.longShot) tags.push(t('bestTimes.tag.longShot'))
    if (w.greyline) tags.push(t('bestTimes.tag.greyline'))
    if (w.route === 'long') tags.push(t('bestTimes.tag.longPath'))
    if (w.stormKp != null) tags.push(t('bestTimes.tag.storm', { kp: kpText(w.stormKp) }))
    return {
      key: `${w.band}-${w.fromUnix}`,
      band: w.band,
      when: t('bestTimes.next.when', { day: dayWord(w.fromUnix, nowUnix), from: utcHm(w.fromUnix), to: utcHm(w.toUnix) }),
      local: t('bestTimes.next.local', { time: localHm(w.fromUnix) }),
      word: wordText(w.word),
      tags,
    }
  })
}

function kpText(kp: number): string {
  return Number.isInteger(kp) ? String(kp) : kp.toFixed(1)
}

/** The line under the windows when there are none. */
export function noWindowLine(v: BestTimesView): string | null {
  const a = v.answer
  if (!a || a.windows.length > 0 || a.now.kind === 'try') return null
  return t('bestTimes.next.none', { mode: a.mode })
}

/** "This week": one line, then a line per forecast day that reaches this path. */
export function weekLines(v: BestTimesView): string[] {
  const a = v.answer
  if (!a) return []
  const lines = [t('bestTimes.week.repeat')]
  for (const f of a.week.flags) lines.push(t('bestTimes.week.flag', { day: weekdayName(f.dayUnix), kp: kpText(f.kp) }))
  if (a.week.forecastUntilUnix == null) lines.push(t('bestTimes.week.noForecast'))
  else lines.push(t('bestTimes.week.horizon', { day: weekdayName(a.week.forecastUntilUnix) }))
  return lines
}

/** What the answer rests on besides the location: the model and its month, the mode, the antenna. */
export function modelLine(v: BestTimesView, nowUnix: number, antGainDbi: number | undefined): string | null {
  const a = v.answer
  if (!a) return null
  const parts = [
    a.engine === 'p533'
      ? t('bestTimes.model.p533', { month: monthName(nowUnix) })
      : t('bestTimes.model.heuristic'),
    a.mode,
  ]
  if (!a.perMode) parts.push(t('bestTimes.model.noModes'))
  if (a.engine === 'p533' && !antGainDbi) parts.push(t('bestTimes.model.noGain'))
  return parts.join(' · ')
}

/** "Last spotted 14 min ago on 20 m": the newest Spots-board row for the call, which ranks nothing. */
export function lastSpottedLine(call: string, rows: readonly SpotRow[] | undefined): string | null {
  const key = call.toUpperCase()
  const spot = (rows ?? []).filter((s) => s.call.toUpperCase() === key).sort((a, b) => a.ageSecs - b.ageSecs)[0]
  if (!spot) return null
  const min = Math.max(0, Math.round(spot.ageSecs / 60))
  return spot.band
    ? t('bestTimes.lastSpotted', { min, band: spot.band })
    : t('bestTimes.lastSpotted.noBand', { min })
}

/** A chart cell's hover: the band, the hour, the word, the route; never a figure. */
export function cellTitle(band: string, hour: number, word: Workability, route: 'short' | 'long' | undefined, licensed: boolean): string {
  const parts = [wordText(word)]
  if (route === 'short') parts.push(t('bestTimes.chart.short'))
  if (route === 'long') parts.push(t('bestTimes.chart.long'))
  if (!licensed) parts.push(t('bestTimes.chart.unlicensed'))
  return t('bestTimes.chart.cell', { band, time: `${String(hour).padStart(2, '0')}:00`, text: parts.join(' · ') })
}
