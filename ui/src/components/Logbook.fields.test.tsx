// @vitest-environment jsdom
//
// #239: "More log information should be shown and be able to edit in the logbook — DE and DX
// geographic location, QSL information, rig information — all on the line with a scroll bar."
// Three things this pins: your own grid and rig are shown and saved from the edit form; QSL sent
// and a received card are editable there (through the same commands as the row menu); and a
// "More columns" chip opens the wider table, off by default so nobody's log changes under them.
import { describe, it, expect, vi, beforeAll, afterEach } from 'vitest'
import { render, waitFor, fireEvent, screen, cleanup, within } from '@testing-library/react'
import { Logbook } from './Logbook'
import * as api from '../api'
import { logTarget } from '../remote-web/operation-protocol'

beforeAll(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  Object.defineProperty(HTMLElement.prototype, 'offsetHeight', { configurable: true, value: 600 })
  Object.defineProperty(HTMLElement.prototype, 'offsetWidth', { configurable: true, value: 900 })
})

// What each write hands back: the row as stored, distinguishable by `step`. Hoisted because
// vi.mock factories run before the module body.
const written = vi.hoisted(() => ({
  edited: { call: 'K0ABC', whenUnix: 1_700_000_000, step: 'edited' },
  sent: { call: 'K0ABC', whenUnix: 1_700_000_000, step: 'sent' },
  card: { call: 'K0ABC', whenUnix: 1_700_000_000, step: 'card' },
}))
vi.mock('../api', () => {
  const noop = () => vi.fn()
  return {
    getLog: vi.fn(),
    deleteQso: noop(), exportGeneralLog: noop(), importAdif: noop(),
    // Each write returns the row it wrote — the key the next write in the same form uses.
    editQso: vi.fn(() => Promise.resolve(written.edited)),
    logOperators: vi.fn(() => Promise.resolve([] as string[])), exportLogForOperator: noop(),
    logActivations: vi.fn(() => Promise.resolve([])), exportLogForActivation: noop(),
    logQso: noop(), purgeLog: noop(), qrzLookup: noop(),
    markQslSent: vi.fn(() => Promise.resolve(written.sent)),
    markQslCard: vi.fn(() => Promise.resolve(written.card)),
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
      call: 'K0ABC', grid: 'EN37', band: '20m', freqMhz: 14.074, mode: 'FT8',
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
    const editQso = api.editQso as ReturnType<typeof vi.fn>
    await waitFor(() => expect(editQso).toHaveBeenCalled())
    const record = editQso.mock.calls[0][1]
    expect(record.myGrid).toBe('EN52XA')
    expect(record.myRig).toBe('FT-991A')
  })
})

describe('QSL status is editable in the edit form (#239)', () => {
  it('marks a card sent by post and a card received, through the row menu commands', async () => {
    const { container } = await renderLog()
    const form = await openEdit(container)
    fireEvent.change(within(form).getByLabelText('QSL sent'), { target: { value: 'D' } })
    fireEvent.click(within(form).getByLabelText('Card received'))
    fireEvent.click(within(form).getByRole('button', { name: /save/i }))

    // Each write is keyed by the row the PREVIOUS write returned: the edit changed the row,
    // so the key the form opened with no longer names it; likewise after the sent mark.
    await waitFor(() => expect(api.markQslCard).toHaveBeenCalled())
    expect(api.editQso).toHaveBeenCalledWith(await logTarget(oneContact()[0]), expect.anything())
    expect(api.markQslSent).toHaveBeenCalledWith(await logTarget(written.edited), 'D')
    expect(api.markQslCard).toHaveBeenCalledWith(await logTarget(written.sent), true)
  })

  it('touches neither when the QSL fields were left alone', async () => {
    const { container } = await renderLog()
    const form = await openEdit(container)
    fireEvent.click(within(form).getByRole('button', { name: /save/i }))
    await waitFor(() => expect(api.editQso).toHaveBeenCalled())
    expect(api.markQslSent).not.toHaveBeenCalled()
    expect(api.markQslCard).not.toHaveBeenCalled()
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
