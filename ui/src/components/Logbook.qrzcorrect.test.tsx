// @vitest-environment jsdom
//
// "Correct at QRZ" — the row action that overwrites QRZ's own copy of ONE contact.
//
// What these hold down is the operator-facing half of the contract; the refusals, the wire
// shapes and the reading of QRZ's answer are the backend's and are tested in
// `tempo_core::qrz_correct`. Here: nothing is sent before the operator has read what changes,
// and QRZ's `RESULT=OK` — which means it added a SECOND copy instead of overwriting — never
// reads as a success on screen.
//
// No test in this tree contacts QRZ: the api module is mocked, so a call that escaped the mock
// would fail rather than reach the network.
import { describe, it, expect, vi, beforeAll, beforeEach } from 'vitest'
import { render, waitFor, screen, fireEvent, act, cleanup } from '@testing-library/react'
import { Logbook } from './Logbook'
import * as api from '../api'
import * as toast from '../toast'

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
    deleteQso: noop(), editQso: noop(), exportGeneralLog: noop(), importAdif: noop(),
    logOperators: vi.fn(() => Promise.resolve([] as string[])), exportLogForOperator: noop(),
    logActivations: vi.fn(() => Promise.resolve([])), exportLogForActivation: noop(),
    logQso: noop(), purgeLog: noop(), qrzLookup: noop(),
    markQslSent: vi.fn(() => Promise.resolve({})),
    markQslCard: vi.fn(() => Promise.resolve({})),
    syncLotwReport: noop(), uploadLotwReport: noop(), qrzPushQso: noop(),
    clublogPushQso: noop(), hrdlogPushQso: noop(), wrlPushQso: noop(),
    qrzCorrectPreview: vi.fn(),
    qrzCorrectApply: vi.fn(),
    qrzCorrectUndoMiss: vi.fn(),
  }
})
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn((run: () => Promise<unknown>) => run()),
}))

function onePhoneContact() {
  return [
    {
      call: 'W1AW', grid: null, band: '20m', freqMhz: 14.25, mode: 'USB',
      rstSent: '59', rstRcvd: '59', name: null, qth: null, comment: null, notes: null,
      country: 'United States', whenUnix: 1_709_303_520,
      confirmed: false, awardConfirmed: false,
      qslRcvd: null, qslSent: null, ota: null, upload: undefined,
    },
  ]
}

const mock = <T,>(f: T) => f as unknown as ReturnType<typeof vi.fn>

/** Click and let the handler's promises settle — these flows are all async. */
async function click(el: HTMLElement) {
  await act(async () => {
    fireEvent.click(el)
  })
}

async function renderLog() {
  mock(api.getLog).mockResolvedValue(onePhoneContact())
  const r = render(<Logbook defaultBand="20m" defaultFreqMhz={14.25} defaultMode="USB" />)
  await waitFor(() => expect(r.container.querySelector('.log-scroll > div')).not.toBeNull())
  return r
}

beforeEach(() => {
  // This suite renders the Logbook once per test; without an explicit unmount the previous
  // render's rows stay in the document and every query finds two of everything.
  cleanup()
  vi.clearAllMocks()
})

describe('the row offers a correction, distinctly from the ordinary push', () => {
  it('renders its own button with its own accessible name', async () => {
    await renderLog()
    const correct = await screen.findByRole('button', { name: 'Correct W1AW at QRZ' })
    // Positive control: the ordinary push is a DIFFERENT button on the same row, so the one
    // above is not simply the push found under a new name.
    const push = screen.getByRole('button', { name: 'Push W1AW to QRZ' })
    expect(correct).not.toBe(push)
    expect(correct.textContent).toContain('QRZ')
  })
})

describe('nothing is sent until the operator has read what will change', () => {
  it('previews, shows the callsign, the date and the change, and sends nothing yet', async () => {
    mock(api.qrzCorrectPreview).mockResolvedValue({
      call: 'W1AW',
      date: '2024-03-01',
      confirmation:
        'Correct W1AW at QRZ — the contact of 2024-03-01 at 1432Z.\n\nThis will change:\n  SUBMODE: (not set) → USB\n',
    })
    await renderLog()
    await click(await screen.findByRole('button', { name: 'Correct W1AW at QRZ' }))

    await waitFor(() => expect(api.qrzCorrectPreview).toHaveBeenCalledTimes(1))
    // ONE contact, always — the count the backend refuses on is what the UI passes.
    expect(mock(api.qrzCorrectPreview).mock.calls[0][1]).toBe(1)

    const dialog = await screen.findByRole('dialog', { name: 'Correct a contact at QRZ' })
    expect(dialog.textContent).toContain('W1AW')
    expect(dialog.textContent).toContain('2024-03-01')
    expect(dialog.textContent).toContain('SUBMODE')
    // And this is the point: reading is not writing.
    expect(api.qrzCorrectApply).not.toHaveBeenCalled()
  })

  it('cancelling sends nothing at all', async () => {
    mock(api.qrzCorrectPreview).mockResolvedValue({
      call: 'W1AW', date: '2024-03-01', confirmation: 'SUBMODE: (not set) → USB',
    })
    await renderLog()
    await click(await screen.findByRole('button', { name: 'Correct W1AW at QRZ' }))
    await screen.findByRole('dialog', { name: 'Correct a contact at QRZ' })
    await click(screen.getByRole('button', { name: 'Cancel' }))

    expect(api.qrzCorrectApply).not.toHaveBeenCalled()
    await waitFor(() =>
      expect(screen.queryByRole('dialog', { name: 'Correct a contact at QRZ' })).toBeNull(),
    )
    // Positive control: confirming DOES send — so the absence above is the cancel working.
    mock(api.qrzCorrectApply).mockResolvedValue({ ok: true, message: 'corrected', canRecover: false })
    await click(screen.getByRole('button', { name: 'Correct W1AW at QRZ' }))
    await screen.findByRole('dialog', { name: 'Correct a contact at QRZ' })
    await click(screen.getByRole('button', { name: 'Correct it at QRZ' }))
    await waitFor(() => expect(api.qrzCorrectApply).toHaveBeenCalledTimes(1))
  })

  it('a refusal is reported and no correction is attempted', async () => {
    mock(api.qrzCorrectPreview).mockRejectedValue(
      'This contact has no time of day recorded. QRZ matches on a ±30-minute window…',
    )
    await renderLog()
    await click(await screen.findByRole('button', { name: 'Correct W1AW at QRZ' }))

    await waitFor(() => expect(toast.pushToast).toHaveBeenCalled())
    expect(mock(toast.pushToast).mock.calls[0][0]).toContain('no time of day')
    expect(mock(toast.pushToast).mock.calls[0][1]).toBe('error')
    expect(api.qrzCorrectApply).not.toHaveBeenCalled()
    expect(screen.queryByRole('dialog', { name: 'Correct a contact at QRZ' })).toBeNull()
  })
})

describe('QRZ inserting a duplicate is shown as a failure, never a success', () => {
  it('reports the miss and offers to delete exactly the record QRZ added', async () => {
    mock(api.qrzCorrectPreview).mockResolvedValue({
      call: 'W1AW', date: '2024-03-01', confirmation: 'SUBMODE: (not set) → USB',
    })
    // This is QRZ's RESULT=OK: it did not overwrite, it inserted a SECOND copy.
    mock(api.qrzCorrectApply).mockResolvedValue({
      ok: false,
      message:
        'QRZ did NOT correct W1AW on 2024-03-01 at 1432Z. … QRZ added a SECOND copy. The new record is QRZ log id 130877825. Nothing has been deleted yet.',
      canRecover: true,
    })
    await renderLog()
    await click(await screen.findByRole('button', { name: 'Correct W1AW at QRZ' }))
    await screen.findByRole('dialog', { name: 'Correct a contact at QRZ' })
    await click(screen.getByRole('button', { name: 'Correct it at QRZ' }))

    const dialog = await screen.findByRole('dialog', { name: 'Correct a contact at QRZ' })
    await waitFor(() => expect(dialog.textContent).toContain('SECOND copy'))
    expect(dialog.textContent).toContain('130877825')
    expect(dialog.textContent).toContain('Nothing has been deleted yet')

    // The recovery is OFFERED, never taken on its own — QRZ's delete is permanent.
    const recover = screen.getByRole('button', { name: 'Delete the duplicate QRZ added' })
    expect(api.qrzCorrectUndoMiss).not.toHaveBeenCalled()
    mock(api.qrzCorrectUndoMiss).mockResolvedValue('Deleted the duplicate QRZ added for W1AW…')
    await click(recover)
    await waitFor(() => expect(api.qrzCorrectUndoMiss).toHaveBeenCalledTimes(1))
  })

  it('a real replace reports success and offers no deletion', async () => {
    // The positive control for the test above: the same flow with QRZ's RESULT=REPLACE.
    mock(api.qrzCorrectPreview).mockResolvedValue({
      call: 'W1AW', date: '2024-03-01', confirmation: 'SUBMODE: (not set) → USB',
    })
    mock(api.qrzCorrectApply).mockResolvedValue({
      ok: true,
      message: "QRZ's copy of W1AW on 2024-03-01 at 1432Z is corrected.",
      canRecover: false,
    })
    await renderLog()
    await click(await screen.findByRole('button', { name: 'Correct W1AW at QRZ' }))
    await screen.findByRole('dialog', { name: 'Correct a contact at QRZ' })
    await click(screen.getByRole('button', { name: 'Correct it at QRZ' }))

    const dialog = await screen.findByRole('dialog', { name: 'Correct a contact at QRZ' })
    await waitFor(() => expect(dialog.textContent).toContain('is corrected'))
    expect(screen.queryByRole('button', { name: 'Delete the duplicate QRZ added' })).toBeNull()
  })
})
