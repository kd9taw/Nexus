// @vitest-environment jsdom
import { describe, it, expect, vi, beforeAll } from 'vitest'
import { render, waitFor, fireEvent } from '@testing-library/react'
import { Logbook } from './Logbook'
import * as api from '../api'
import * as toast from '../toast'

// react-virtual (virtual-core) measures the scroll element and rows via offsetHeight + a
// ResizeObserver, neither of which jsdom implements — stub them so a non-trivial visible window is
// computed. offsetHeight (not getBoundingClientRect) is what virtual-core actually reads.
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
  return {
    getLog: vi.fn(),
    deleteQso: noop(), editQso: noop(), exportGeneralLog: noop(), importAdif: noop(),
    // #25 per-operator export: the Logbook asks on mount, so the mock must answer.
    // A vi.fn WITH a default implementation: the Logbook calls this on its own during render,
    // so a bare vi.fn returning undefined blows up on .then — and mockResolvedValue can still
    // override it per test.
    logOperators: vi.fn(() => Promise.resolve([] as string[])), exportLogForOperator: noop(),
    logQso: noop(), markQslSent: noop(), purgeLog: noop(), qrzLookup: noop(),
    syncLotwReport: noop(), uploadLotwReport: noop(), qrzPushQso: noop(),
    clublogPushQso: noop(), hrdlogPushQso: noop(),
  }
})
vi.mock('../toast', () => ({ pushToast: vi.fn(), withErrorToast: vi.fn() }))

function fakeLog(n: number) {
  return Array.from({ length: n }, (_, i) => ({
    call: `K${i}ABC`,
    grid: 'EN37',
    band: '20m',
    freqMhz: 14.074 + i * 1e-6,
    mode: 'FT8',
    rstSent: '-10',
    rstRcvd: '-12',
    name: null,
    qth: null,
    comment: null,
    notes: null,
    country: 'United States',
    whenUnix: 1_700_000_000 + i,
    confirmed: false,
    awardConfirmed: false,
    qslRcvd: null,
    qslSent: null,
    ota: null,
    upload: undefined,
  }))
}

describe('Logbook virtualization', () => {
  it('mounts only a small window of rows for a large (5k) log', async () => {
    const N = 5000
    ;(api.getLog as ReturnType<typeof vi.fn>).mockResolvedValue(fakeLog(N))
    const { container } = render(
      <Logbook defaultBand="20m" defaultFreqMhz={14.074} defaultMode="FT8" />,
    )
    // Wait for the async getLog → setLog → virtualized render (the spacer div appears only once the
    // log has loaded; before that .log-scroll's child is the "no contacts" <p>).
    await waitFor(() => expect(container.querySelector('.log-scroll > div')).not.toBeNull())
    // The virtualizer is engaged over the FULL set — the spacer reserves the whole scroll height
    // (~5000 rows) even though only a small window is realized.
    const spacer = container.querySelector('.log-rows') as HTMLElement
    expect(parseInt(spacer.style.height, 10)).toBeGreaterThan(5000 * 30)
    // A real (small) window is realized — proves rows actually render (catches a dropped scroll
    // ref / broken wiring, which would leave getVirtualItems() empty while the spacer still sized).
    const mounted = container.querySelectorAll('.logbook-row:not(.head)').length
    expect(mounted).toBeGreaterThan(0)
    // ...but nowhere near all 5000 — the whole point of virtualization.
    expect(mounted).toBeLessThan(200)
    // Default sort is newest-first, so the top of the window shows the highest-whenUnix call.
    expect(container.textContent).toContain('K4999ABC')
  })
})

// The purge dialog must warn that a purge also resets the LoTW/eQSL sync cursors. That coupling is
// the non-obvious half of a purge and it cost an operator his whole confirmation history: he purged,
// re-synced, and got 816 of 26,007 QSOs back confirmed, because the cursor still claimed he held
// everything matched up to the old date. The code now clears the cursors, so the pull is correct —
// but it is a FULL history download, far slower than a routine sync, and an operator who is not
// warned reads that wait as a hang. Renders the real dialog and reads the real text: a
// source-grep test would pass on prose that never reaches the screen.
describe('purge confirmation dialog', () => {
  it('warns that purging resets the LoTW/eQSL sync position', async () => {
    ;(api.getLog as ReturnType<typeof vi.fn>).mockResolvedValue(fakeLog(3))
    const { container } = render(
      <Logbook defaultBand="20m" defaultFreqMhz={14.074} defaultMode="FT8" />,
    )
    await waitFor(() => expect(container.querySelector('.log-scroll > div')).not.toBeNull())

    // The button is disabled on an empty log, so this also proves the log actually loaded.
    const btn = [...container.querySelectorAll('button')].find(
      (b) => b.textContent?.trim() === 'Purge log',
    ) as HTMLButtonElement
    expect(btn).toBeTruthy()
    expect(btn.disabled).toBe(false)
    btn.click()

    const dialog = await waitFor(() => {
      const d = container.querySelector('.purge-confirm')
      expect(d).not.toBeNull()
      return d as HTMLElement
    })
    const text = dialog.textContent ?? ''

    // The irreversibility warning that was always there — a regression here matters as much.
    expect(text).toMatch(/permanently deletes/i)
    expect(text).toMatch(/no undo/i)

    // The new coupling warning: it must name BOTH services and say a full re-download follows.
    // Asserting the meaning, not one phrasing, so a reword does not red the suite falsely.
    expect(text).toMatch(/LoTW/)
    expect(text).toMatch(/eQSL/)
    expect(text).toMatch(/sync position|sync cursor/i)
    expect(text).toMatch(/re-?downloads?[^.]*confirmation history/i)
    // And that it sets the expectation about duration — the whole point of telling them.
    expect(text).toMatch(/longer|slower/i)
  })
})

// Per-operator export (#25). POTA and Field Day both require each operator to submit their own
// log; before the operator was stamped on the record there was nothing to split on.
describe('per-operator export', () => {
  it('is not offered to a single-op station', async () => {
    ;(api.getLog as ReturnType<typeof vi.fn>).mockResolvedValue(fakeLog(3))
    ;(api.logOperators as ReturnType<typeof vi.fn>).mockResolvedValue([])
    const { container } = render(
      <Logbook defaultBand="20m" defaultFreqMhz={14.074} defaultMode="FT8" />,
    )
    await waitFor(() => expect(container.querySelector('.log-scroll > div')).not.toBeNull())
    // One operator, or none, means the split file would be identical to Export ADIF.
    expect(
      [...container.querySelectorAll('button')].some((b) =>
        b.textContent?.includes('Export per operator'),
      ),
      'a button that produces a duplicate of the file beside it is noise',
    ).toBe(false)
  })

  it('appears once the log holds more than one operator', async () => {
    ;(api.getLog as ReturnType<typeof vi.fn>).mockResolvedValue(fakeLog(3))
    ;(api.logOperators as ReturnType<typeof vi.fn>).mockResolvedValue(['G0PQR', 'W1ABC'])
    const { container } = render(
      <Logbook defaultBand="20m" defaultFreqMhz={14.074} defaultMode="FT8" />,
    )
    const btn = await waitFor(() => {
      const b = [...container.querySelectorAll('button')].find((x) =>
        x.textContent?.includes('Export per operator'),
      )
      expect(b).toBeTruthy()
      return b as HTMLButtonElement
    })
    // The tooltip names them, because a wrong operator is silent until submission.
    expect(btn.title).toMatch(/G0PQR/)
    expect(btn.title).toMatch(/W1ABC/)
  })
})

// POTA park on the edit form (#60). The park round-trips through the DTO and shows in the table's
// Park column, but the EDIT form never rendered a control for it, so a park could not be viewed,
// corrected or added on an existing QSO — and the fix must not become an ADIF-field-loss bug of its
// own (the backend's ota-preserve guard checks only the four park fields, not iota).
describe('POTA park on logbook edit (#60)', () => {
  function logWithOta(ota: unknown) {
    const [row] = fakeLog(1)
    return [{ ...row, ota }]
  }

  it('shows the stored park in the edit form', async () => {
    ;(api.getLog as ReturnType<typeof vi.fn>).mockResolvedValue(
      logWithOta({ theirProgram: 'POTA', theirRef: 'US-1234' }),
    )
    const { container } = render(
      <Logbook defaultBand="20m" defaultFreqMhz={14.074} defaultMode="FT8" />,
    )
    await waitFor(() => expect(container.querySelector('.log-scroll > div')).not.toBeNull())
    fireEvent.click(container.querySelector('button[aria-label="Edit K0ABC"]') as HTMLButtonElement)
    // RED before the fix: the form rendered no park control at all, so this input did not exist.
    await waitFor(() =>
      expect(
        (container.querySelector('input[title*="you worked"]') as HTMLInputElement)?.value,
      ).toBe('US-1234'),
    )
  })

  it('does not drop an IOTA reference when editing a POTA QSO', async () => {
    ;(api.getLog as ReturnType<typeof vi.fn>).mockResolvedValue(
      logWithOta({ theirProgram: 'POTA', theirRef: 'US-1234', iota: 'NA-001' }),
    )
    // The real withErrorToast runs its callback; the module mock is a bare stub that never does,
    // so override it here or editQso is never reached and the assertion is vacuous.
    ;(toast.withErrorToast as ReturnType<typeof vi.fn>).mockImplementation((fn: () => unknown) =>
      fn(),
    )
    const editQso = api.editQso as ReturnType<typeof vi.fn>
    editQso.mockResolvedValue({})
    const { container } = render(
      <Logbook defaultBand="20m" defaultFreqMhz={14.074} defaultMode="FT8" />,
    )
    await waitFor(() => expect(container.querySelector('.log-scroll > div')).not.toBeNull())
    fireEvent.click(container.querySelector('button[aria-label="Edit K0ABC"]') as HTMLButtonElement)
    await waitFor(() =>
      expect(container.querySelector('input[title*="you worked"]')).not.toBeNull(),
    )
    fireEvent.click(container.querySelector('.logbook-form button[type="submit"]') as HTMLButtonElement)
    await waitFor(() => expect(editQso).toHaveBeenCalled())
    const record = editQso.mock.calls[0][1]
    // Editing a POTA QSO that also carries an island reference must keep the iota (the guard's
    // blind spot): a park-only ota built from the four editable fields would silently drop it.
    expect(record.ota.iota).toBe('NA-001')
    expect(record.ota.theirRef).toBe('US-1234')
  })
})
