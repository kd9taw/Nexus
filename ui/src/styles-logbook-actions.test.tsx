// @vitest-environment jsdom
import { describe, it, expect, vi, beforeAll } from 'vitest'
import { render, waitFor } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { parseRules, specificity, cmpSpec } from './cssCascade'
import type { LogQuestion } from './features/logAnswers'
import { t } from './i18n'

// ─────────────────────────────────────────────────────────────────────────────────────────
// THE LOGBOOK ROW'S ACTION CLUSTER MUST FIT ITS GRID TRACK.
//
// The defect this exists to make unrepeatable (operator report, 2026-09-16 — "buttons look
// cramped on the screen"): the 160px floor was written 2026-07-15 against the cluster of the
// day — six controls of 22px each, 6×22 + 5×4 = 152. Three more joined (📢, QRZ✎, WRL), ↥
// became the word QRZ, and the floor never moved. `.log-cell` is `overflow:hidden` with
// `justify-content:flex-end`, so the excess is sheared off the LEFT: at 1366–1920 the harness
// measures 70px of the cluster gone — 📢, QRZ and part of QRZ✎, invisible and unclickable —
// with every surviving label also printing across its neighbour.
//
// WHAT THIS FILE CAN AND CANNOT DO. jsdom lays nothing out: every getBoundingClientRect here
// is 0×0, so it cannot measure a label. ui/layout-harness/measure-logbook-actions.sh does
// that, in real Chrome. This guard does the two things jsdom IS good for — it MOUNTS the real
// component and counts what it renders, and it COMPUTES cascade winners out of the real sheet
// — and joins them:
//
//   1. a LOWER BOUND on the cluster's width, derived from the rendered controls and the
//      resolved CSS box model, using a per-character advance chosen well BELOW any real face.
//      It cannot over-estimate, so it cannot produce a false red — and on origin/main it
//      still comes to 236px against that sheet's 160px floor, which is why this file fails
//      there.
//   2. the INPUTS to the harness measurement, pinned: control count, label set, font-size,
//      padding, gap, and the track count against the cells the component actually renders.
//      Change any of them and this goes red pointing at the harness, because the number in
//      the sheet has stopped describing what is on screen. That is exactly the step that was
//      skipped when the glyphs became words.
//
// Never replace any of this with a regex for the floor's text — a dead selector passes that.
// ─────────────────────────────────────────────────────────────────────────────────────────

// Comments are stripped BEFORE parsing: `parseRules` is brace-aware, not comment-aware, and
// these rules carry their rationale INSIDE the body, where a comment's own colon would be read
// as a declaration. (Resolved from the cwd, not `import.meta.url`: under the jsdom environment
// that is an http URL. Same idiom as the other jsdom style guards.)
const CSS = readFileSync(resolve(process.cwd(), 'src', 'styles.css'), 'utf8').replace(
  /\/\*[\s\S]*?\*\//g,
  '',
)
const RULES = parseRules(CSS)

/**
 * The winning declaration of `prop` on an element matched by any of `selectors`, by the real
 * cascade: important first, then specificity, then source order. The caller names the exact
 * selectors that apply to the element it means — which is what keeps a whole-sheet resolver
 * honest without a DOM to match against.
 */
function winner(selectors: string[], prop: string): string | null {
  let best: { value: string; important: boolean; spec: readonly number[]; order: number } | null = null
  for (const rule of RULES) {
    if (!selectors.includes(rule.selector)) continue
    for (const d of rule.decls) {
      if (d.prop !== prop) continue
      const important = /!important/.test(d.value)
      const cand = {
        value: d.value.replace(/\s*!important\s*/, '').trim(),
        important,
        spec: specificity(rule.selector),
        order: rule.order,
      }
      const beats =
        !best ||
        (cand.important && !best.important) ||
        (cand.important === best.important &&
          (cmpSpec(cand.spec, best.spec) > 0 ||
            (cmpSpec(cand.spec, best.spec) === 0 && cand.order > best.order)))
      if (beats) best = cand
    }
  }
  return best?.value ?? null
}

/** The track list the row gets under `selectors`. `minmax(336px, 0.8fr)` stays one track. */
function tracks(selectors: string[]): string[] {
  const v = winner(selectors, 'grid-template-columns')
  expect(v, `no grid-template-columns wins for ${selectors.join(' / ')}`).toBeTruthy()
  const out: string[] = []
  let depth = 0
  let cur = ''
  for (const ch of (v ?? '').replace(/\s+/g, ' ').trim()) {
    if (ch === '(') depth++
    if (ch === ')') depth--
    if (ch === ' ' && depth === 0) {
      if (cur) out.push(cur)
      cur = ''
    } else cur += ch
  }
  if (cur) out.push(cur)
  return out
}

/** The px floor of a `minmax(<px>, …)` track. A bare `fr` track has no floor. */
const floorOf = (track: string): number | null => {
  const m = /^minmax\(\s*(\d+(?:\.\d+)?)px\s*,/.exec(track)
  return m ? Number(m[1]) : null
}

const pxOf = (v: string | null) => (v == null ? 0 : Number(/(-?\d+(?:\.\d+)?)px/.exec(v)?.[1] ?? 0))

/** `--space-1` resolved at --space-scale 1 — md and up, the widest case and the one the floor
 *  has to cover. Narrow viewports tighten it, which only ever makes the cluster smaller. */
const SPACE_1 = 4
const spacing = (v: string | null) => (v && /var\(--space-1\)/.test(v) ? SPACE_1 : pxOf(v))

// ── The harness's numbers, and the inputs they were taken at ─────────────────────────────
// ui/layout-harness/measure-logbook-actions.sh, 2026-09-16, real headless Chrome:
//   requiredInkW 324.6px at --space-scale 1, on the widest of the five sans faces installed
//   (DejaVu Sans; Liberation Sans 322.2, FreeSans 322.2, Bitstream Charter 320.2, Ubuntu
//   313.5 — an 11.1px spread). The sheet's floor is that requirement plus one spread, so a
//   wider face than any of them still fits.
//
// RE-TAKEN 2026-09-22, same harness, when the satellite-tag menu joined the cluster and made
// it ten controls: requiredInkW 372.6px, spread 11.3 (361.3 .. 372.6), floor 336 -> 384. The
// delta is exactly the 44px the sheet pins a `select.log-rowbtn` to plus one 4px gap —
// measured, not assumed, which is the step this file exists to stop anyone skipping.
const MEASURED_REQUIREMENT = 372.6
const MEASURED_FACE_SPREAD = 11.3
/** The controls the row renders, in order, with station control held (the desktop default,
 *  and the widest case). If this changes, the measurement above is stale. */
const EXPECTED_LABELS = ['📢', 'QRZ', 'QRZ✎', 'CL', 'HL', 'WRL', 'QSL▸', 'SAT▸', '✎', '✕']
const EXPECTED_FONT_SIZE = '0.8rem'
/** A per-character advance chosen BELOW every face measured — the narrowest observed was
 *  Ubuntu's `CL` at 7.2px/char at this size. It builds a LOWER bound only, so every face that
 *  is wider than this (all of them) can only make the real requirement larger. */
const MIN_ADVANCE_PX = 6.5

/**
 * A LOWER bound on the width the action cluster needs, in px: every term is read off the
 * cascade or deliberately understated, so it can only under-estimate the truth — which is
 * what makes a floor below it a certain defect rather than a suspected one.
 *
 * The box terms are inputs, not assertions: a sheet that declares less (no padding, a bare
 * `width` where there is now a `min-width`) yields a SMALLER bound, so this stays valid
 * against older sheets — that is how it is run against origin/main.
 */
function clusterLowerBound(): number {
  const box = Math.max(
    pxOf(winner(['.log-rowbtn'], 'min-width')),
    pxOf(winner(['.log-rowbtn'], 'width')),
  )
  const border = pxOf(winner(['.log-rowbtn'], 'border'))
  const padX = spacing((winner(['.log-rowbtn'], 'padding') ?? '').split(/\s+/).pop() ?? null)
  const gap = spacing(winner(['.log-rowactions'], 'gap'))
  // A <select> is as wide as its widest OPTION, so whatever the sheet pins it to is its
  // contribution; unpinned, it is at least a button box.
  const selW = Math.max(box, pxOf(winner(['select.log-rowbtn'], 'width')))
  return (
    EXPECTED_LABELS.reduce((sum, label) => {
      if (label === 'QSL\u25B8' || label === 'SAT\u25B8') return sum + selW
      return sum + Math.max(box, 2 * padX + 2 * border + [...label].length * MIN_ADVANCE_PX)
    }, 0) +
    gap * (EXPECTED_LABELS.length - 1)
  )
}

// ── What the component actually renders ─────────────────────────────────────────────────
beforeAll(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  Object.defineProperty(HTMLElement.prototype, 'offsetHeight', { configurable: true, value: 600 })
  Object.defineProperty(HTMLElement.prototype, 'offsetWidth', { configurable: true, value: 900 })
})

/** The log the engine holds: `askLog` answers from it as the engine does (features/logAnswers.testkit). */
const engineLog = vi.hoisted(() => vi.fn())
vi.mock('./api', () => {
  const noop = () => vi.fn()
  return {
    askLog: vi.fn(async (q: LogQuestion) => (await import('./features/logAnswers.testkit')).answerAs(q, await engineLog())),
    deleteQsoById: noop(), editQsoById: noop(), exportGeneralLog: noop(), importAdif: noop(),
    logOperators: vi.fn(() => Promise.resolve([] as string[])), exportLogForOperator: noop(),
    logActivations: vi.fn(() => Promise.resolve([])), exportLogForActivation: noop(),
    // ⚠️ NON-EMPTY, unlike the sibling Logbook suites: the satellite menu renders only
    // when the backend hands it names, and a cluster measured without it is a cluster
    // no operator sees. The names are LoTW's own (Engine::LOTW_SAT_NAMES).
    lotwSatNames: vi.fn(async () => ['AO-91', 'ARISS', 'SO-50']),
    setSatTagById: vi.fn(async () => ({})),
    saveTextToDownloads: noop(),
    logQso: noop(), markQslSentById: noop(), purgeLog: noop(), qrzLookup: noop(),
    syncLotwReport: noop(), uploadLotwReport: noop(), qrzPushQso: noop(),
    clublogPushQso: noop(), hrdlogPushQso: noop(),
  }
})
vi.mock('./toast', () => ({ pushToast: vi.fn(), withErrorToast: vi.fn() }))

async function renderRow(moreColumns = false): Promise<HTMLElement> {
  // `moreColumns` is read from localStorage on mount (#239's "More columns" toggle), so the
  // wide table is rendered by seeding it rather than by driving the chip.
  window.localStorage.setItem('nexus.logbook.moreColumns', moreColumns ? '1' : '0')
  const { Logbook } = await import('./components/Logbook')
  engineLog.mockResolvedValue([
    {
      call: 'K0ABC', grid: 'EN37', band: '20m', freqMhz: 14.074, mode: 'FT8',
      rstSent: '-10', rstRcvd: '-12', name: null, qth: null, comment: null, notes: null,
      country: 'United States', whenUnix: 1_700_000_000, confirmed: false,
      awardConfirmed: false, qslRcvd: null, qslSent: null, ota: null, upload: undefined,
      // Tagged, so the satellite menu carries its removal entry too — the widest it gets.
      propMode: 'SAT', satName: 'SO-50',
    },
  ])
  const { container } = render(
    <Logbook defaultBand="20m" defaultFreqMhz={14.074} defaultMode="FT8" />,
  )
  await waitFor(() => expect(container.querySelector('.logbook-row:not(.head)')).not.toBeNull())
  // The whole cluster, the satellite menu in it. That menu is the one control a SEPARATE answer
  // draws — the backend's list of names (`lotwSatNames`) — which lands on its own schedule, so
  // the row can be on screen a moment before it. Counted then, the cluster is nine controls, not
  // the ten an operator sees.
  const satMenu = t('logbook.row.sat.aria', { call: 'K0ABC' })
  await waitFor(() =>
    expect(
      container.querySelector(`.logbook-row:not(.head) select[aria-label="${satMenu}"]`),
      'the satellite names never reached the row',
    ).not.toBeNull(),
  )
  return container.querySelector('.logbook-row:not(.head)') as HTMLElement
}

describe('the Logbook row-action cluster fits its grid track', () => {
  it('renders the controls the floor was measured against', async () => {
    const row = await renderRow()
    const cluster = row.querySelector('.log-rowactions')
    expect(cluster, 'the row must render an action cluster, or every check here is vacuous').not.toBeNull()
    const labels = [...cluster!.querySelectorAll('.log-rowbtn')].map((el) =>
      // A <select>'s closed label is its first option, not its whole text content.
      el.tagName === 'SELECT' ? (el as HTMLSelectElement).options[0].text : (el.textContent ?? ''),
    )
    expect(
      labels,
      'the action cluster changed. Its width is a FONT metric jsdom cannot compute — re-run ' +
        'ui/layout-harness/measure-logbook-actions.sh, then update MEASURED_REQUIREMENT, ' +
        'EXPECTED_LABELS and the minmax() floor in .logbook-row.',
    ).toEqual(EXPECTED_LABELS)
  })

  it('gives the tail cell a track of its own at every viewport', async () => {
    const row = await renderRow()
    const cells = row.querySelectorAll(':scope > .log-cell').length
    expect(cells, 'the row must render cells').toBeGreaterThan(5)
    expect(
      tracks(['.logbook-row']).length,
      'the compact template must have one track per rendered cell',
    ).toBe(cells)

    // …and the narrow one, which hides seven columns (six until #239 split Date from Time —
    // the count is computed below, so only this sentence had to move). A template SHORTER
    // than the cell count
    // auto-places the tail cell onto a second GRID ROW — the action cluster rendered beneath
    // the callsign instead of beside it. That is what shipped from 2026-08-23, when the Notes
    // column landed and was added to neither the hide list nor this template.
    // Which columns sm hides is COMPUTED, one cascade query per cell position, so a later
    // rule that unhides one is seen. (:nth-child is 1-based, hence the +1.)
    const hidden = Array.from({ length: cells }, (_, i) => i + 1).filter(
      (n) => winner([`[data-viewport='sm'] .logbook-row .log-cell:nth-child(${n})`], 'display') === 'none',
    ).length
    expect(hidden, 'the sm rules must hide some columns, or this check is vacuous').toBeGreaterThan(0)
    expect(
      tracks(["[data-viewport='sm'] .logbook-row"]).length,
      'the narrow template must have a track for every cell it does not hide',
    ).toBe(cells - hidden)

    // The "More columns" table (#239) is the same shape of trap: seven more cells, its own
    // template, and nothing tying the two together. It shows every column at every viewport,
    // so one count covers it.
    const wideRow = await renderRow(true)
    const wideCells = wideRow.querySelectorAll(':scope > .log-cell').length
    expect(wideCells, 'the wide table must render more cells than the compact one').toBeGreaterThan(cells)
    expect(
      tracks(['.logbook-table.wide .logbook-row']).length,
      'the wide template must have one track per rendered cell',
    ).toBe(wideCells)
  })

  it('floors the tail track above the cluster it has to hold', async () => {
    const row = await renderRow()
    const controls = row.querySelectorAll('.log-rowactions .log-rowbtn').length
    expect(controls).toBe(EXPECTED_LABELS.length)

    // The font the measurement was taken at, and a square floor for the one-glyph buttons.
    expect(winner(['.log-rowbtn'], 'font-size')).toBe(EXPECTED_FONT_SIZE)
    expect(
      Math.max(pxOf(winner(['.log-rowbtn'], 'min-width')), pxOf(winner(['.log-rowbtn'], 'width'))),
      '.log-rowbtn must keep a square floor for its one-glyph buttons',
    ).toBeGreaterThanOrEqual(22)

    const lower = clusterLowerBound()
    expect(lower, 'the bound must be non-trivial, or every floor check below is vacuous').toBeGreaterThan(200)

    const surfaces: [string, string[]][] = [
      ['the compact table', ['.logbook-row']],
      ['the narrow viewport', ["[data-viewport='sm'] .logbook-row"]],
      ['the wide ("more columns") table', ['.logbook-table.wide .logbook-row']],
    ]
    for (const [name, sels] of surfaces) {
      const list = tracks(sels)
      const floor = floorOf(list[list.length - 1])
      expect(floor, `${name}: the tail track must carry a px floor, not a bare fr`).not.toBeNull()
      expect(
        floor as number,
        `${name}: the tail track's floor (${floor}px) is below what ${controls} controls need ` +
          `(at least ${lower.toFixed(1)}px). The cluster gets sheared off the LEFT of an ` +
          'overflow:hidden cell — buttons the operator can neither see nor click.',
      ).toBeGreaterThanOrEqual(lower)
      expect(
        floor as number,
        `${name}: the floor must also cover the MEASURED requirement (${MEASURED_REQUIREMENT}px, ` +
          `ui/layout-harness/measure-logbook-actions.sh) plus the ${MEASURED_FACE_SPREAD}px ` +
          'spread between faces, so a wider face on Windows still fits.',
      ).toBeGreaterThanOrEqual(MEASURED_REQUIREMENT + MEASURED_FACE_SPREAD)
    }

    // …and the QSL menu must be PINNED, or the option catalog sizes the cluster and every
    // floor above becomes a property of the operator's language: a <select> is as wide as its
    // widest OPTION and these options are translated prose (98px in English, 110px in German
    // — harness). `min-width` does not pin it; only an explicit width does.
    expect(
      pxOf(winner(['select.log-rowbtn'], 'width')),
      'select.log-rowbtn must pin an explicit width — otherwise the widest translated option ' +
        'sizes the control, and the cluster with it',
    ).toBeGreaterThan(0)
  })

  it('FIRES: the floor that shipped is reported as too small', () => {
    // The control that must trip, run through the SAME two functions the checks above use —
    // `floorOf` on the track as it shipped, against `clusterLowerBound()`. Without this, a
    // green run says nothing about whether the arithmetic is capable of failing at all.
    const shipped = floorOf('minmax(160px, 0.8fr)')
    expect(shipped, 'the shipped track must parse, or this control is checking nothing').toBe(160)
    expect(shipped as number).toBeLessThan(clusterLowerBound())
    // …and a bare `fr` track — no floor at all, which is what the narrow viewport had — is
    // reported as having none rather than as passing.
    expect(floorOf('0.7fr')).toBeNull()
  })
})
