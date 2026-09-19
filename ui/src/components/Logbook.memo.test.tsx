// @vitest-environment jsdom
//
// The Logbook re-renders on every 300 ms snapshot (the dial, the tick, a new callback from App),
// and it used to walk the whole log twice per render just to count the LoTW backlog — at 150k
// contacts, that was work done three times a second for a number that had not changed. The count
// is taken once per log now (big-log fix, U3). The positive control is the new log at the end:
// the count must still follow the log it describes.
import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest'
import { cleanup, render, screen } from '@testing-library/react'
import { Logbook } from './Logbook'
import { lotwBacklog } from '../features/lotwBacklog'
import { t } from '../i18n'
import type { LoggedQso } from '../types'

vi.mock('../features/lotwBacklog', async (importOriginal) => {
  const real = await importOriginal<typeof import('../features/lotwBacklog')>()
  return { lotwBacklog: vi.fn(real.lotwBacklog) }
})
const engine = vi.hoisted(() => ({ log: [] as unknown[] }))
vi.mock('../api', () => {
  const noop = () => vi.fn()
  return {
    // Every read of the shared log store answered with the whole log.
    getLogDelta: vi.fn(async () => ({ revision: 1, full: true, rows: engine.log })),
    deleteQso: noop(), editQso: noop(), exportGeneralLog: noop(), importAdif: noop(),
    logOperators: vi.fn(() => Promise.resolve([] as string[])), exportLogForOperator: noop(),
    logActivations: vi.fn(() => Promise.resolve([])), exportLogForActivation: noop(),
    saveTextToDownloads: noop(),
    logQso: noop(), markQslSent: noop(), purgeLog: noop(), qrzLookup: noop(),
    syncLotwReport: noop(), uploadLotwReport: noop(), qrzPushQso: noop(),
    clublogPushQso: noop(), hrdlogPushQso: noop(),
  }
})

beforeAll(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
})
afterEach(cleanup)

const qso = (call: string) =>
  ({ call, grid: 'EN37', band: '20m', freqMhz: 14.074, mode: 'FT8', rstSent: '-10', rstRcvd: '-12',
    whenUnix: 1_700_000_000, confirmed: false, awardConfirmed: false }) as unknown as LoggedQso
const view = (dialMhz: number, logTick: number) => (
  <Logbook defaultBand="20m" defaultFreqMhz={dialMhz} defaultMode="FT8" logTick={logTick} />
)
const uploadButton = (count: number) =>
  screen.findByRole('button', { name: t('logbook.lotw.upload.labelCount', { count }) })

describe('the Logbook counts the LoTW backlog once per log, not once per render', () => {
  it('snapshots with the log unchanged do not recount it; a new log does', async () => {
    engine.log = [qso('W1AW'), qso('K1ABC'), qso('N2XYZ')]
    const { rerender } = render(view(14.074, 1))
    await uploadButton(3)
    const passes = vi.mocked(lotwBacklog).mock.calls.length

    // Three snapshots: the dial moves, the log does not.
    rerender(view(14.075, 1))
    rerender(view(14.076, 1))
    rerender(view(14.077, 1))
    expect(vi.mocked(lotwBacklog).mock.calls.length, 'recounted on a render with the same log').toBe(passes)

    engine.log = [...engine.log, qso('K9XYZ')]
    rerender(view(14.077, 2))
    await uploadButton(4)
    expect(vi.mocked(lotwBacklog).mock.calls.length).toBe(passes + 1)
  })
})
