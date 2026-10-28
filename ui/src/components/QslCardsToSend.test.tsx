// @vitest-environment jsdom
//
// "Cards to send": the contacts queued for a paper QSL card, ticked, printed as labels — and marked
// sent ONLY once the operator says the labels printed. Printing marks nothing; "No" marks nothing;
// "Yes" marks exactly the contacts that were printed, each by the id and key it was listed with,
// with the route chosen and nothing else. The api is a mock: no test here can change a log.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, cleanup, waitFor, within } from '@testing-library/react'
import { QslCardsToSend, labelQso } from './QslCardsToSend'
import { getSettings, isTauri, markQslSentById, openQslLabels, qslCardQueue, type KeyedRow } from '../api'
import type { LoggedQso } from '../types'
import { t } from '../i18n'

vi.mock('../api', () => ({
  qslCardQueue: vi.fn(),
  markQslSentById: vi.fn(),
  openQslLabels: vi.fn(),
  isTauri: vi.fn(() => true),
  getSettings: vi.fn(),
}))
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  // As the real one: a failure is toasted and answers null.
  withErrorToast: vi.fn(async (run: () => Promise<unknown>) => {
    try {
      return await run()
    } catch {
      return null
    }
  }),
}))

const queue = vi.mocked(qslCardQueue)
const mark = vi.mocked(markQslSentById)
const openSheet = vi.mocked(openQslLabels)

function keyed(call: string, when: number, extra: [string, string][] = [['QSL_SENT', 'Q']]): KeyedRow {
  const row: LoggedQso = {
    id: `id-${call}-${when}`, call, grid: null, band: '20m', freqMhz: 14.074, mode: 'FT8',
    rstSent: '-07', rstRcvd: '-10', whenUnix: when, confirmed: false, awardConfirmed: false,
    qslSent: { sent: false, via: null, dateUnix: null }, extra,
  }
  return { row, editKey: `key-${call}-${when}` }
}
const ROWS = [keyed('W1AW', 1_700_000_000), keyed('EA8XYZ', 1_700_000_600, [['QSL_SENT', 'R'], ['QSL_VIA', 'EA8MGR']]), keyed('K2DEF', 1_700_001_200)]

beforeEach(() => {
  queue.mockReset().mockResolvedValue(ROWS)
  mark.mockReset().mockImplementation(async (target) => ({ kind: 'applied', current: { row: ROWS[0].row, editKey: target.editKey } }))
  openSheet.mockReset().mockResolvedValue(undefined)
  vi.mocked(isTauri).mockReturnValue(true)
  vi.mocked(getSettings).mockResolvedValue({ mycall: 'KD9TAW' } as Awaited<ReturnType<typeof getSettings>>)
})
afterEach(cleanup)

async function opened() {
  const onClose = vi.fn()
  const onChanged = vi.fn()
  render(<QslCardsToSend open onClose={onClose} onChanged={onChanged} />)
  const dialog = await screen.findByRole('dialog', { name: t('logbook.qslCards.title') })
  await within(dialog).findByText('W1AW')
  return { dialog, onClose, onChanged }
}
const boxOf = (dialog: HTMLElement, call: string) =>
  within(within(dialog).getByText(call).closest('label') as HTMLElement).getByRole('checkbox') as HTMLInputElement
const printButton = (dialog: HTMLElement) =>
  within(dialog).getByRole('button', { name: /^Print \d+ label/ })

async function printed(dialog: HTMLElement) {
  fireEvent.click(printButton(dialog))
  await within(dialog).findByText(t('logbook.qslCards.question'))
}

describe('Cards to send', () => {
  it('lists every queued card ticked, with its QSL route', async () => {
    const { dialog } = await opened()
    expect(['W1AW', 'EA8XYZ', 'K2DEF'].map((c) => boxOf(dialog, c).checked)).toEqual([true, true, true])
    expect(within(dialog).getByText('via EA8MGR')).toBeTruthy()
  })

  it('printing opens the sheet and marks nothing until the operator answers', async () => {
    const { dialog, onChanged } = await opened()
    fireEvent.click(boxOf(dialog, 'K2DEF'))
    await printed(dialog)
    expect(openSheet).toHaveBeenCalledTimes(1)
    const html = openSheet.mock.calls[0][0]
    expect(html).toContain('<b>W1AW</b>')
    expect(html).toContain('<b>EA8XYZ</b>')
    expect(html).not.toContain('K2DEF')
    expect(mark, 'a print alone marks nothing').not.toHaveBeenCalled()
    expect(onChanged).not.toHaveBeenCalled()
  })

  it('"No" keeps every card queued', async () => {
    const { dialog, onChanged } = await opened()
    await printed(dialog)
    fireEvent.click(within(dialog).getByRole('button', { name: t('logbook.qslCards.no') }))
    await within(dialog).findByText('W1AW')
    expect(mark).not.toHaveBeenCalled()
    expect(onChanged).not.toHaveBeenCalled()
  })

  it('"Yes" marks exactly the printed contacts sent, by id and key, via the route chosen', async () => {
    const { dialog, onChanged } = await opened()
    fireEvent.click(boxOf(dialog, 'W1AW'))
    await printed(dialog)
    // Ticking after the print changes nothing about what was printed.
    fireEvent.change(within(dialog).getByRole('combobox'), { target: { value: 'D' } })
    fireEvent.click(within(dialog).getByRole('button', { name: t('logbook.qslCards.yes', { count: 2 }) }))
    await waitFor(() => expect(onChanged).toHaveBeenCalled())
    expect(mark.mock.calls).toEqual([
      [{ id: 'id-EA8XYZ-1700000600', editKey: 'key-EA8XYZ-1700000600' }, 'D'],
      [{ id: 'id-K2DEF-1700001200', editKey: 'key-K2DEF-1700001200' }, 'D'],
    ])
    expect(queue).toHaveBeenCalledTimes(2) // listed again after marking
  })

  it('a sheet that did not open asks nothing and marks nothing', async () => {
    openSheet.mockRejectedValue(new Error('no browser'))
    const { dialog } = await opened()
    fireEvent.click(printButton(dialog))
    await waitFor(() => expect(openSheet).toHaveBeenCalled())
    expect(within(dialog).queryByText(t('logbook.qslCards.question'))).toBeNull()
    expect(mark).not.toHaveBeenCalled()
  })

  it('a label names the contact’s own call, else the operator’s', () => {
    expect(labelQso(ROWS[0].row, 'kd9taw').myCall).toBe('KD9TAW')
    expect(labelQso({ ...ROWS[0].row, stationCallsign: 'W9XYZ' }, 'KD9TAW').myCall).toBe('W9XYZ')
    expect(labelQso(ROWS[1].row, 'KD9TAW').via).toBe('EA8MGR')
  })
})
