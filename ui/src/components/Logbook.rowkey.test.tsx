// @vitest-environment jsdom
//
// The shack's log view and a Remote browser are two writers of one log. This view used to
// address a row by its position at load time, and loaded once; a browser's delete removed a
// row and shifted every later one, and the shack's next Delete or Edit went to a DIFFERENT
// contact under a toast naming the one the operator meant. Three things this pins: a row is
// addressed by the row itself (the station keys it), the edit form stays on the row it opened
// with while the list changes under it, and the list reloads when the engine says the log
// changed.
import { describe, it, expect, vi, beforeAll, afterEach } from 'vitest'
import { render, waitFor, fireEvent, cleanup, act } from '@testing-library/react'
import { Logbook } from './Logbook'
import { ConfirmHost } from '../confirm'
import * as api from '../api'
import { t } from '../i18n'

beforeAll(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  Object.defineProperty(HTMLElement.prototype, 'offsetHeight', { configurable: true, value: 600 })
  Object.defineProperty(HTMLElement.prototype, 'offsetWidth', { configurable: true, value: 900 })
})

vi.mock('../api', () => {
  const noop = () => vi.fn()
  const getLog = vi.fn()
  return {
    getLog,
    // The Logbook reads the shared log store, which asks get_log_delta. Every answer here is
    // the whole log (a valid answer), stocked through `getLog` as before.
    getLogDelta: vi.fn(async () => ({ revision: 1, full: true, rows: await getLog() })),
    deleteQsoById: vi.fn(() => Promise.resolve({ kind: 'deleted' })),
    editQsoById: vi.fn(() => Promise.resolve({ kind: 'applied' })),
    exportGeneralLog: noop(), importAdif: noop(),
    logOperators: vi.fn(() => Promise.resolve([] as string[])), exportLogForOperator: noop(),
    logActivations: vi.fn(() => Promise.resolve([])), exportLogForActivation: noop(),
    // Empty list => no satellite picker rendered, so this suite's DOM is unchanged.
    lotwSatNames: vi.fn(async () => [] as string[]), setSatTagById: vi.fn(async () => ({})),
    logQso: noop(), purgeLog: noop(), qrzLookup: noop(),
    markQslSentById: noop(), markQslCardById: noop(),
    syncLotwReport: noop(), uploadLotwReport: noop(), qrzPushQso: noop(),
    clublogPushQso: noop(), hrdlogPushQso: noop(), wrlPushQso: noop(),
  }
})
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn((run: () => Promise<unknown>) => run()),
}))

const contact = (call: string, whenUnix: number) => ({
  id: `id-${call}`, call, grid: 'EN37', band: '20m', freqMhz: 14.074, mode: 'FT8',
  rstSent: '-10', rstRcvd: '-12', name: null, qth: null, comment: null, notes: null,
  country: 'United States', whenUnix, confirmed: false, awardConfirmed: false,
  qslRcvd: null, qslSent: null, ota: null,
})
// Oldest first, as get_log returns them: W1AW at 0, K1ABC at 1, N2XYZ at 2.
const three = () => [contact('W1AW', 1_700_000_000), contact('K1ABC', 1_700_000_100), contact('N2XYZ', 1_700_000_200)]

async function renderLog(rows: ReturnType<typeof three>, logTick = 1) {
  ;(api.getLog as ReturnType<typeof vi.fn>).mockResolvedValue(rows)
  const utils = render(
    <>
      <Logbook defaultBand="20m" defaultFreqMhz={14.074} defaultMode="FT8" logTick={logTick} />
      <ConfirmHost />
    </>,
  )
  await waitFor(() => expect(utils.container.querySelector('.log-scroll > div')).not.toBeNull())
  return utils
}

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
  localStorage.clear()
})

describe('the shack addresses a contact by the id of the row it saw, never its position', () => {
  it('deletes the row the operator confirmed', async () => {
    const { container, findByRole } = await renderLog(three())
    fireEvent.click(container.querySelector('button[aria-label="Delete K1ABC"]') as HTMLButtonElement)
    fireEvent.click(await findByRole('button', { name: t('logbook.delete.confirm') }))
    await waitFor(() => expect(api.deleteQsoById).toHaveBeenCalled())
    expect(api.deleteQsoById).toHaveBeenCalledWith(expect.objectContaining({ id: 'id-K1ABC' }))
  })

  it('edits the row the form opened with, after a delete above it has shifted the list', async () => {
    const { container, rerender } = await renderLog(three())
    fireEvent.click(container.querySelector('button[aria-label="Edit K1ABC"]') as HTMLButtonElement)
    await waitFor(() => expect(container.querySelector('.logbook-form')).not.toBeNull())

    // A Remote browser deletes W1AW; the engine's tick moves and the list reloads. Position 1
    // — the one the form opened at — now names N2XYZ.
    ;(api.getLog as ReturnType<typeof vi.fn>).mockResolvedValue(three().slice(1))
    rerender(
      <>
        <Logbook defaultBand="20m" defaultFreqMhz={14.074} defaultMode="FT8" logTick={2} />
        <ConfirmHost />
      </>,
    )
    await waitFor(() => expect(api.getLog).toHaveBeenCalledTimes(2))
    await waitFor(() => expect(container.querySelector('button[aria-label="Edit W1AW"]')).toBeNull())

    fireEvent.click(container.querySelector('.logbook-form button[type="submit"]') as HTMLButtonElement)
    await waitFor(() => expect(api.editQsoById).toHaveBeenCalled())
    const [target, edit] = (api.editQsoById as ReturnType<typeof vi.fn>).mock.calls[0]
    expect(target).toEqual(expect.objectContaining({ id: 'id-K1ABC' }))
    expect(edit.call).toBe('K1ABC')
  })
})

describe('the list follows the engine', () => {
  it('reloads when logTick moves, once per burst, and not for the value it mounted with', async () => {
    const { rerender } = await renderLog(three(), 5)
    expect(api.getLog).toHaveBeenCalledTimes(1)
    const at = (tick: number) =>
      rerender(
        <>
          <Logbook defaultBand="20m" defaultFreqMhz={14.074} defaultMode="FT8" logTick={tick} />
          <ConfirmHost />
        </>,
      )
    // The same tick again is not a change.
    at(5)
    await act(() => new Promise((r) => setTimeout(r, 400)))
    expect(api.getLog).toHaveBeenCalledTimes(1)
    // Three stamps in quick succession — one fetch.
    at(6)
    at(7)
    at(8)
    await waitFor(() => expect(api.getLog).toHaveBeenCalledTimes(2))
    await act(() => new Promise((r) => setTimeout(r, 400)))
    expect(api.getLog).toHaveBeenCalledTimes(2)
  })
})
