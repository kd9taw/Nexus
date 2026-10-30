// @vitest-environment jsdom
//
// The Logbook's Path column: off until the operator turns it on; then each row shows its grade from
// the shack (one question for the rows drawn together), a modelled grade dimmer than one stamped
// when logged, the meanings and the grading's success measures in the header's hover. It has no
// sort (the grades partly live outside the log), and every other added column keeps its own.
import { describe, it, expect, vi, beforeAll, afterEach } from 'vitest'
import { render, waitFor, screen, cleanup, fireEvent } from '@testing-library/react'
import { Logbook } from './Logbook'
import * as api from '../api'
import type { LogQuestion } from '../features/logAnswers'
import type { PathGrade } from '../types'
import { forgetPathGrades } from '../features/pathGrade'

beforeAll(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  Object.defineProperty(HTMLElement.prototype, 'offsetHeight', { configurable: true, value: 600 })
  Object.defineProperty(HTMLElement.prototype, 'offsetWidth', { configurable: true, value: 120 })
})

const engineLog = vi.hoisted(() => vi.fn())
vi.mock('../api', () => {
  const noop = () => vi.fn()
  return {
    askLog: vi.fn(async (q: LogQuestion) => (await import('../features/logAnswers.testkit')).answerAs(q, await engineLog())),
    deleteQsoById: noop(), exportGeneralLog: noop(), importAdif: noop(),
    editQsoById: vi.fn(() => Promise.resolve({ kind: 'applied' })),
    logOperators: vi.fn(() => Promise.resolve([] as string[])), exportLogForOperator: noop(),
    logActivations: vi.fn(() => Promise.resolve([])), exportLogForActivation: noop(),
    lotwSatNames: vi.fn(async () => [] as string[]), setSatTagById: vi.fn(async () => ({})),
    logQso: vi.fn(() => Promise.resolve({})), purgeLog: noop(), qrzLookup: noop(),
    markQslSentById: noop(), markQslCardById: noop(),
    syncLotwReport: noop(), uploadLotwReport: noop(), qrzPushQso: noop(),
    clublogPushQso: noop(), hrdlogPushQso: noop(), wrlPushQso: noop(),
    logPathGrades: vi.fn(),
    getSettings: vi.fn(async () => ({})),
    setLogbookFilters: vi.fn(async () => {}),
    pathGradingStatus: vi.fn(),
  }
})
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn((run: () => Promise<unknown>) => run()),
}))

const base = {
  band: '20m', freqMhz: 14.074, mode: 'FT8', rstSent: '-10', rstRcvd: '-12', name: null, qth: null,
  comment: null, notes: null, country: null, confirmed: false, awardConfirmed: false,
  qslRcvd: { card: false, lotw: false, eqsl: false, qrz: false },
  qslSent: { sent: false, via: null, dateUnix: null },
  ota: { myProgram: null, myRef: null, theirProgram: null, theirRef: null, iota: null },
  upload: {}, myGrid: 'FN42',
}
const LOG = [
  { ...base, id: 'a', call: 'VK2AA', grid: 'QF56', whenUnix: 1_778_150_000, extra: [['APP_NEXUS_PATH', 'S']] },
  { ...base, id: 'b', call: 'JA1AA', grid: 'PM95', whenUnix: 1_778_140_000, extra: [] },
  { ...base, id: 'c', call: 'K1NO', grid: null, whenUnix: 1_778_130_000, extra: [] },
]
const GRADES: Record<string, PathGrade | null> = {
  QF56: { verdict: 'short', confidence: 'confirmed', why: ['MARGIN_S', 'HEADING_S'], tier: 'logged', polar: false },
  PM95: { verdict: 'long', confidence: 'possible', why: ['OPEN_L'], tier: 'modelled', polar: true, ssnSource: 'logged' },
}

async function renderLog(shown: string[]) {
  engineLog.mockResolvedValue(LOG)
  vi.mocked(api.logPathGrades).mockImplementation(async (rows) => rows.map((r) => GRADES[r.grid ?? ''] ?? null))
  vi.mocked(api.pathGradingStatus).mockResolvedValue({
    running: false, contacts: 3, graded: 2, ungradable: 1, cellsDone: 1, cellsTotal: 1,
    last30Days: {
      all: { graded: 20, open: 17, outside: 2 }, polar: { graded: 5, open: 3, outside: 1 },
      nonPolar: { graded: 15, open: 14, outside: 1 }, byBand: [], headingJudged: 4, headingAgree: 3,
    },
  })
  const utils = render(<Logbook defaultBand="20m" defaultFreqMhz={14.074} defaultMode="FT8" columns={{ shown, hidden: [], widths: {} }} />)
  await waitFor(() => expect(utils.container.querySelectorAll('.logbook-row:not(.head):not(.placeholder)')).toHaveLength(3))
  return utils
}

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
  localStorage.clear()
  forgetPathGrades()
})

describe('the Path column', () => {
  it('asks nothing while it is off', async () => {
    await renderLog([])
    expect(screen.queryByRole('columnheader', { name: /^Path/ })).toBeNull()
    expect(api.logPathGrades).not.toHaveBeenCalled()
    expect(api.pathGradingStatus).not.toHaveBeenCalled()
  })

  it('shows each row its grade, asked together, and the logged one brighter than the modelled', async () => {
    const { container } = await renderLog(['path'])
    await waitFor(() => expect(container.querySelectorAll('.log-path')).toHaveLength(3))
    await waitFor(() => expect([...container.querySelectorAll('.log-path')].map((c) => c.textContent)).toEqual(['▸ confirmed', '◂ possible ⊕', '—']))
    expect(api.logPathGrades).toHaveBeenCalledTimes(1)
    const [vk, ja] = [...container.querySelectorAll('.log-path')]
    expect(vk.classList.contains('modelled')).toBe(false)
    expect(ja.classList.contains('modelled')).toBe(true)
    expect(ja.getAttribute('title')).toMatch(/unvalidated for long path/)
    expect(vk.getAttribute('title')).toMatch(/confirmed by your beam heading/)
  })

  it('its header explains the marks and carries the success measures, and does not sort', async () => {
    await renderLog(['path'])
    const head = screen.getAllByRole('columnheader').find((h) => h.textContent === 'Path') as HTMLElement
    expect(head.tagName).not.toBe('BUTTON')
    await waitFor(() => expect(head.getAttribute('title')).toMatch(/85%/))
    expect(head.getAttribute('title')).toMatch(/▸ short, ◂ long/)
    expect(head.getAttribute('title')).toMatch(/75%/)
  })
})

describe('the Path filter', () => {
  const lastPageQuery = () => {
    const pages = vi.mocked(api.askLog).mock.calls.map(([q]) => q).filter((q) => q.kind === 'page')
    const last = pages[pages.length - 1]
    return last?.kind === 'page' ? last.query : undefined
  }

  it('asks the engine for the long paths, then the polar ones, and no filter when cleared', async () => {
    await renderLog([])
    fireEvent.click(screen.getByRole('button', { name: 'Filters' }))
    const path = screen.getByLabelText('Path')
    fireEvent.change(path, { target: { value: 'long' } })
    await waitFor(() => expect(lastPageQuery()?.filter).toEqual({ path: 'long' }))
    fireEvent.change(path, { target: { value: 'polar' } })
    await waitFor(() => expect(lastPageQuery()?.filter).toEqual({ path: 'polar' }))
    expect(screen.getByRole('button', { name: 'Filters (1)' })).toBeTruthy()
    // Cleared: back to the unfiltered list (which the window already holds an answer for).
    fireEvent.change(path, { target: { value: '' } })
    await waitFor(() => expect(screen.getByRole('button', { name: 'Filters' })).toBeTruthy())
  })
})
