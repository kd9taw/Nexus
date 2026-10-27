// @vitest-environment jsdom
//
// THE CONTEST SCREEN'S LOWER SECTION USES THE WHOLE WINDOW (operator, 2026-10-12), so what its old
// 1600px cap was bounding needs a bound of its own where the panel can now outgrow it: Settings'
// prose audit, here. Two of those are styled inline in this component rather than in styles.css —
// the club section's sentences and the scoreboard's operator box — so they read their measures from
// two custom properties the panel sets on lg and xl (fdDashboardShell.test.tsx computes the panel's
// side per tier). This file renders the real screen and resolves both through the real sheet: a
// sentence of the club's at 90ch, the box at 520px on lg/xl, and nothing new on xs/sm/md, where the
// panel is never wider than it was.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, act, cleanup } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { ContestView } from './ContestView'
import { parseRules, cmpSpec } from '../cssCascade'
import { t } from '../i18n'
import defaultSettings from './__fixtures__/defaultSettings.json'
import type { FieldDayStatus } from '../types'

vi.mock('../api', () => ({
  contestRemoved: vi.fn(async () => []),
  contestUnmerged: vi.fn(async () => []),
  getSettings: vi.fn(async () => ({ ...defaultSettings, fdOperator: 'W9OPX' })),
  setSettings: vi.fn(async () => ({})),
  setFdOperator: vi.fn(async () => ({})),
  exportLog: vi.fn(async () => ''),
  fdClubExport: vi.fn(async () => ''),
  fdClubGivePosition: vi.fn(async () => ({ outcome: 'given' })),
  fdClubRemovePosition: vi.fn(async () => ({ outcome: 'done' })),
  fdClubAllowBack: vi.fn(async () => ({ outcome: 'done' })),
  fdClubImport: vi.fn(async () => ({})),
  openPanelWindow: vi.fn(async () => {}),
}))

const RULES = parseRules(
  readFileSync(resolve(process.cwd(), 'src', 'styles.css'), 'utf8').replace(/\/\*[\s\S]*?\*\//g, ''),
)

/** The winning value of `prop` on `el` over the real sheet (fdDashboardShell.test.tsx's resolver). */
function applied(el: Element, prop: string): string | null {
  let best: { value: string; spec: readonly [number, number, number]; order: number } | null = null
  for (const r of RULES) {
    const d = r.decls.filter((x) => x.prop === prop).pop()
    if (!d) continue
    let hit = false
    try {
      hit = el.matches(r.selector)
    } catch {
      continue
    }
    if (!hit) continue
    if (!best || cmpSpec(r.spec, best.spec) > 0 || (cmpSpec(r.spec, best.spec) === 0 && r.order >= best.order)) {
      best = { value: d.value, spec: r.spec, order: r.order }
    }
  }
  return best ? best.value : null
}

/** An inline `var(--x, fallback)`, resolved where `el` sits: the nearest declaring element's value
 *  (custom properties inherit), else the fallback. */
function inlineMeasure(el: HTMLElement): string {
  const m = /^var\(\s*(--[\w-]+)\s*,\s*(.+)\)$/.exec(el.style.maxWidth)
  if (!m) return el.style.maxWidth || 'none'
  for (let n: Element | null = el; n; n = n.parentElement) {
    const d = applied(n, m[1])
    if (d !== null) return d
  }
  return m[2].trim()
}

const LONG =
  "the host removed this laptop's position from the club event, so it is not syncing. The contacts it already sent stay in the club's log."
const FD = {
  running: true,
  state: 'sp',
  event: 'ilqp',
  qsoCount: 0,
  sections: 0,
  points: 0,
  log: [],
  composing: [],
  club: {
    syncState: 'offline',
    queued: 1,
    offlineSinceUnix: 0,
    hosting: false,
    event: 'W9XYZ IL QSO Party',
    hostCall: 'W9XYZ',
    score: 0,
    qsos: 0,
    sections: 0,
    skewSecs: 0,
    dupes: [],
    dkeys: [],
    board: [],
    lastError: LONG,
  },
} as unknown as FieldDayStatus

afterEach(() => {
  cleanup()
  document.documentElement.removeAttribute('data-viewport')
})

async function mount(): Promise<void> {
  render(
    <main className="layout single">
      <ContestView fieldDay={FD} onSetMode={() => {}} />
    </main>,
  )
  await act(async () => {
    for (let i = 0; i < 8; i++) await Promise.resolve()
  })
}

const TIERS = ['xs', 'sm', 'md', 'lg', 'xl'] as const

describe('the inline-styled blocks keep measures of their own on the full-width screen', () => {
  it('a club sentence: 90ch on lg/xl, none below', async () => {
    await mount()
    const line = screen.getByText(t('fieldDay.club.error', { msg: LONG }))
    for (const tier of TIERS) {
      document.documentElement.setAttribute('data-viewport', tier)
      expect(inlineMeasure(line), `[data-viewport='${tier}']`).toBe(tier === 'lg' || tier === 'xl' ? '90ch' : 'none')
    }
  })

  it('the scoreboard’s operator box, a lone control: 520px on lg/xl, none below', async () => {
    await mount()
    const field = screen.getByLabelText(t('fieldDay.operator.aria')).closest('label') as HTMLElement
    for (const tier of TIERS) {
      document.documentElement.setAttribute('data-viewport', tier)
      expect(inlineMeasure(field), `[data-viewport='${tier}']`).toBe(tier === 'lg' || tier === 'xl' ? '520px' : 'none')
    }
  })
})
