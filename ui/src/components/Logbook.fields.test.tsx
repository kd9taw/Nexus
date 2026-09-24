// @vitest-environment jsdom
//
// #239: "More log information should be shown and be able to edit in the logbook — DE and DX
// geographic location, QSL information, rig information — all on the line with a scroll bar."
// Three things this pins: your own grid and rig are shown and saved from the edit form; QSL sent
// and a received card are editable there (in the same one change as the fields); and a
// "More columns" chip opens the wider table, off by default so nobody's log changes under them.
import { describe, it, expect, vi, beforeAll, afterEach } from 'vitest'
import { render, waitFor, fireEvent, screen, cleanup, within } from '@testing-library/react'
import { Logbook } from './Logbook'
import * as api from '../api'

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
  ;(api.getLog as ReturnType<typeof vi.fn>).mockResolvedValue(oneContact(over))
  const utils = render(<Logbook defaultBand="20m" defaultFreqMhz={14.074} defaultMode="FT8" />)
  await waitFor(() => expect(utils.container.querySelector('.log-scroll > div')).not.toBeNull())
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

describe('own location and rig on each contact (#239)', () => {
  it('shows my grid and rig in the edit form and saves a change', async () => {
    const { container } = await renderLog()
    const form = await openEdit(container)
    const myGrid = within(form).getByLabelText('My grid') as HTMLInputElement
    const rig = within(form).getByLabelText('Rig') as HTMLInputElement
    expect(myGrid.value).toBe('EN52XA')
    expect(rig.value).toBe('IC-705')

    fireEvent.change(rig, { target: { value: 'FT-991A' } })
    fireEvent.click(within(form).getByRole('button', { name: /save/i }))
    const editQsoById = api.editQsoById as ReturnType<typeof vi.fn>
    await waitFor(() => expect(editQsoById).toHaveBeenCalled())
    const edit = editQsoById.mock.calls[0][1]
    expect(edit.myGrid).toBe('EN52XA')
    expect(edit.myRig).toBe('FT-991A')
  })
})

describe('QSL status is editable in the edit form (#239)', () => {
  it('marks a card sent by post and a card received, in the same one change as the fields', async () => {
    const { container } = await renderLog()
    const form = await openEdit(container)
    fireEvent.change(within(form).getByLabelText('QSL sent'), { target: { value: 'D' } })
    fireEvent.click(within(form).getByLabelText('Card received'))
    fireEvent.click(within(form).getByRole('button', { name: /save/i }))

    // ONE write, the fields and both marks in it: the three writes the form used to send each
    // changed the row, so each had to target the row the one before it returned.
    await waitFor(() => expect(api.editQsoById).toHaveBeenCalled())
    expect(api.editQsoById).toHaveBeenCalledTimes(1)
    expect(api.editQsoById).toHaveBeenCalledWith(
      expect.objectContaining({ id: 'id-K0ABC' }),
      expect.objectContaining({ call: 'K0ABC', qslSentVia: 'D', qslCard: true }),
    )
    expect(api.markQslSentById).not.toHaveBeenCalled()
    expect(api.markQslCardById).not.toHaveBeenCalled()
  })

  it('sends the marks as they stand when the QSL fields were left alone', async () => {
    // The engine changes a mark only where the edit's differs from the stored contact, so a form
    // left alone must send exactly what is stored — for a contact with neither mark…
    const { container } = await renderLog()
    const form = await openEdit(container)
    fireEvent.click(within(form).getByRole('button', { name: /save/i }))
    await waitFor(() => expect(api.editQsoById).toHaveBeenCalled())
    expect(api.editQsoById).toHaveBeenCalledWith(
      expect.anything(),
      expect.objectContaining({ qslSentVia: null, qslCard: false }),
    )
    cleanup()
    vi.clearAllMocks()

    // …and for one with both — a DIFFERENT answer, so the first cannot pass on a constant.
    const marked = await renderLog({ qslSent: { sent: true, via: 'B', dateUnix: 1_700_000_000 }, qslRcvd: { card: true } })
    const form2 = await openEdit(marked.container)
    fireEvent.click(within(form2).getByRole('button', { name: /save/i }))
    await waitFor(() => expect(api.editQsoById).toHaveBeenCalled())
    expect(api.editQsoById).toHaveBeenCalledWith(
      expect.anything(),
      expect.objectContaining({ qslSentVia: 'B', qslCard: true }),
    )
    expect(api.markQslSentById).not.toHaveBeenCalled()
    expect(api.markQslCardById).not.toHaveBeenCalled()
  })
})

describe('More columns (#239)', () => {
  it('is off by default, and on shows my grid and rig as columns', async () => {
    await renderLog()
    expect(screen.queryByRole('columnheader', { name: 'My grid' }), 'off by default').toBeNull()

    const chip = screen.getByRole('button', { name: 'More columns' })
    expect(chip.getAttribute('aria-pressed')).toBe('false')
    fireEvent.click(chip)

    expect(chip.getAttribute('aria-pressed')).toBe('true')
    expect(screen.getByRole('columnheader', { name: 'My grid' })).toBeTruthy()
    expect(screen.getByRole('columnheader', { name: 'Rig' })).toBeTruthy()
    expect(screen.getByText('EN52XA')).toBeTruthy()
    expect(screen.getByText('IC-705')).toBeTruthy()
  })
})
