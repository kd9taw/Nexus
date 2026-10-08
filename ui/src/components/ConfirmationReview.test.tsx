// @vitest-environment jsdom
//
// "Check confirmations": LoTW's whole history, downloaded once when the operator presses Check, and
// the contacts holding a LoTW confirmation or upload mark that LoTW's own records give another
// contact, or none. A line starts ticked only when the station says the evidence decides it, Apply
// sends only the ticked lines, and Cancel or Escape sends nothing and drops the held check. The api
// is a mock: no test here downloads anything or changes a log.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, cleanup, waitFor, within, act } from '@testing-library/react'
import { useState } from 'react'
import { ConfirmationReview } from './ConfirmationReview'
import {
  applyConfirmationCheck,
  cancelConfirmationCheck,
  confirmationCheck,
  type ConfirmationApplied,
  type ConfirmationCheck,
  type ConfirmationLine,
} from '../api'
import { pushToast } from '../toast'
import { t } from '../i18n'

vi.mock('../api', () => ({
  confirmationCheck: vi.fn(),
  applyConfirmationCheck: vi.fn(),
  cancelConfirmationCheck: vi.fn(),
}))
vi.mock('../toast', () => {
  const pushToast = vi.fn()
  return {
    pushToast,
    // As the real one: the failure is said, and the caller gets null.
    withErrorToast: vi.fn(async (run: () => Promise<unknown>, fallback: string) => {
      try {
        return await run()
      } catch (err) {
        pushToast(`${fallback}: ${String(err)}`, 'error')
        return null
      }
    }),
  }
})

const check = vi.mocked(confirmationCheck)
const apply = vi.mocked(applyConfirmationCheck)
const cancel = vi.mocked(cancelConfirmationCheck)
const toast = vi.mocked(pushToast)

const at = (day: number, h: number, m = 0, s = 0) => Date.UTC(2026, 0, day, h, m, s) / 1000

function line(call: string, over: Partial<ConfirmationLine>): ConfirmationLine {
  return {
    id: `id-${call}`, mark: 'lotw', class: 'moved', call, whenUnix: at(5, 6), timeKnown: true,
    band: '20m', mode: 'FT8', rowUnix: at(5, 18, 2), siblingId: `id-${call}-2`, siblingUnix: at(5, 18),
    ownUnix: null, decisive: true, unticked: null, removeGranted: [], removeSubmitted: [],
    cardHeld: false, owedAfter: false, ...over,
  }
}

/** Newest first, as the station sends them; a contact's confirmation line before its upload line. */
const LINES: ConfirmationLine[] = [
  line('K1ABC', { class: 'orphan', whenUnix: at(7, 12), rowUnix: at(7, 14, 10), siblingId: null, siblingUnix: null, decisive: false, unticked: 'orphan', band: '40m', mode: 'CW' }),
  line('N0XYZ', { class: 'contradicted', whenUnix: at(6, 10), rowUnix: at(6, 10, 20), siblingId: null, siblingUnix: null, ownUnix: at(6, 10), cardHeld: true }),
  line('W1AW', { removeGranted: ['DXCC'], owedAfter: true }),
  line('W1AW', { mark: 'lotwUpload', owedAfter: true }),
  line('VE3AAA', { whenUnix: at(4, 23, 59), rowUnix: at(5, 0, 0), siblingUnix: at(5, 0, 1), decisive: false, unticked: 'tie' }),
  line('G4BBB', { whenUnix: at(3, 0), timeKnown: false, rowUnix: at(3, 11), siblingUnix: at(3, 11), decisive: false, unticked: 'dateOnly' }),
  line('KH6CCC', { mark: 'lotwUpload', whenUnix: at(2, 9), rowUnix: at(2, 9, 5), siblingUnix: at(2, 9, 5), decisive: false, unticked: 'notReplayed' }),
]
const FOUND: ConfirmationCheck = {
  session: 7, lines: LINES, gains: 2, unreached: 12, outOfScope: 3, uploadsUnreached: 1, uploadsOutOfScope: 0,
}
const BEFORE_FILE = '/home/op/Nexus/confirmations-before-check-20261007-210000Z.adi'
const APPLIED: ConfirmationApplied = {
  ticked: 3, confirmations: 2, uploads: 1, newlyConfirmed: 2, newlyCredited: 1, beforeFile: BEFORE_FILE,
}

beforeEach(() => {
  check.mockReset()
  apply.mockReset().mockResolvedValue(APPLIED)
  cancel.mockReset().mockResolvedValue(undefined)
  toast.mockReset()
})
afterEach(cleanup)

async function opened(found: ConfirmationCheck = FOUND) {
  check.mockResolvedValue(found)
  const onClose = vi.fn()
  const onApplied = vi.fn()
  render(<ConfirmationReview open onClose={onClose} onApplied={onApplied} />)
  const dialog = await screen.findByRole('dialog', { name: t('logbook.confirmations.title') })
  return { dialog, onClose, onApplied }
}
const checkButton = (dialog: HTMLElement) =>
  within(dialog).getByRole('button', { name: t('logbook.confirmations.check') }) as HTMLButtonElement

/** Opened, and Check pressed: the list on screen. */
async function checked(found: ConfirmationCheck = FOUND) {
  const o = await opened(found)
  fireEvent.click(checkButton(o.dialog))
  if (found.lines.length) await within(o.dialog).findByText(found.lines[0].call)
  else await within(o.dialog).findByText(t('logbook.confirmations.none'))
  return o
}
const rows = (dialog: HTMLElement) => within(dialog).getAllByRole('listitem')
const boxes = (dialog: HTMLElement) =>
  rows(dialog).map((r) => within(r).getByRole('checkbox') as HTMLInputElement)
const applyButton = (dialog: HTMLElement, count: number) =>
  within(dialog).getByRole('button', { name: t('logbook.confirmations.apply', { count }) }) as HTMLButtonElement

describe('Check confirmations', () => {
  it('downloads nothing until Check is pressed, and says so while it downloads', async () => {
    let land: (c: ConfirmationCheck) => void = () => {}
    const { dialog } = await opened()
    check.mockReturnValue(new Promise((r) => { land = r }))
    expect(within(dialog).getByText(t('logbook.confirmations.intro'))).toBeTruthy()
    expect(check, 'opening it downloads nothing').not.toHaveBeenCalled()
    fireEvent.click(checkButton(dialog))
    expect(check).toHaveBeenCalledTimes(1)
    expect(within(dialog).getByText(t('logbook.confirmations.checking'))).toBeTruthy()
    expect(checkButton(dialog).disabled, 'one check at a time').toBe(true)
    await act(async () => land(FOUND))
    expect(within(dialog).queryByText(t('logbook.confirmations.checking'))).toBeNull()
    expect(apply).not.toHaveBeenCalled()
  })

  it('lists each line with what changes, why, and what it keeps or costs', async () => {
    const { dialog } = await checked()
    expect(within(dialog).getByRole('heading', { name: 'LoTW' })).toBeTruthy()
    const text = rows(dialog).map((r) => r.textContent ?? '')
    expect(text.length).toBe(LINES.length)
    const change = t('logbook.confirmations.change')
    const changeUpload = t('logbook.confirmations.changeUpload')
    const owed = t('logbook.confirmations.badge.owed')
    // The orphan: LoTW's row, at its own time, pairs with nothing in the log.
    for (const part of ['K1ABC', '2026-01-07 12:00Z', '40m CW', change, t('logbook.confirmations.why.orphan', { time: '14:10' })])
      expect(text[0]).toContain(part)
    // LoTW holds this contact itself, unconfirmed; the paper card stays.
    for (const part of ['N0XYZ', change, t('logbook.confirmations.why.contradicted'), t('logbook.confirmations.badge.card')])
      expect(text[1]).toContain(part)
    // #400's pair: the 06:00 contact holds the 18:00 contact's confirmation, and its DXCC credit.
    for (const part of ['W1AW', '2026-01-05 06:00Z', '20m FT8', change, t('logbook.confirmations.why.moved', { time: '18:00' }), t('logbook.confirmations.badge.credit', { codes: 'DXCC' })])
      expect(text[2]).toContain(part)
    for (const part of ['W1AW', changeUpload, t('logbook.confirmations.why.uploadMoved', { time: '18:00' }), owed])
      expect(text[3]).toContain(part)
    expect(text[2], 'the upload it is owed is said once, on the contact’s last line').not.toContain(owed)
    // A sibling on another UTC day is named with its date.
    expect(text[4]).toContain(t('logbook.confirmations.why.moved', { time: '2026-01-05 00:01' }))
    // A date-only contact shows no time, and says so.
    expect(text[5]).toContain('2026-01-03')
    expect(text[5]).not.toContain('2026-01-03 00:00')
    expect(text[5]).toContain(t('logbook.confirmations.why.dateOnly'))
    expect(text[6]).toContain(t('logbook.confirmations.why.uploadMoved', { time: '09:05' }))
    // What it leaves alone, under the list; a count of nothing is not shown.
    for (const said of [
      t('logbook.confirmations.gains', { count: 2 }),
      t('logbook.confirmations.unreached', { count: 12 }),
      t('logbook.confirmations.outOfScope', { count: 3 }),
      t('logbook.confirmations.uploadsUnreached', { count: 1 }),
    ])
      expect(within(dialog).getByText(said)).toBeTruthy()
    expect(within(dialog).queryByText(t('logbook.confirmations.uploadsOutOfScope', { count: 0 }))).toBeNull()
  })

  it('ticks a line only when the evidence decides it, and an unticked line says why', async () => {
    const { dialog } = await checked()
    expect(boxes(dialog).map((b) => b.checked)).toEqual(LINES.map((l) => l.decisive))
    const titles = rows(dialog).map((r) => r.querySelector('label')?.getAttribute('title') ?? null)
    expect(titles).toEqual([
      t('logbook.confirmations.unticked.orphan'),
      null,
      null,
      null,
      t('logbook.confirmations.unticked.tie'),
      t('logbook.confirmations.unticked.dateOnly'),
      t('logbook.confirmations.unticked.uploadNotReplayed'),
    ])
    // Two ticked lines of one contact are one contact to change, beside the two gains.
    expect(applyButton(dialog, 4).disabled).toBe(false)
  })

  it('applies only the ticked lines, each with its mark, under the check that listed them', async () => {
    const { dialog, onApplied, onClose } = await checked()
    fireEvent.click(boxes(dialog)[0]) // tick the orphan
    fireEvent.click(boxes(dialog)[2]) // untick W1AW's confirmation, keep its upload line
    fireEvent.click(applyButton(dialog, 5))
    await waitFor(() => expect(apply).toHaveBeenCalledTimes(1))
    expect(apply).toHaveBeenCalledWith(7, [
      { id: 'id-K1ABC', mark: 'lotw' },
      { id: 'id-N0XYZ', mark: 'lotw' },
      { id: 'id-W1AW', mark: 'lotwUpload' },
    ])
    await waitFor(() => expect(onApplied).toHaveBeenCalled())
    expect(onClose).toHaveBeenCalled()
    expect(cancel, 'Apply took the held check; there is nothing to drop').not.toHaveBeenCalled()
  })

  it('sends nothing on Cancel, and drops the check the station holds only once there is one', async () => {
    const before = await opened()
    fireEvent.click(within(before.dialog).getByRole('button', { name: t('logbook.confirmations.cancel') }))
    expect(before.onClose).toHaveBeenCalledTimes(1)
    expect(cancel, 'no check was run, so none is dropped').not.toHaveBeenCalled()
    cleanup()
    const after = await checked()
    fireEvent.click(within(after.dialog).getByRole('button', { name: t('logbook.confirmations.cancel') }))
    expect(after.onClose).toHaveBeenCalledTimes(1)
    expect(cancel).toHaveBeenCalledTimes(1)
    expect(apply).not.toHaveBeenCalled()
  })

  it('closes on Escape, drops the check, and gives the keyboard back to the button that opened it', async () => {
    check.mockResolvedValue(FOUND)
    function Host() {
      const [open, setOpen] = useState(false)
      return (
        <>
          <button type="button" onClick={() => setOpen(true)}>open it</button>
          <ConfirmationReview open={open} onClose={() => setOpen(false)} onApplied={() => {}} />
        </>
      )
    }
    render(<Host />)
    const opener = screen.getByRole('button', { name: 'open it' })
    opener.focus()
    fireEvent.click(opener)
    const dialog = await screen.findByRole('dialog', { name: t('logbook.confirmations.title') })
    expect(dialog.contains(document.activeElement), 'the keyboard goes into the dialog').toBe(true)
    // Pressed from the keyboard: Check disables while it downloads, and a browser drops the focus of
    // a disabled button to the page, so the keyboard moves to the dialog's box first.
    checkButton(dialog).focus()
    fireEvent.click(checkButton(dialog))
    expect(document.activeElement, 'the keyboard stays in the dialog while it checks').toBe(dialog.querySelector('.logconfirm'))
    await within(dialog).findByText('K1ABC')
    fireEvent.keyDown(window, { key: 'Escape' })
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
    expect(document.activeElement).toBe(opener)
    expect(cancel).toHaveBeenCalledTimes(1)
    expect(apply).not.toHaveBeenCalled()
  })

  it('names the file the changed contacts were saved in, and how to put them back', async () => {
    const { dialog } = await checked()
    fireEvent.click(applyButton(dialog, 4))
    await waitFor(() => expect(toast).toHaveBeenCalledTimes(1))
    const [message, kind] = toast.mock.calls[0]
    expect(message).toContain(BEFORE_FILE)
    expect(message).toContain(t('logbook.confirmations.done.beforeFile', { path: BEFORE_FILE }))
    expect(message).toContain(t('logbook.confirmations.done.confirmations', { count: 2 }))
    expect(message).toContain(t('logbook.confirmations.done.uploads', { count: 1 }))
    expect(message).toContain(t('logbook.confirmations.done.gained', { count: 2 }))
    expect(message).not.toContain(t('logbook.confirmations.done.skipped', { count: 0 }))
    // A change to the log, said even with pop-ups off (toast.ts, popsUpWhenOff).
    expect(kind).toBe('info')
  })

  it('says how many ticked lines were left alone because their contacts changed after the check', async () => {
    apply.mockResolvedValue({ ...APPLIED, confirmations: 1, uploads: 0, newlyConfirmed: 0 })
    const { dialog } = await checked()
    fireEvent.click(applyButton(dialog, 4))
    await waitFor(() => expect(toast).toHaveBeenCalledTimes(1))
    const [message] = toast.mock.calls[0]
    expect(message).toContain(t('logbook.confirmations.done.skipped', { count: 2 }))
    expect(message).toContain(t('logbook.confirmations.done.confirmations', { count: 1 }))
    expect(message).not.toContain(t('logbook.confirmations.done.uploads', { count: 0 }))
  })

  it('drops a check that lands after Cancel without a word', async () => {
    let fail: (e: unknown) => void = () => {}
    const { dialog, onClose } = await opened()
    check.mockReturnValue(new Promise((_, r) => { fail = r }))
    fireEvent.click(checkButton(dialog))
    fireEvent.click(within(dialog).getByRole('button', { name: t('logbook.confirmations.cancel') }))
    expect(onClose).toHaveBeenCalledTimes(1)
    expect(cancel).toHaveBeenCalledTimes(1)
    await act(async () => fail('This check was cancelled, so nothing was kept from it.'))
    expect(toast).not.toHaveBeenCalled()
  })

  it('says why a check failed, and Check can be pressed again', async () => {
    const { dialog } = await opened()
    check.mockRejectedValue('Set your LoTW username in Settings first.')
    fireEvent.click(checkButton(dialog))
    await waitFor(() => expect(toast).toHaveBeenCalledTimes(1))
    expect(toast).toHaveBeenCalledWith(
      `${t('logbook.confirmations.failed')}: Set your LoTW username in Settings first.`,
      'error',
    )
    expect(checkButton(dialog).disabled).toBe(false)
    expect(within(dialog).queryByRole('list')).toBeNull()
  })

  it('says so when nothing needs taking off, and Apply still adds what LoTW confirms', async () => {
    const { dialog } = await checked({ ...FOUND, lines: [], gains: 2 })
    fireEvent.click(applyButton(dialog, 2))
    await waitFor(() => expect(apply).toHaveBeenCalledWith(7, []))
    cleanup()
    const none = await checked({ ...FOUND, lines: [], gains: 0 })
    expect(applyButton(none.dialog, 0).disabled).toBe(true)
  })

  it('keeps the check when the operator clicks outside the box', async () => {
    const { dialog, onClose } = await checked()
    fireEvent.click(dialog)
    expect(onClose).not.toHaveBeenCalled()
    expect(cancel).not.toHaveBeenCalled()
    expect(rows(dialog).length).toBe(LINES.length)
  })
})
