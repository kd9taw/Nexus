// @vitest-environment jsdom
//
// D#278 — "is there a way to remove [the Logbook globe], not just reduce its size". There was
// not: the 3-D globe band showed on every machine with a capable GPU. A Settings ▸ Appearance ▸
// Workspace switch now hides it, and the table starts at the top instead.
import { describe, it, expect, vi, beforeAll, beforeEach, afterEach } from 'vitest'
import { render, waitFor, cleanup, act } from '@testing-library/react'
import { Logbook } from './Logbook'
import { LOGBOOK_GLOBE_KEY, setLogbookGlobeShown } from '../features/logbookGlobe'
import type { LogQuestion } from '../features/logAnswers'

beforeAll(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  Object.defineProperty(HTMLElement.prototype, 'offsetHeight', { configurable: true, value: 600 })
  Object.defineProperty(HTMLElement.prototype, 'offsetWidth', { configurable: true, value: 900 })
})

// A capable GPU, so the globe's own gate is open and only the new setting decides.
vi.mock('../gpu', () => ({ gpuCapableForGlobe: () => true }))
vi.mock('./QsoGlobe', () => ({ default: () => <div data-testid="qso-globe" /> }))
/** The log the engine holds: `askLog` answers from it as the engine does (features/logAnswers.testkit). */
const engineLog = vi.hoisted(() => vi.fn())
vi.mock('../api', () => {
  const noop = () => vi.fn()
  return {
    askLog: vi.fn(async (q: LogQuestion) => (await import('../features/logAnswers.testkit')).answerAs(q, await engineLog())),
    deleteQsoById: noop(), editQsoById: noop(), exportGeneralLog: noop(), importAdif: noop(),
    logOperators: vi.fn(() => Promise.resolve([] as string[])), exportLogForOperator: noop(),
    logActivations: vi.fn(() => Promise.resolve([])), exportLogForActivation: noop(),
    // Empty list => no satellite picker rendered, so this suite's DOM is unchanged.
    lotwSatNames: vi.fn(async () => [] as string[]), setSatTagById: vi.fn(async () => ({})),
    saveTextToDownloads: noop(),
    logQso: noop(), markQslSentById: noop(), purgeLog: noop(), qrzLookup: noop(),
    syncLotwReport: noop(), uploadLotwReport: noop(), qrzPushQso: noop(),
    clublogPushQso: noop(), hrdlogPushQso: noop(),
  }
})
vi.mock('../toast', () => ({ pushToast: vi.fn(), withErrorToast: vi.fn() }))

const qso = {
  call: 'K1ABC', grid: 'FN31', band: '40m', freqMhz: 7.074, mode: 'FT8', rstSent: '-10',
  rstRcvd: '-12', name: null, qth: null, comment: null, notes: null, country: 'United States',
  whenUnix: 1_700_000_000, confirmed: false, awardConfirmed: false, qslRcvd: null, qslSent: null,
  ota: null, upload: undefined,
}

async function mount() {
  engineLog.mockResolvedValue([qso])
  const view = render(<Logbook defaultBand="40m" defaultFreqMhz={7.074} defaultMode="FT8" />)
  await waitFor(() => expect(view.container.querySelector('.logbook-row:not(.head)')).not.toBeNull())
  return view.container
}

beforeEach(() => localStorage.clear())
afterEach(cleanup)

describe('hiding the Logbook globe (D#278)', () => {
  it('control: by default the globe band shows above the table', async () => {
    const c = await mount()
    expect(c.querySelector('.log-globe-band')).not.toBeNull()
  })

  it('switched off, there is no globe band and the table is still there', async () => {
    localStorage.setItem(LOGBOOK_GLOBE_KEY, 'off')
    const c = await mount()
    expect(c.querySelector('.log-globe-band'), 'the globe is still on screen').toBeNull()
    expect(c.querySelector('.log-sticky')).not.toBeNull()
  })

  it('follows the switch live, without reopening the Logbook', async () => {
    const c = await mount()
    expect(c.querySelector('.log-globe-band')).not.toBeNull()
    act(() => setLogbookGlobeShown(false))
    expect(c.querySelector('.log-globe-band')).toBeNull()
    act(() => setLogbookGlobeShown(true))
    await waitFor(() => expect(c.querySelector('.log-globe-band')).not.toBeNull())
  })
})
