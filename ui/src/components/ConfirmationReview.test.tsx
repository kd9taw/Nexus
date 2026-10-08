// @vitest-environment jsdom
//
// "Check confirmations": each service's whole history (LoTW, eQSL, QRZ), downloaded once when the
// operator presses Check, and the contacts holding a confirmation or LoTW upload mark that the
// service's own records give another contact, or none. Each service is listed under its own
// heading as its check lands, and says why when it could not be checked. A line starts ticked only
// when the station says the evidence decides it, Apply sends only the ticked lines, and Cancel or
// Escape sends nothing and drops the held check. The api is a mock: no test here downloads anything
// or changes a log.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, cleanup, waitFor, within, act } from '@testing-library/react'
import { useState } from 'react'
import { ConfirmationReview } from './ConfirmationReview'
import {
  applyConfirmationCheck,
  cancelConfirmationCheck,
  confirmationCheck,
  startConfirmationCheck,
  type CheckedService,
  type ConfirmationApplied,
  type ConfirmationCheck,
  type ConfirmationLine,
} from '../api'
import { pushToast } from '../toast'
import { t } from '../i18n'

vi.mock('../api', () => ({
  startConfirmationCheck: vi.fn(),
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

const start = vi.mocked(startConfirmationCheck)
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

/** LoTW's, newest first, as the station sends them; a contact's confirmation line before its upload
 *  line. */
const LOTW_LINES: ConfirmationLine[] = [
  line('K1ABC', { class: 'orphan', whenUnix: at(7, 12), rowUnix: at(7, 14, 10), siblingId: null, siblingUnix: null, decisive: false, unticked: 'orphan', band: '40m', mode: 'CW' }),
  line('N0XYZ', { class: 'contradicted', whenUnix: at(6, 10), rowUnix: at(6, 10, 20), siblingId: null, siblingUnix: null, ownUnix: at(6, 10), cardHeld: true }),
  line('W1AW', { removeGranted: ['DXCC'], owedAfter: true }),
  line('W1AW', { mark: 'lotwUpload', owedAfter: true }),
  line('VE3AAA', { whenUnix: at(4, 23, 59), rowUnix: at(5, 0, 0), siblingUnix: at(5, 0, 1), decisive: false, unticked: 'tie' }),
  line('G4BBB', { whenUnix: at(3, 0), timeKnown: false, rowUnix: at(3, 11), siblingUnix: at(3, 11), decisive: false, unticked: 'dateOnly' }),
  line('KH6CCC', { mark: 'lotwUpload', whenUnix: at(2, 9), rowUnix: at(2, 9, 5), siblingUnix: at(2, 9, 5), decisive: false, unticked: 'notReplayed' }),
]
/** eQSL's: #400's pair took the 18:00 contact's card too, and a card its sender timed within 30
 *  minutes of a contact holding a paper card. */
const EQSL_LINES: ConfirmationLine[] = [
  line('W1AW', { mark: 'eqsl', rowUnix: at(5, 18, 1) }),
  line('JA1XYZ', { mark: 'eqsl', whenUnix: at(4, 10), rowUnix: at(4, 10, 20), siblingUnix: at(4, 10, 25), decisive: false, unticked: 'insideWindow', cardHeld: true }),
]
/** QRZ's: it holds #400's 06:00 contact itself, unconfirmed, and a mark the old matching could not
 *  have put on DL1ABC. */
const QRZ_LINES: ConfirmationLine[] = [
  line('W1AW', { mark: 'qrz', class: 'contradicted', rowUnix: at(5, 18), siblingUnix: at(5, 18), ownUnix: at(5, 6) }),
  line('DL1ABC', { mark: 'qrz', whenUnix: at(3, 8), rowUnix: at(3, 8, 1), siblingUnix: at(3, 8, 1), decisive: false, unticked: 'notReplayed' }),
]
const found = (service: CheckedService, lines: ConfirmationLine[], over: Partial<ConfirmationCheck> = {}): ConfirmationCheck => ({
  session: 7, service, records: lines.length, lines, gainIds: ['id-W1AW-2'], unreached: 0, outOfScope: 0, uploadsUnreached: 0, uploadsOutOfScope: 0, ...over,
})
/** Each service's check. 18:00 W1AW gains all three services' confirmations, K2GAIN LoTW's. */
const FOUND: Record<CheckedService, ConfirmationCheck | string> = {
  lotw: found('lotw', LOTW_LINES, { gainIds: ['id-W1AW-2', 'id-K2GAIN'], unreached: 12, outOfScope: 3, uploadsUnreached: 1 }),
  eqsl: found('eqsl', EQSL_LINES, { unreached: 4 }),
  qrz: found('qrz', QRZ_LINES),
}
const LINES = [...LOTW_LINES, ...EQSL_LINES, ...QRZ_LINES]
const BEFORE_FILE = '/home/op/Nexus/confirmations-before-check-20261007-210000Z.adi'
const APPLIED: ConfirmationApplied = {
  ticked: 5,
  services: [
    { service: 'lotw', confirmations: 2, gained: 2 },
    { service: 'eqsl', confirmations: 1, gained: 1 },
    { service: 'qrz', confirmations: 1, gained: 1 },
  ],
  uploads: 1,
  beforeFile: BEFORE_FILE,
}

/** Each service answers its check from `answers`: a check, or the station's words for a failure. */
function answering(answers: Partial<Record<CheckedService, ConfirmationCheck | string>> = FOUND) {
  check.mockImplementation(async (_session, service) => {
    const got = answers[service]
    if (got === undefined || typeof got === 'string') throw got ?? `${service}: no answer`
    return got
  })
}

beforeEach(() => {
  start.mockReset().mockResolvedValue(7)
  check.mockReset()
  answering()
  apply.mockReset().mockResolvedValue(APPLIED)
  cancel.mockReset().mockResolvedValue(undefined)
  toast.mockReset()
})
afterEach(cleanup)

async function opened() {
  const onClose = vi.fn()
  const onApplied = vi.fn()
  render(<ConfirmationReview open onClose={onClose} onApplied={onApplied} />)
  const dialog = await screen.findByRole('dialog', { name: t('logbook.confirmations.title') })
  return { dialog, onClose, onApplied }
}
const checkButton = (dialog: HTMLElement) =>
  within(dialog).getByRole('button', { name: t('logbook.confirmations.check') }) as HTMLButtonElement

/** Opened, and Check pressed: every service's check landed. */
async function checked(answers: Partial<Record<CheckedService, ConfirmationCheck | string>> = FOUND) {
  answering(answers)
  const o = await opened()
  fireEvent.click(checkButton(o.dialog))
  await waitFor(() => expect(check).toHaveBeenCalledTimes(3))
  await waitFor(() => expect(within(o.dialog).queryAllByRole('status')).toEqual([]))
  return o
}
const rows = (dialog: HTMLElement) => within(dialog).getAllByRole('listitem')
const boxes = (dialog: HTMLElement) =>
  rows(dialog).map((r) => within(r).getByRole('checkbox') as HTMLInputElement)
const applyButton = (dialog: HTMLElement, count: number) =>
  within(dialog).getByRole('button', { name: t('logbook.confirmations.apply', { count }) }) as HTMLButtonElement
const checking = (service: string) => t('logbook.confirmations.checking', { service })

describe('Check confirmations', () => {
  it('downloads nothing until Check is pressed, then each service at once, and says so while each downloads', async () => {
    const land: Partial<Record<CheckedService, (c: ConfirmationCheck) => void>> = {}
    check.mockImplementation((_, service) => new Promise((r) => { land[service] = r }))
    const { dialog } = await opened()
    expect(within(dialog).getByText(t('logbook.confirmations.intro'))).toBeTruthy()
    expect(start, 'opening it starts nothing').not.toHaveBeenCalled()
    expect(check, 'opening it downloads nothing').not.toHaveBeenCalled()
    fireEvent.click(checkButton(dialog))
    await waitFor(() => expect(check).toHaveBeenCalledTimes(3))
    expect(start).toHaveBeenCalledTimes(1)
    expect(check.mock.calls).toEqual([[7, 'lotw'], [7, 'eqsl'], [7, 'qrz']])
    for (const service of ['LoTW', 'eQSL', 'QRZ']) expect(within(dialog).getByText(checking(service))).toBeTruthy()
    expect(checkButton(dialog).disabled, 'one check at a time').toBe(true)
    // QRZ's lands first: listed, while the other two still download.
    await act(async () => land.qrz?.(FOUND.qrz as ConfirmationCheck))
    expect(within(dialog).queryByText(checking('QRZ'))).toBeNull()
    expect(within(dialog).getByText('DL1ABC')).toBeTruthy()
    expect(within(dialog).getByText(checking('LoTW'))).toBeTruthy()
    expect(checkButton(dialog).disabled, 'nothing to apply until every service is in').toBe(true)
    await act(async () => {
      land.lotw?.(FOUND.lotw as ConfirmationCheck)
      land.eqsl?.(FOUND.eqsl as ConfirmationCheck)
    })
    expect(within(dialog).queryAllByRole('status')).toEqual([])
    expect(applyButton(dialog, 4).disabled).toBe(false)
    expect(apply).not.toHaveBeenCalled()
  })

  it('lists each service under its own name, each line with what changes, why, and what it keeps or costs', async () => {
    const { dialog } = await checked()
    expect(within(dialog).getAllByRole('heading', { level: 3 }).map((h) => h.textContent)).toEqual(['LoTW', 'eQSL', 'QRZ'])
    const text = rows(dialog).map((r) => r.textContent ?? '')
    expect(text.length).toBe(LINES.length)
    const change = (service: string) => t('logbook.confirmations.change', { service })
    const changeUpload = t('logbook.confirmations.changeUpload')
    const owed = t('logbook.confirmations.badge.owed')
    const card = t('logbook.confirmations.badge.card')
    // The orphan: LoTW's row, at its own time, pairs with nothing in the log.
    for (const part of ['K1ABC', '2026-01-07 12:00Z', '40m CW', change('LoTW'), t('logbook.confirmations.why.orphan', { service: 'LoTW', time: '14:10' })])
      expect(text[0]).toContain(part)
    // LoTW holds this contact itself, unconfirmed; the paper card stays.
    for (const part of ['N0XYZ', change('LoTW'), t('logbook.confirmations.why.contradicted', { service: 'LoTW' }), card])
      expect(text[1]).toContain(part)
    // #400's pair: the 06:00 contact holds the 18:00 contact's confirmation, and its DXCC credit.
    for (const part of ['W1AW', '2026-01-05 06:00Z', '20m FT8', change('LoTW'), t('logbook.confirmations.why.moved', { service: 'LoTW', time: '18:00' }), t('logbook.confirmations.badge.credit', { codes: 'DXCC' })])
      expect(text[2]).toContain(part)
    for (const part of ['W1AW', changeUpload, t('logbook.confirmations.why.uploadMoved', { time: '18:00' }), owed])
      expect(text[3]).toContain(part)
    expect(text[2], 'the upload it is owed is said once, on the contact’s last line').not.toContain(owed)
    // A sibling on another UTC day is named with its date.
    expect(text[4]).toContain(t('logbook.confirmations.why.moved', { service: 'LoTW', time: '2026-01-05 00:01' }))
    // A date-only contact shows no time, and says so.
    expect(text[5]).toContain('2026-01-03')
    expect(text[5]).not.toContain('2026-01-03 00:00')
    expect(text[5]).toContain(t('logbook.confirmations.why.dateOnly'))
    expect(text[6]).toContain(t('logbook.confirmations.why.uploadMoved', { time: '09:05' }))
    // eQSL's and QRZ's lines name their service, and say what that service holds.
    for (const part of ['W1AW', change('eQSL'), t('logbook.confirmations.why.moved', { service: 'eQSL', time: '18:00' })])
      expect(text[7]).toContain(part)
    expect(text[7], 'taking eQSL’s confirmation off owes LoTW nothing').not.toContain(owed)
    expect(text[8]).toContain('JA1XYZ')
    expect(text[8], 'an eQSL confirmation never gave award credit for a card to keep').not.toContain(card)
    for (const part of ['W1AW', change('QRZ'), t('logbook.confirmations.why.contradicted', { service: 'QRZ' })])
      expect(text[9]).toContain(part)
    expect(text[10]).toContain(t('logbook.confirmations.why.moved', { service: 'QRZ', time: '08:01' }))
    // What each service leaves alone, under its list; a count of nothing is not shown.
    for (const said of [
      t('logbook.confirmations.gains', { count: 2, service: 'LoTW' }),
      t('logbook.confirmations.unreached', { count: 12, service: 'LoTW' }),
      t('logbook.confirmations.outOfScope', { count: 3, service: 'LoTW' }),
      t('logbook.confirmations.uploadsUnreached', { count: 1 }),
      t('logbook.confirmations.gains', { count: 1, service: 'eQSL' }),
      t('logbook.confirmations.unreached', { count: 4, service: 'eQSL' }),
      t('logbook.confirmations.gains', { count: 1, service: 'QRZ' }),
    ])
      expect(within(dialog).getByText(said)).toBeTruthy()
    expect(within(dialog).queryByText(t('logbook.confirmations.uploadsOutOfScope', { count: 0 }))).toBeNull()
    expect(within(dialog).queryByText(t('logbook.confirmations.unreached', { count: 0, service: 'QRZ' }))).toBeNull()
  })

  it('ticks a line only when the evidence decides it, and an unticked line says why', async () => {
    const { dialog } = await checked()
    expect(boxes(dialog).map((b) => b.checked)).toEqual(LINES.map((l) => l.decisive))
    const titles = rows(dialog).map((r) => r.querySelector('label')?.getAttribute('title') ?? null)
    expect(titles).toEqual([
      t('logbook.confirmations.unticked.orphan', { service: 'LoTW' }),
      null,
      null,
      null,
      t('logbook.confirmations.unticked.tie', { service: 'LoTW' }),
      t('logbook.confirmations.unticked.dateOnly', { service: 'LoTW' }),
      t('logbook.confirmations.unticked.uploadNotReplayed'),
      null,
      t('logbook.confirmations.unticked.insideWindowEqsl'),
      null,
      t('logbook.confirmations.unticked.notReplayedQrz'),
    ])
    // Each contact is one contact to change, however many services' lines tick it or give it a
    // confirmation: N0XYZ and W1AW ticked, and W1AW's 18:00 contact and K2GAIN gaining.
    expect(applyButton(dialog, 4).disabled).toBe(false)
  })

  it('applies only the ticked lines, each with its mark, under the check that listed them', async () => {
    const { dialog, onApplied, onClose } = await checked()
    fireEvent.click(boxes(dialog)[0]) // tick LoTW's orphan
    fireEvent.click(boxes(dialog)[2]) // untick W1AW's LoTW confirmation, keep its upload line
    fireEvent.click(boxes(dialog)[9]) // untick QRZ's W1AW line, keep eQSL's
    fireEvent.click(boxes(dialog)[10]) // tick QRZ's DL1ABC
    fireEvent.click(applyButton(dialog, 6))
    await waitFor(() => expect(apply).toHaveBeenCalledTimes(1))
    expect(apply).toHaveBeenCalledWith(7, [
      { id: 'id-K1ABC', mark: 'lotw' },
      { id: 'id-N0XYZ', mark: 'lotw' },
      { id: 'id-W1AW', mark: 'lotwUpload' },
      { id: 'id-W1AW', mark: 'eqsl' },
      { id: 'id-DL1ABC', mark: 'qrz' },
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

  it('names each service in what it made, the file the changed contacts were saved in, and how to put them back', async () => {
    const { dialog } = await checked()
    fireEvent.click(applyButton(dialog, 4))
    await waitFor(() => expect(toast).toHaveBeenCalledTimes(1))
    const [message, kind] = toast.mock.calls[0]
    expect(message).toContain(BEFORE_FILE)
    for (const said of [
      t('logbook.confirmations.done.beforeFile', { path: BEFORE_FILE }),
      t('logbook.confirmations.done.confirmations', { count: 2, service: 'LoTW' }),
      t('logbook.confirmations.done.confirmations', { count: 1, service: 'eQSL' }),
      t('logbook.confirmations.done.confirmations', { count: 1, service: 'QRZ' }),
      t('logbook.confirmations.done.uploads', { count: 1 }),
      t('logbook.confirmations.done.gained', { count: 2, service: 'LoTW' }),
      t('logbook.confirmations.done.gained', { count: 1, service: 'eQSL' }),
      t('logbook.confirmations.done.gained', { count: 1, service: 'QRZ' }),
    ])
      expect(message).toContain(said)
    expect(message).not.toContain(t('logbook.confirmations.done.skipped', { count: 0 }))
    // A change to the log, said even with pop-ups off (toast.ts, popsUpWhenOff).
    expect(kind).toBe('info')
  })

  it('says how many ticked lines were left alone because their contacts changed after the check', async () => {
    apply.mockResolvedValue({
      ...APPLIED,
      services: [
        { service: 'lotw', confirmations: 1, gained: 0 },
        { service: 'eqsl', confirmations: 0, gained: 0 },
        { service: 'qrz', confirmations: 1, gained: 0 },
      ],
      uploads: 0,
    })
    const { dialog } = await checked()
    fireEvent.click(applyButton(dialog, 4))
    await waitFor(() => expect(toast).toHaveBeenCalledTimes(1))
    const [message] = toast.mock.calls[0]
    expect(message).toContain(t('logbook.confirmations.done.skipped', { count: 3 }))
    expect(message).toContain(t('logbook.confirmations.done.confirmations', { count: 1, service: 'QRZ' }))
    expect(message).not.toContain(t('logbook.confirmations.done.confirmations', { count: 0, service: 'eQSL' }))
    expect(message).not.toContain(t('logbook.confirmations.done.uploads', { count: 0 }))
  })

  it('drops a check that lands after Cancel without a word, even once the next one has started', async () => {
    const late: ((c: ConfirmationCheck) => void)[] = []
    let fail: (e: unknown) => void = () => {}
    check.mockImplementation(
      (_, service) =>
        new Promise((resolve, reject) => {
          if (service === 'qrz') fail = reject
          else late.push(resolve)
        }),
    )
    const { dialog, onClose } = await opened()
    fireEvent.click(checkButton(dialog))
    await waitFor(() => expect(check).toHaveBeenCalledTimes(3))
    fireEvent.click(within(dialog).getByRole('button', { name: t('logbook.confirmations.cancel') }))
    expect(onClose).toHaveBeenCalledTimes(1)
    expect(cancel).toHaveBeenCalledTimes(1)
    // Checked again at once (this host keeps the dialog open): a new check, every service pending.
    start.mockResolvedValue(8)
    check.mockImplementation(() => new Promise(() => {}))
    fireEvent.click(checkButton(dialog))
    await waitFor(() => expect(check).toHaveBeenCalledTimes(6))
    // The first check's downloads land now, and its cancelled one fails: none of it is shown.
    const cancelled = 'This check was cancelled, so nothing was kept from it.'
    await act(async () => {
      for (const land of late) land(FOUND.lotw as ConfirmationCheck)
      fail(cancelled)
    })
    expect(within(dialog).queryByText('K1ABC')).toBeNull()
    expect(within(dialog).queryByText(t('logbook.confirmations.notChecked', { reason: cancelled }))).toBeNull()
    expect(within(dialog).getAllByRole('status'), 'the new check still downloads').toHaveLength(3)
    expect(toast).not.toHaveBeenCalled()
  })

  it('says why a service could not be checked, under its name, and lists the others', async () => {
    const eqsl = 'Set your eQSL username in Settings first.'
    const qrz = 'No QRZ Logbook API key stored.'
    const { dialog } = await checked({ ...FOUND, eqsl, qrz })
    expect(within(dialog).getByText(t('logbook.confirmations.notChecked', { reason: eqsl }))).toBeTruthy()
    expect(within(dialog).getByText(t('logbook.confirmations.notChecked', { reason: qrz }))).toBeTruthy()
    expect(rows(dialog).length, 'LoTW is listed').toBe(LOTW_LINES.length)
    expect(toast, 'said in the dialog, not in a pop-up').not.toHaveBeenCalled()
    expect(applyButton(dialog, 4).disabled).toBe(false)
  })

  it('offers Check again when no service could be checked', async () => {
    const { dialog } = await checked({ lotw: 'Set your LoTW username in Settings first.', eqsl: 'eQSL down', qrz: 'QRZ down' })
    expect(within(dialog).getByText(t('logbook.confirmations.notChecked', { reason: 'Set your LoTW username in Settings first.' }))).toBeTruthy()
    expect(checkButton(dialog).disabled).toBe(false)
    expect(within(dialog).queryByRole('list')).toBeNull()
    answering()
    fireEvent.click(checkButton(dialog))
    await waitFor(() => expect(start).toHaveBeenCalledTimes(2))
    await within(dialog).findByText('K1ABC')
  })

  it('says so when nothing needs taking off, and Apply still adds what the services confirm', async () => {
    const empty = { lotw: found('lotw', [], { gainIds: ['id-W1AW-2'] }), eqsl: found('eqsl', [], { gainIds: ['id-W1AW-2', 'id-K2GAIN'] }), qrz: found('qrz', [], { gainIds: [] }) }
    const { dialog } = await checked(empty)
    expect(within(dialog).getByText(t('logbook.confirmations.none'))).toBeTruthy()
    expect(within(dialog).getByText(t('logbook.confirmations.noneService', { service: 'eQSL' }))).toBeTruthy()
    expect(within(dialog).getByText(t('logbook.confirmations.noneService', { service: 'QRZ' }))).toBeTruthy()
    fireEvent.click(applyButton(dialog, 2))
    await waitFor(() => expect(apply).toHaveBeenCalledWith(7, []))
    cleanup()
    const none = await checked({ lotw: found('lotw', [], { gainIds: [] }), eqsl: found('eqsl', [], { gainIds: [] }), qrz: found('qrz', [], { gainIds: [] }) })
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
