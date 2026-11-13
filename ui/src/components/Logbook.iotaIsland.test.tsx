// @vitest-environment jsdom
//
// REVIEW FIX: an island (IOTA_ISLAND_ID) is one group's. The Logbook's edit form takes the stored
// island off when the IOTA box moves to another group or is cleared, and keeps it when the group is
// left alone. (The backend holds the same rule for every other path: `offer::drop_stale_island`.)
import { describe, it, expect, vi, beforeAll, afterEach } from 'vitest'
import { render, waitFor, fireEvent, cleanup, within } from '@testing-library/react'
import { Logbook } from './Logbook'
import * as api from '../api'
import type { LogQuestion } from '../features/logAnswers'
import { __resetLogSourceForTests } from '../features/logSource'

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
vi.mock('../api', () => {
  const noop = () => vi.fn()
  return {
    askLog: vi.fn(async (q: LogQuestion) => (await import('../features/logAnswers.testkit')).answerAs(q, await engineLog())),
    deleteQsoById: noop(), exportGeneralLog: noop(), importAdif: noop(),
    editQsoById: vi.fn(() => Promise.resolve({ kind: 'applied' })),
    logOperators: vi.fn(() => Promise.resolve([] as string[])), exportLogForOperator: noop(),
    logActivations: vi.fn(() => Promise.resolve([])), exportLogForActivation: noop(),
    // Empty list => no satellite picker rendered, so this suite's DOM is unchanged.
    lotwSatNames: vi.fn(async () => [] as string[]), setSatTagById: vi.fn(async () => ({})),
    logQso: noop(), purgeLog: noop(), qrzLookup: noop(),
    markQslSentById: noop(),
    markQslCardById: noop(),
    syncLotwReport: noop(), uploadLotwReport: noop(), qrzPushQso: noop(),
    clublogPushQso: noop(), hrdlogPushQso: noop(), wrlPushQso: noop(),
  }
})
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn((run: () => Promise<unknown>) => run()),
}))

function oneContact(over: Record<string, unknown> = {}) {
  return [
    {
      id: 'id-K0ABC', call: 'K0ABC', grid: 'EN37', band: '20m', freqMhz: 14.074, mode: 'FT8',
      rstSent: '-10', rstRcvd: '-12', name: null, qth: null, comment: null, notes: null,
      country: 'United States', whenUnix: 1_700_000_000,
      confirmed: false, awardConfirmed: false,
      qslRcvd: null, qslSent: null, ota: null, upload: undefined,
      myGrid: 'EN52XA', myRig: 'IC-705',
      ...over,
    },
  ]
}

async function renderLog(over: Record<string, unknown> = {}) {
  engineLog.mockResolvedValue(oneContact(over))
  const utils = render(<Logbook defaultBand="20m" defaultFreqMhz={14.074} defaultMode="FT8" />)
  await waitFor(() => expect(utils.container.querySelector('.logbook-row:not(.head):not(.placeholder)')).not.toBeNull())
  return utils
}

async function openEdit(container: HTMLElement) {
  fireEvent.click(container.querySelector('button[aria-label="Edit K0ABC"]') as HTMLButtonElement)
  await waitFor(() => expect(container.querySelector('.logbook-form')).not.toBeNull())
  return container.querySelector('.logbook-form') as HTMLElement
}

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
  localStorage.clear()
})

const STORED = { ota: { iota: 'EU-005' }, extra: [['IOTA_ISLAND_ID', '9001']] }

async function saveWithIota(value: string | null) {
  const { container } = await renderLog(STORED)
  const form = await openEdit(container)
  const box = within(form).getByLabelText('IOTA') as HTMLInputElement
  expect(box.value, 'premise: the stored group').toBe('EU-005')
  if (value !== null) fireEvent.change(box, { target: { value } })
  fireEvent.click(within(form).getByRole('button', { name: /save/i }))
  const editQsoById = api.editQsoById as ReturnType<typeof vi.fn>
  await waitFor(() => expect(editQsoById).toHaveBeenCalledTimes(1))
  return editQsoById.mock.calls[0][2] ?? []
}

describe("the edit form keeps an island only with its group (review fix)", () => {
  const drop = [{ tag: 'IOTA_ISLAND_ID', old: '9001', new: null }]

  it('another group takes the island off', async () => {
    expect(await saveWithIota('EU-011')).toEqual(drop)
  })

  it('a cleared box takes the island off', async () => {
    expect(await saveWithIota('')).toEqual(drop)
  })

  it('CONTROL: the group left alone, or typed in another spelling, keeps it', async () => {
    expect(await saveWithIota(null)).toEqual([])
    cleanup()
    vi.clearAllMocks()
    expect(await saveWithIota('eu-5')).toEqual([])
  })
})
