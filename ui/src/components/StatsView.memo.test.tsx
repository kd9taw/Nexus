// @vitest-environment jsdom
//
// Statistics is six passes over the whole log (`computeLogStats`), and the view used to run them
// in its render body — on every re-render, which App gives it on every 300 ms snapshot. At 150k
// contacts that is the whole dashboard recomputed three times a second for numbers that had not
// changed. Computed once per log now (big-log fix, U3).
import { afterEach, describe, expect, it, vi } from 'vitest'
import { cleanup, render, screen } from '@testing-library/react'
import { StatsView } from './StatsView'
import { computeLogStats } from '../features/logStats'
import { t } from '../i18n'
import type { LoggedQso } from '../types'
import type { LogQuestion } from '../features/logAnswers'

vi.mock('../features/logStats', async (importOriginal) => {
  const real = await importOriginal<typeof import('../features/logStats')>()
  return { ...real, computeLogStats: vi.fn(real.computeLogStats) }
})
const engine = vi.hoisted(() => ({ log: [] as unknown[] }))
vi.mock('../api', () => ({
  // The engine's answer over this log (features/logAnswers.testkit): the roll-up is its to compute.
  askLog: vi.fn(async (q: LogQuestion) => (await import('../features/logAnswers.testkit')).answerAs(q, engine.log as LoggedQso[])),
  // No geographic cards: the frontend statistics are what is under test.
  getLogStats: vi.fn(async () => {
    throw new Error('not under test')
  }),
}))
afterEach(cleanup)

const qso = (call: string, band: string) =>
  ({ call, band, mode: 'FT8', freqMhz: 14.074, whenUnix: 1_700_000_000, confirmed: true, awardConfirmed: false,
    country: 'United States', grid: 'EN37' }) as unknown as LoggedQso

describe('Statistics computes once per log, not once per render', () => {
  it('re-renders with the log unchanged do not recompute it', async () => {
    engine.log = [qso('W1AW', '20m'), qso('K1ABC', '40m'), qso('N2XYZ', '20m')]
    const { rerender } = render(<StatsView />)
    await screen.findByText(t('stats.qsos'))
    const passes = vi.mocked(computeLogStats).mock.calls.length
    expect(passes, 'never computed at all — the control').toBeGreaterThan(0)

    rerender(<StatsView />)
    rerender(<StatsView />)
    rerender(<StatsView />)
    expect(vi.mocked(computeLogStats).mock.calls.length, 'recomputed on a render with the same log').toBe(passes)
  })
})
