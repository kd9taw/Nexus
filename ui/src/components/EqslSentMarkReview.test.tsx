// @vitest-environment jsdom
//
// "Check eQSL sent marks": the contacts holding the "QSL sent, electronic" mark an eQSL card copied
// onto them, every one ticked, for the operator to untick any they set and clear the rest. Nothing
// is sent until the button, only the ticked contacts are, and the notice's Undo puts back exactly
// the contacts the clear changed, at the clear's own time. The api is a mock: no test here can
// change a log.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, cleanup, waitFor, within } from '@testing-library/react'
import { EqslSentMarkReview } from './EqslSentMarkReview'
import { clearEqslSentMarks, restoreEqslSentMarks, type EqslSentMark } from '../api'
import { pushToast } from '../toast'
import { t } from '../i18n'
import { utcDate } from '../features/utcLog'

vi.mock('../api', () => ({
  clearEqslSentMarks: vi.fn(),
  restoreEqslSentMarks: vi.fn(),
}))
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn((run: () => Promise<unknown>) => run()),
}))

const clear = vi.mocked(clearEqslSentMarks)
const restore = vi.mocked(restoreEqslSentMarks)
const toast = vi.mocked(pushToast)

function marked(call: string, whenUnix: number, band: string, mode: string): EqslSentMark {
  return { id: `id-${call}`, call, whenUnix, band, mode }
}
const ROWS = [marked('K2BB', 1_767_316_922, '20m', 'FT8'), marked('K1AA', 1_767_315_661, '40m', 'CW')]

beforeEach(() => {
  clear.mockReset().mockResolvedValue({ ids: [], clearedUnix: 0 })
  restore.mockReset().mockResolvedValue(0)
  toast.mockReset()
})
afterEach(cleanup)

function opened(rows: readonly EqslSentMark[] = ROWS) {
  const onClose = vi.fn()
  const onChanged = vi.fn()
  render(<EqslSentMarkReview open rows={rows} onClose={onClose} onChanged={onChanged} />)
  const dialog = screen.getByRole('dialog', { name: t('logbook.eqslSentMarks.title') })
  return { dialog, onClose, onChanged }
}
const lineOf = (dialog: HTMLElement, call: string) => within(dialog).getByText(call).closest('label') as HTMLElement
const boxOf = (dialog: HTMLElement, call: string) => within(lineOf(dialog, call)).getByRole('checkbox') as HTMLInputElement
const clearButton = (count: number) =>
  screen.getByRole('button', { name: t('logbook.eqslSentMarks.clear', { count }) }) as HTMLButtonElement

describe('Check eQSL sent marks', () => {
  it('lists each contact with its date, band, mode and mark, every one ticked, and says why', () => {
    const { dialog } = opened()
    expect(dialog.textContent).toContain(t('logbook.eqslSentMarks.intro'))
    const mark = t('logbook.qsl.sentVia', { via: t('logbook.qsl.via.electronic') })
    expect([lineOf(dialog, 'K2BB').textContent, lineOf(dialog, 'K1AA').textContent]).toEqual([
      `K2BB${utcDate(1_767_316_922)}20m FT8${mark}`,
      `K1AA${utcDate(1_767_315_661)}40m CW${mark}`,
    ])
    expect([boxOf(dialog, 'K2BB').checked, boxOf(dialog, 'K1AA').checked]).toEqual([true, true])
    expect(clearButton(2).disabled).toBe(false)
    expect(clear, 'nothing is sent by opening it').not.toHaveBeenCalled()
  })

  it('clears only the ticked contacts, and its Undo puts back exactly the ones it cleared', async () => {
    clear.mockResolvedValue({ ids: ['id-K2BB'], clearedUnix: 1_780_000_000 })
    restore.mockResolvedValue(1)
    const { dialog, onClose, onChanged } = opened()
    fireEvent.click(boxOf(dialog, 'K1AA'))
    expect(boxOf(dialog, 'K1AA').checked).toBe(false)
    fireEvent.click(clearButton(1))
    await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1))
    expect(clear.mock.calls).toEqual([[['id-K2BB']]])
    expect(onChanged).toHaveBeenCalledTimes(1)
    expect(toast.mock.calls.length).toBe(1)
    const [message, kind, , opts] = toast.mock.calls[0]
    expect([message, kind, opts?.actionLabel]).toEqual([
      t('logbook.eqslSentMarks.done', { count: 1 }),
      'info',
      t('logbook.eqslSentMarks.undo'),
    ])
    expect(restore, 'nothing is put back until Undo').not.toHaveBeenCalled()
    opts?.action?.()
    await waitFor(() => expect(onChanged).toHaveBeenCalledTimes(2))
    expect(restore.mock.calls).toEqual([[['id-K2BB'], 1_780_000_000]])
    expect(toast.mock.calls[1].slice(0, 2)).toEqual([t('logbook.eqslSentMarks.restored', { count: 1 }), 'success'])
  })

  it('offers no Undo for a clear that changed nothing', async () => {
    const { onClose } = opened()
    fireEvent.click(clearButton(2))
    await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1))
    expect(clear.mock.calls).toEqual([[['id-K2BB', 'id-K1AA']]])
    const [message, , , opts] = toast.mock.calls[0]
    expect([message, opts?.action]).toEqual([t('logbook.eqslSentMarks.done', { count: 0 }), undefined])
  })

  it('sends nothing on Cancel or Escape, and nothing with every contact unticked', () => {
    const { dialog, onClose } = opened()
    fireEvent.click(boxOf(dialog, 'K1AA'))
    fireEvent.click(boxOf(dialog, 'K2BB'))
    expect(clearButton(0).disabled).toBe(true)
    fireEvent.click(clearButton(0))
    fireEvent.click(screen.getByRole('button', { name: t('logbook.eqslSentMarks.cancel') }))
    fireEvent.keyDown(window, { key: 'Escape' })
    expect(onClose).toHaveBeenCalledTimes(2)
    expect(clear).not.toHaveBeenCalled()
  })

  // With thousands of lines, Tab otherwise passes every checkbox before Cancel (5,000 marks: Cancel
  // was Tab stop 5,001 in Chrome). The dialog's first Tab stop skips the list to its buttons.
  it("first Tab stop skips the list: it puts the keyboard on Cancel and changes nothing", () => {
    const { dialog, onClose } = opened()
    const stops = [...dialog.querySelectorAll<HTMLElement>('button, input')].filter((e) => e.tabIndex >= 0)
    const skip = within(dialog).getByRole('button', { name: t('logbook.eqslSentMarks.skip') })
    expect(stops[0], 'the skip comes before every line').toBe(skip)
    expect(stops.slice(1).every((e) => e.compareDocumentPosition(skip) & Node.DOCUMENT_POSITION_PRECEDING)).toBe(true)
    fireEvent.click(skip)
    expect(document.activeElement).toBe(within(dialog).getByRole('button', { name: t('logbook.eqslSentMarks.cancel') }))
    expect(onClose).not.toHaveBeenCalled()
    expect(clear).not.toHaveBeenCalled()
    expect(boxOf(dialog, 'K1AA').checked && boxOf(dialog, 'K2BB').checked).toBe(true)
  })
})
