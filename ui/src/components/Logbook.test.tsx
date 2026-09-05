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

// OPERATOR REPORT (2026-08-23): "Notes (shared) are not visible in the program... They should
// also be visible in the Log viewer. I've never found anywhere that 'Private' note show up
// either."
//
// Half right, and this is the half that was wrong. Both fields were WRITE-ONLY in the log
// table: the row editor took a Comment and multi-line Notes, saved them, and the table showed
// neither back — so the only way to read a note was to open the row you had no way of knowing
// held one. (The callsign-recall card DOES surface them, which is the other half of the
// report; that path has its own tests in RecallPanel.test.tsx.)
describe('the log table shows the comment and flags a private note', () => {
  function withNotes(over: { comment?: string | null; notes?: string | null }) {
    return [{ ...fakeLog(1)[0], ...over }]
  }

  it('shows the shared comment inline', async () => {
    ;(api.getLog as ReturnType<typeof vi.fn>).mockResolvedValue(
      withNotes({ comment: 'Rag chew about his 6-el yagi' }),
    )
    const { container } = render(
      <Logbook defaultBand="20m" defaultFreqMhz={14.074} defaultMode="FT8" />,
    )
    await waitFor(() => expect(container.querySelector('.log-note')).not.toBeNull())
    expect((container.querySelector('.log-note') as HTMLElement).textContent).toContain(
      'Rag chew about his 6-el yagi',
    )
  })

  it('flags a private note with 📝 and carries the text in the tooltip', async () => {
    ;(api.getLog as ReturnType<typeof vi.fn>).mockResolvedValue(
      withNotes({ comment: null, notes: 'Runs a KX3 at 5W from a sailboat' }),
    )
    const { container } = render(
      <Logbook defaultBand="20m" defaultFreqMhz={14.074} defaultMode="FT8" />,
    )
    await waitFor(() => expect(container.querySelector('.log-note-flag')).not.toBeNull())
    const cell = container.querySelector('.log-note') as HTMLElement
    // The marker is visible; the multi-line text itself lives in the tooltip, because a
    // multi-line note in a one-line table cell is how a row height starts fighting the
    // virtualizer's measurement.
    expect(cell.textContent).toContain('📝')
    expect(cell.title).toContain('Runs a KX3 at 5W from a sailboat')
  })

  it('POSITIVE CONTROL — a row with neither shows no marker and no tooltip', async () => {
    ;(api.getLog as ReturnType<typeof vi.fn>).mockResolvedValue(
      withNotes({ comment: null, notes: null }),
    )
    const { container } = render(
      <Logbook defaultBand="20m" defaultFreqMhz={14.074} defaultMode="FT8" />,
    )
    await waitFor(() => expect(container.querySelector('.log-note')).not.toBeNull())
    const cell = container.querySelector('.log-note') as HTMLElement
    expect(container.querySelector('.log-note-flag')).toBeNull()
    expect(cell.title).toBe('')
  })

  it('keeps the header and the data rows on the SAME track count', async () => {
    // The row is a CSS grid with a fixed template; a header cell added without its data cell
    // (or the reverse) silently shears every column after it. Counting both is the cheap guard
    // that a rendered-structure test can actually make in jsdom.
    ;(api.getLog as ReturnType<typeof vi.fn>).mockResolvedValue(fakeLog(2))
    const { container } = render(
      <Logbook defaultBand="20m" defaultFreqMhz={14.074} defaultMode="FT8" />,
    )
    await waitFor(() => expect(container.querySelector('.logbook-row:not(.head)')).not.toBeNull())
    const head = container.querySelectorAll('.logbook-row.head > .log-cell').length
    const row = container.querySelectorAll('.logbook-row:not(.head)')[0].querySelectorAll(':scope > .log-cell').length
    expect(row).toBe(head)
  })
})

// #152, REPORTED AGAIN AFTER THE 1.8.0 FIX SHIPPED (rgoiko). The control existed and could not
// be reached: it was gated on `needsConfirmOnly && !q.qslSent?.sent`.
//
// Both halves were wrong. The filter half meant the menu only appeared while the "needs
// confirmation" chip happened to be on, so an operator working through a stack of cards in the
// ordinary Logbook found nothing. The sent half is worse — a paper QSL is a ROUND TRIP, so
// removing the menu the moment a card was marked sent deleted the control for the arrival, and
// the very card the feature exists to record could never be recorded.
describe('#152 — recording a QSL card does not depend on a filter, or on not having sent one', () => {
  // ⚠️ SCOPED TO THIS RENDER'S OWN CONTAINER, not `document`. This file has no afterEach
  // cleanup, so a document-wide query finds the FIRST Logbook still mounted from an earlier
  // test — which is exactly how the second case below passed alone and failed in the file,
  // reading another test's row and reporting the opposite answer.
  const qsl = (c: HTMLElement) => c.querySelector('select.log-rowbtn') as HTMLSelectElement | null
  const opts = (c: HTMLElement) => Array.from(qsl(c)?.options ?? []).map((o) => o.value)

  it('is reachable in the ORDINARY logbook, with no filter chip set', async () => {
    ;(api.getLog as ReturnType<typeof vi.fn>).mockResolvedValue(fakeLog(1))
    const { container } = render(
      <Logbook defaultBand="20m" defaultFreqMhz={14.074} defaultMode="FT8" />,
    )
    await waitFor(() => expect(container.querySelector('.logbook-row:not(.head)')).not.toBeNull())
    expect(qsl(container), 'the QSL menu must be on an ordinary row').not.toBeNull()
    expect(opts(container), 'and it offers the inbound card').toContain('R')
  })

  it('still offers the arriving card AFTER one has been marked sent — the round trip', async () => {
    const sent = [{ ...fakeLog(1)[0], qslSent: { sent: true, via: 'B' } }]
    ;(api.getLog as ReturnType<typeof vi.fn>).mockResolvedValue(sent)
    const { container } = render(
      <Logbook defaultBand="20m" defaultFreqMhz={14.074} defaultMode="FT8" />,
    )
    await waitFor(() => expect(container.querySelector('.logbook-row:not(.head)')).not.toBeNull())
    expect(qsl(container), 'the menu must survive marking a card sent').not.toBeNull()
    expect(opts(container), 'the inbound card is still recordable').toContain('R')
    // …and you still cannot send twice.
    expect(opts(container), 'the send entries are gone once sent').not.toContain('B')
  })
})


// The receiving half of #192 (kr4fqg): a previous-contact row in a cockpit's recall card switches
// to the Logbook with that callsign already filtering it. The SEARCH BOX is the seam on purpose —
// a `LoggedQso` has no stable id and the edit/delete API addresses rows by index, so an index
// carried across a view switch is stale by construction. Landing in the visible box also means the
// operator can SEE what is filtering the log and clear it with the ✕ that is already there.
describe('recall-card handoff (#192)', () => {
  it('opens filtered to the handed-over callsign, and reports the handoff consumed', async () => {
    ;(api.getLog as ReturnType<typeof vi.fn>).mockResolvedValue([
      { ...fakeLog(1)[0], call: 'W1AW' },
      { ...fakeLog(1)[0], call: 'K9XYZ', whenUnix: fakeLog(1)[0].whenUnix - 100 },
    ])
    const onConsume = vi.fn()
    const { container } = render(
      <Logbook
        defaultBand="20m"
        defaultFreqMhz={14.074}
        defaultMode="FT8"
        focusCall={{ call: 'W1AW', ts: 1 }}
        onConsumeFocusCall={onConsume}
      />,
    )
    await waitFor(() => expect(container.querySelector('.logbook-row:not(.head)')).not.toBeNull())
    // The box shows the filter rather than applying one invisibly.
    const box = container.querySelector('.log-search') as HTMLInputElement
    expect(box.value).toBe('W1AW')
    await waitFor(() => {
      const calls = [...container.querySelectorAll('.logbook-row:not(.head)')].map(
        (r) => r.textContent ?? '',
      )
      expect(calls.some((c) => c.includes('W1AW'))).toBe(true)
      expect(calls.some((c) => c.includes('K9XYZ'))).toBe(false)
    })
    // …and the parent is told to clear it, so a later trip through the nav opens unfiltered.
    expect(onConsume).toHaveBeenCalled()
  })

  it('opens unfiltered when nothing was handed over', async () => {
    ;(api.getLog as ReturnType<typeof vi.fn>).mockResolvedValue(fakeLog(3))
    const { container } = render(
      <Logbook defaultBand="20m" defaultFreqMhz={14.074} defaultMode="FT8" />,
    )
    await waitFor(() => expect(container.querySelector('.logbook-row:not(.head)')).not.toBeNull())
    expect((container.querySelector('.log-search') as HTMLInputElement).value).toBe('')
    expect(container.querySelectorAll('.logbook-row:not(.head)').length).toBe(3)
  })
})
