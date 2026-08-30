// @vitest-environment jsdom
import { describe, it, expect, beforeEach } from 'vitest'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'

// The Field Day spectator scoreboard page, executed for real: the page keeps
// all rendering in pure render(data, meta) functions behind the __fdboard test
// seam (no network, no timers under __FDBOARD_TEST__), so jsdom can drive the
// exact script the TV runs. Server-side truth (scoring math, dedupe, payload
// shape) is pinned in Rust — crates/tempo-app/src/fd_scoreboard.rs; this file
// covers what the PAGE does with a payload: the 83-cell section board, ticker
// attribution, the WFD no-power-math rendering, and the stale badge.
// (jsdom never lays out — visual/layout checks are the manual puppeteer pass.)

const html = readFileSync(
  resolve(process.cwd(), '../crates/tempo-app/assets/fd_scoreboard.html'),
  'utf8',
)
const body = /<body>([\s\S]*)<\/body>/.exec(html)?.[1]
const script = /<script>([\s\S]*?)<\/script>/.exec(html)?.[1]

type Board = {
  STRINGS: Record<string, string>
  render: (data: unknown, meta: unknown) => void
  renderMeta: (meta: unknown) => void
  setStale: (on: boolean) => void
  setInactive: (on: boolean) => void
  tickClock: () => void
}

declare global {
  interface Window {
    __FDBOARD_TEST__?: boolean
    __fdboard?: Board
  }
}

function boot(): Board {
  window.__FDBOARD_TEST__ = true
  document.body.innerHTML = body!
  new Function(script!)()
  return window.__fdboard!
}

// A meta fixture shaped like /scoreboard/meta.json: a full 83-code section
// universe (the page must build one chip per code, grouped by division).
const DIVISIONS = ['Atlantic', 'Central', 'New England', 'Pacific', 'RAC']
function makeMeta(scoringModel: 'powered' | 'objectives') {
  const sections = []
  for (let i = 0; i < 83; i++) {
    sections.push({
      code: `S${String(i).padStart(2, '0')}`,
      name: `Section ${i}`,
      division: DIVISIONS[i % DIVISIONS.length],
    })
  }
  return {
    event: {
      kind: scoringModel === 'objectives' ? 'wfd' : 'arrlfd',
      name: scoringModel === 'objectives' ? 'Winter Field Day' : 'ARRL Field Day',
      year: 2026,
      start_unix: 1_750_000_000,
      end_unix: 1_750_097_200,
      call: 'W9ABC',
      class: '3A',
      section: 'WI',
    },
    scoring_model: scoringModel,
    rules_year: 2026,
    rules_generated: '2026-08-29T00:00:00Z',
    sections,
    bonuses: [
      { id: 'emergency-power', label: 'Emergency power', points: 100 },
      { id: 'safety-officer', label: 'Safety officer', points: 100 },
      { id: 'web-submission', label: 'Web submission', points: 50 },
    ],
  }
}

function makeData(model: 'powered' | 'objectives') {
  const meta = makeMeta(model)
  const score =
    model === 'powered'
      ? {
          model: 'powered',
          qso_points: 6,
          bonus_points: 150,
          power_mult: 2,
          powered_points: 12,
          total: 162,
        }
      : {
          model: 'objectives',
          qso_points: 6,
          bonus_points: 150,
          total: 6,
          projected_at_submission: 18,
          objectives_claimed: 2,
        }
  return {
    rev: 7,
    now_unix: 1_750_007_500,
    event: meta.event,
    score,
    qsos: { count: 4, rate_hour: 12, rate_10min: 3, hourly: [2, 2, 0] },
    ticker: [
      {
        when_unix: 1_750_007_000,
        call: 'VE3AAA',
        class: '3A',
        section: 'S05',
        band: '20m',
        mode: 'DIG',
        submode: 'FT8',
        position: 'CW tent',
        operator: 'W9AAA',
      },
      {
        when_unix: 1_750_004_000,
        call: 'K1ABC',
        class: '2A',
        section: 'S01',
        band: '20m',
        mode: 'PH',
        submode: '',
        position: 'Phone tent',
        operator: 'W9BBB',
      },
    ],
    band_mode: [
      { band: '40m', ph: 1, cw: 0, dig: 0 },
      { band: '20m', ph: 1, cw: 1, dig: 1 },
    ],
    sections_worked: ['S01', 'S05'],
    positions: [
      {
        id: 'aaaa1111',
        label: 'CW tent',
        operator: 'W9AAA',
        qsos_raw: 2,
        qsos_unique: 2,
        points: 4,
        last_qso_unix: 1_750_007_000,
      },
      {
        id: 'bbbb2222',
        label: 'Phone tent',
        operator: 'W9BBB',
        qsos_raw: 3,
        qsos_unique: 2,
        points: 2,
        last_qso_unix: 1_750_004_000,
      },
    ],
    claimed: ['emergency-power', 'web-submission'],
  }
}

beforeEach(() => {
  // A fresh DOM + a fresh script run per test (the script keeps module state:
  // metaDone, lastHeroKey).
  document.body.innerHTML = ''
})

describe('fd scoreboard page', () => {
  it('exposes the test seam with pure render functions and the locale seam', () => {
    const b = boot()
    expect(typeof b.render).toBe('function')
    expect(typeof b.renderMeta).toBe('function')
    // Every page string lives in ONE object — the later-locale injection seam.
    expect(Object.keys(b.STRINGS).length).toBeGreaterThanOrEqual(30)
  })

  it('builds one chip per section — all 83 — grouped under division labels', () => {
    const b = boot()
    b.renderMeta(makeMeta('powered'))
    const chips = document.querySelectorAll('#sec-board .sec')
    expect(chips.length).toBe(83)
    const labels = [...document.querySelectorAll('#sec-board .div-label')].map(
      (e) => e.textContent,
    )
    expect(labels).toEqual(DIVISIONS)
    // Nothing is lit before data arrives.
    expect(document.querySelectorAll('#sec-board .sec.lit').length).toBe(0)
  })

  it('lights exactly the worked sections and shows the n-of-83 count', () => {
    const b = boot()
    const meta = makeMeta('powered')
    b.render(makeData('powered'), meta)
    const lit = [...document.querySelectorAll('#sec-board .sec.lit')].map(
      (e) => e.textContent,
    )
    expect(lit.sort()).toEqual(['S01', 'S05'])
    expect(document.getElementById('sec-count')!.textContent).toContain('2')
    expect(document.getElementById('sec-count')!.textContent).toContain('83')
  })

  it('renders the newest QSO as the hero with position + operator attribution', () => {
    const b = boot()
    b.render(makeData('powered'), makeMeta('powered'))
    const hero = document.getElementById('hero')!.textContent!
    expect(hero).toContain('VE3AAA')
    expect(hero).toContain('Section 5') // section NAME resolved from meta
    expect(hero).toContain('20m FT8') // submode wins over the mode class
    expect(hero).toContain('CW tent')
    expect(hero).toContain('W9AAA')
    // The compact recent line carries the rest of the ticker.
    expect(document.getElementById('recent')!.textContent).toContain('K1ABC')
  })

  it('SFD control: the powered score block DOES show the power multiplier', () => {
    const b = boot()
    b.render(makeData('powered'), makeMeta('powered'))
    const block = document.getElementById('score-lines')!.textContent!
    expect(document.getElementById('score-total')!.textContent).toBe('162')
    expect(block).toContain(b.STRINGS.powerLine)
    expect(block).toContain('× 2')
  })

  it('WFD: raw headline + labelled projection, and NO power-multiplier text anywhere', () => {
    const b = boot()
    b.render(makeData('objectives'), makeMeta('objectives'))
    // Headline is the RAW qso points, captioned as provisional.
    expect(document.getElementById('score-total')!.textContent).toBe('6')
    expect(document.getElementById('score-caption')!.textContent).toBe(
      b.STRINGS.rawPoints,
    )
    const block = document.getElementById('score-lines')!.textContent!
    expect(block).toContain('6 × 3 → 18') // the ×(n+1) projection, labelled
    expect(block).toContain(b.STRINGS.atSubmission)
    // The no-power pin (the SFD test above is the positive control that this
    // text DOES appear when the model has a power multiplier).
    const leftPanel = document.getElementById('col-left')!.textContent!
    expect(leftPanel).not.toContain(b.STRINGS.powerLine)
    expect(leftPanel).not.toContain(b.STRINGS.poweredLine)
  })

  it('ticks claimed bonuses and leaves the rest unticked', () => {
    const b = boot()
    b.render(makeData('powered'), makeMeta('powered'))
    const claimed = [...document.querySelectorAll('#bonus-list .bonus.claimed')].map(
      (e) => e.getAttribute('data-id'),
    )
    expect(claimed.sort()).toEqual(['emergency-power', 'web-submission'])
    const unticked = document.querySelector('.bonus[data-id="safety-officer"]')!
    expect(unticked.classList.contains('claimed')).toBe(false)
    expect(unticked.querySelector('.tick')!.textContent).toBe('☐')
  })

  it('renders positions as a leaderboard with raw QSO counts and points', () => {
    const b = boot()
    b.render(makeData('powered'), makeMeta('powered'))
    const rows = [...document.querySelectorAll('#pos-list .pos-row')]
    expect(rows.length).toBe(2)
    expect(rows[0].textContent).toContain('CW tent')
    expect(rows[0].textContent).toContain('W9AAA')
    expect(rows[0].textContent).toContain('2') // raw QSOs (board shows raw)
    expect(rows[0].textContent).toContain('4') // points
  })

  it('shows the stale badge on demand and clears it on recovery', () => {
    const b = boot()
    b.renderMeta(makeMeta('powered'))
    const badge = document.getElementById('stale')!
    expect(badge.classList.contains('on')).toBe(false)
    b.setStale(true)
    expect(badge.classList.contains('on')).toBe(true)
    expect(badge.textContent).toBe(b.STRINGS.staleBadge)
    b.setStale(false)
    expect(badge.classList.contains('on')).toBe(false)
  })

  it('shows the host-only overlay for a non-host instance', () => {
    const b = boot()
    b.setInactive(true)
    const overlay = document.getElementById('inactive')!
    expect(overlay.classList.contains('on')).toBe(true)
    expect(overlay.textContent).toBe(b.STRINGS.hostOnlyMsg)
  })

  it('treats payload values as text, never markup (calls arrive over RF)', () => {
    const b = boot()
    const data = makeData('powered')
    data.ticker[0].call = '<img src=x onerror=alert(1)>'
    b.render(data, makeMeta('powered'))
    expect(document.querySelector('#hero img')).toBeNull()
    expect(document.getElementById('hero')!.textContent).toContain('<img')
  })
})
