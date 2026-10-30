// A LOGGED CONTACT'S PATH — the Logbook's Path column, its filter and the contact card's answer
// line, from the shack's grade (src-tauri `path_grades`).
//
// A grade is either stamped on the contact when it was logged (`tier: 'logged'`: from the path card
// computed for that station and the station's own evidence — its beam heading, receivers along the
// route, its decodes) or modelled afterwards for display only (`tier: 'modelled'`: the propagation
// model alone, at the solar flux the contact logged). The two are never confused on screen: a
// modelled grade is dimmed and its hover says it is the model's monthly median, never a
// measurement. Only the station's own beam heading makes a grade "confirmed", and never for the
// long path, whose figures are unvalidated (the hover carries that label whenever the long path is
// involved). The reasons arrive as tokens and are put into words here, so every language reads the
// same evidence.
//
// The Logbook asks for the grades of the rows it draws: asks made together go as one question,
// answers are kept by what the grade depends on (time, band, both grid squares, the fields it
// reads), so an edited contact asks again. A modelled grade or none is asked again after a minute:
// the shack grades the log in the background, and a row graded meanwhile should show it.
import { useEffect, useState } from 'react'
import { logPathGrades, pathGradingStatus } from '../api'
import { t } from '../i18n'
import type { LoggedQso, PathGrade, PathGradeAsk, PathGradingStatus } from '../types'
import { ROUTE_GLYPH } from './pathDay'

/** The passthrough fields a grade reads (src-tauri `path_grades::READS`, plus the K index the
 *  contact card's day reads). Nothing else of a contact is sent. */
export const PATH_GRADE_READS = [
  'SFI',
  'K_INDEX',
  'ANT_AZ',
  'APP_NEXUS_PATH',
  'APP_NEXUS_PATH_CONF',
  'APP_NEXUS_PATH_WHY',
] as const

/** Invariant marks, explained by the column's hover and the strip's legend. */
const EITHER_GLYPH = `${ROUTE_GLYPH.short}${ROUTE_GLYPH.long}`
const OUTSIDE_GLYPH = '?'
export const POLAR_GLYPH = '⊕'

export function gradeAsk(q: LoggedQso): PathGradeAsk {
  return {
    whenUnix: q.whenUnix,
    band: q.band,
    grid: q.grid ?? null,
    myGrid: q.myGrid ?? null,
    extra: (q.extra ?? []).filter(([k]) => (PATH_GRADE_READS as readonly string[]).includes(k)),
  }
}

/** What a row's grade depends on: an edit of any of it asks again. */
export function gradeKey(q: LoggedQso): string {
  const a = gradeAsk(q)
  return JSON.stringify([q.id ?? '', a.whenUnix, a.band, a.grid, a.myGrid, a.extra])
}

function verdictWord(g: PathGrade): string {
  switch (g.verdict) {
    case 'short':
      return t('pathGrade.verdict.short')
    case 'long':
      return t('pathGrade.verdict.long')
    case 'either':
      return t('pathGrade.verdict.either')
    case 'unknown':
      return t('pathGrade.verdict.unknown')
  }
}

function confWord(c: PathGrade['confidence']): string | null {
  switch (c) {
    case 'confirmed':
      return t('pathGrade.conf.confirmed')
    case 'likely':
      return t('pathRoute.conf.likely')
    case 'possible':
      return t('pathRoute.conf.possible')
    default:
      return null
  }
}

/** One reason token in words; null for a token this build does not know. */
export function reasonText(token: string): string | null {
  const [head, arg] = token.split(':', 2)
  switch (head) {
    case 'OPEN_S':
      return t('pathGrade.why.openS')
    case 'OPEN_L':
      return t('pathGrade.why.openL')
    case 'MARGIN_S':
      return t('pathGrade.why.marginS')
    case 'MARGIN_L':
      return t('pathGrade.why.marginL')
    case 'RELIAB_S':
      return t('pathGrade.why.reliabS')
    case 'RELIAB_L':
      return t('pathGrade.why.reliabL')
    case 'WITHIN':
      return t('pathGrade.why.within')
    case 'CLOSED':
      return t('pathGrade.why.closed')
    case 'GREY_S':
      return t('pathGrade.why.greyS')
    case 'GREY_L':
      return t('pathGrade.why.greyL')
    case 'HEADING_S':
      return t('pathGrade.why.headingS')
    case 'HEADING_L':
      return t('pathGrade.why.headingL')
    case 'HEARD_S':
    case 'HEARD_L': {
      const n = Number(arg)
      if (!Number.isInteger(n) || n < 1) return null
      return head === 'HEARD_S' ? t('pathGrade.why.heardS', { n }) : t('pathGrade.why.heardL', { n })
    }
    case 'LATEDT': {
      const m = /^(\d+)\/(\d+)$/.exec(arg ?? '')
      if (!m) return null
      return t('pathGrade.why.lateDt', { ms: Number(m[1]), n: Number(m[2]) })
    }
    default:
      return null
  }
}

/** The answer line: route, how sure, and the polar cap. */
export function gradeSentence(g: PathGrade): string {
  if (g.verdict === 'unknown') return t('pathGrade.sentence.outside')
  if (g.verdict === 'either') return g.polar ? t('pathGrade.sentence.eitherPolar') : t('pathGrade.sentence.either')
  const verdict = verdictWord(g)
  const conf = confWord(g.confidence)
  if (conf) return g.polar ? t('pathGrade.sentence.polar', { verdict, conf }) : t('pathGrade.sentence', { verdict, conf })
  return g.polar ? t('pathGrade.sentence.barePolar', { verdict }) : t('pathGrade.sentence.bare', { verdict })
}

/** The hover's lines: the answer, the reasons, where the grade stands, and the long-path label. */
export function gradeLines(g: PathGrade): string[] {
  const lines = [gradeSentence(g)]
  const why = g.why.map(reasonText).filter((x): x is string => x !== null)
  if (why.length) lines.push(t('pathGrade.why.lead', { text: why.join(t('pathWhy.sep')) }))
  lines.push(g.tier === 'logged' ? t('pathGrade.tier.logged') : t('pathGrade.tier.modelled'))
  if (g.ssnSource === 'logged') lines.push(t('pathGrade.ssn.logged'))
  if (g.ssnSource === 'recent') lines.push(t('pathGrade.ssn.recent'))
  if (g.stampStale) lines.push(t('pathGrade.stale'))
  if (g.verdict === 'long' || g.verdict === 'either') lines.push(t('pathRoute.lpBadge'))
  return lines
}

export interface GradeCellView {
  text: string
  title: string
  /** A grade modelled afterwards, drawn dimmer than one stamped when the contact was logged. */
  dim: boolean
}

/** The route's mark: ▸ short, ◂ long, ▸◂ either, ? outside the model. */
export function gradeGlyph(g: PathGrade): string {
  return g.verdict === 'short' ? ROUTE_GLYPH.short : g.verdict === 'long' ? ROUTE_GLYPH.long : g.verdict === 'either' ? EITHER_GLYPH : OUTSIDE_GLYPH
}

/** What the Path column shows for a row. */
export function gradeCell(g: PathGrade | null | undefined): GradeCellView {
  if (!g) return { text: '—', title: t('pathGrade.none'), dim: false }
  const glyph = gradeGlyph(g)
  const word =
    g.verdict === 'either'
      ? t('pathGrade.cell.either')
      : g.verdict === 'unknown'
        ? t('pathGrade.cell.outside')
        : g.confidence === 'confirmed'
          ? t('pathGrade.cell.confirmed')
          : (confWord(g.confidence) ?? '')
  const text = [glyph, word, g.polar ? POLAR_GLYPH : ''].filter(Boolean).join(' ')
  return { text, title: gradeLines(g).join('\n'), dim: g.tier === 'modelled' }
}

const pct = (n: number, of: number) => (of > 0 ? Math.round((100 * n) / of) : 0)

/** The success measures in words (the expert's hover). */
export function validationLines(s: PathGradingStatus): string[] {
  const lines: string[] = []
  if (s.running || s.cellsDone < s.cellsTotal) lines.push(t('pathGrade.measure.progress', { done: s.cellsDone, total: s.cellsTotal }))
  const v = s.last30Days
  if (!v || v.all.graded === 0) {
    lines.push(t('pathGrade.measure.none'))
  } else {
    lines.push(
      t('pathGrade.measure.coverage', {
        n: v.all.graded,
        open: pct(v.all.open, v.all.graded),
        outside: pct(v.all.outside, v.all.graded),
      }),
    )
    if (v.polar.graded > 0 && v.nonPolar.graded > 0) {
      lines.push(
        t('pathGrade.measure.polar', {
          polar: pct(v.polar.open, v.polar.graded),
          other: pct(v.nonPolar.open, v.nonPolar.graded),
        }),
      )
    }
    lines.push(
      v.headingJudged > 0
        ? t('pathGrade.measure.heading', { pct: pct(v.headingAgree, v.headingJudged), n: v.headingJudged })
        : t('pathGrade.measure.noHeading'),
    )
  }
  if (s.ungradable > 0) lines.push(t('pathGrade.measure.ungradable', { n: s.ungradable }))
  return lines
}

// ── asking ──────────────────────────────────────────────────────────────────────────────────────

/** A modelled grade, or none, is asked again after this long: the shack grades in the background. */
const RECHECK_MS = 60_000
const KEPT = 4000
const kept = new Map<string, { g: PathGrade | null; at: number }>()
let queue: { key: string; ask: PathGradeAsk; done: ((g: PathGrade | null) => void)[] }[] = []
let flushing: Promise<void> | null = null

/** Drop every kept answer (tests). */
export function forgetPathGrades(): void {
  kept.clear()
  queue = []
  flushing = null
}

async function flush(): Promise<void> {
  await Promise.resolve()
  const batch = queue
  queue = []
  flushing = null
  if (batch.length === 0) return
  let answers: (PathGrade | null)[] | null = null
  try {
    answers = await logPathGrades(batch.map((b) => b.ask))
  } catch {
    answers = null
  }
  const now = Date.now()
  batch.forEach((b, i) => {
    const g = answers?.[i] ?? null
    if (answers) {
      kept.set(b.key, { g, at: now })
      if (kept.size > KEPT) kept.delete(kept.keys().next().value as string)
    }
    for (const d of b.done) d(g)
  })
}

/** The grade of a row: kept, or asked together with every other row asked in the same moment. */
export function askPathGrade(q: LoggedQso): Promise<PathGrade | null> {
  const key = gradeKey(q)
  const k = kept.get(key)
  if (k && (k.g?.tier === 'logged' || Date.now() - k.at < RECHECK_MS)) return Promise.resolve(k.g)
  return new Promise((resolve) => {
    const waiting = queue.find((x) => x.key === key)
    if (waiting) waiting.done.push(resolve)
    else queue.push({ key, ask: gradeAsk(q), done: [resolve] })
    if (!flushing) flushing = flush()
  })
}

/** The kept grade for a row, without asking. */
export function keptPathGrade(q: LoggedQso): PathGrade | null | undefined {
  return kept.get(gradeKey(q))?.g
}

/** A row's grade for a component: what is kept at once, then the answer. */
export function usePathGrade(q: LoggedQso): PathGrade | null | undefined {
  const key = gradeKey(q)
  const [g, setG] = useState<PathGrade | null | undefined>(() => keptPathGrade(q))
  useEffect(() => {
    let live = true
    setG(keptPathGrade(q))
    void askPathGrade(q).then((x) => {
      if (live) setG(x)
    })
    return () => {
      live = false
    }
    // The key is everything the grade depends on.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key])
  return g
}

/** How the shack's grading is going, asked while `on` (the Path column is shown) and again every
 *  minute: the success measures for the column's hover. */
export function usePathGradingStatus(on: boolean): PathGradingStatus | null {
  const [s, setS] = useState<PathGradingStatus | null>(null)
  useEffect(() => {
    if (!on) return
    let live = true
    const ask = () =>
      void Promise.resolve()
        .then(() => pathGradingStatus())
        .then((x) => {
          if (live) setS(x)
        })
        .catch(() => {})
    ask()
    const id = setInterval(ask, RECHECK_MS)
    return () => {
      live = false
      clearInterval(id)
    }
  }, [on])
  return s
}
