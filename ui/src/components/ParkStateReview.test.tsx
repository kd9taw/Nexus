// @vitest-environment jsdom
//
// "Check park states": the contacts already logged whose park or summit names one state and which
// hold another, or none, for the operator to tick. Nothing is sent until Apply, only the ticked
// rows are, a confirmed contact starts unticked, and Cancel or Escape sends nothing. The api is a
// mock: no test here can change a log.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, cleanup, waitFor, within } from '@testing-library/react'
import { ParkStateReview } from './ParkStateReview'
import { applyParkStates, parkStateReview, type ParkStateRow } from '../api'
import { t } from '../i18n'

vi.mock('../api', () => ({
  parkStateReview: vi.fn(),
  applyParkStates: vi.fn(),
}))
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn((run: () => Promise<unknown>) => run()),
}))

const review = vi.mocked(parkStateReview)
const apply = vi.mocked(applyParkStates)

function row(call: string, state: string | null, parkState: string, confirmed = false): ParkStateRow {
  return {
    id: `id-${call}`, call, whenUnix: 1_700_000_000, band: '20m', mode: 'SSB',
    program: 'POTA', reference: 'US-0001', state, parkState, confirmed,
  }
}
const ROWS = [row('K1AA', 'OH', 'ND'), row('K6FF', null, 'ND'), row('K7GG', 'OH', 'ND', true)]

beforeEach(() => {
  review.mockReset()
  apply.mockReset().mockResolvedValue(0)
})
afterEach(cleanup)

async function opened(rows: ParkStateRow[]) {
  review.mockResolvedValue(rows)
  const onClose = vi.fn()
  const onApplied = vi.fn()
  render(<ParkStateReview open onClose={onClose} onApplied={onApplied} />)
  const dialog = await screen.findByRole('dialog', { name: t('logbook.parkStates.title') })
  if (rows.length) await within(dialog).findByText('K1AA')
  return { dialog, onClose, onApplied }
}
const boxOf = (dialog: HTMLElement, call: string) =>
  within(within(dialog).getByText(call).closest('label') as HTMLElement).getByRole('checkbox') as HTMLInputElement

describe('Check park states', () => {
  it('lists each contact old → new, every one ticked but the confirmed', async () => {
    const { dialog } = await opened(ROWS)
    expect(within(dialog).getAllByText(t('logbook.parkStates.change', { from: 'OH', to: 'ND' })).length).toBe(2)
    expect(within(dialog).getByText(t('logbook.parkStates.change', { from: t('logbook.parkStates.noState'), to: 'ND' }))).toBeTruthy()
    expect([boxOf(dialog, 'K1AA').checked, boxOf(dialog, 'K6FF').checked, boxOf(dialog, 'K7GG').checked]).toEqual([true, true, false])
    expect(apply, 'nothing is sent by opening it').not.toHaveBeenCalled()
  })

  it('sends only the ticked rows, each with the state it was listed with', async () => {
    apply.mockResolvedValue(1)
    const { dialog, onApplied, onClose } = await opened(ROWS)
    fireEvent.click(boxOf(dialog, 'K6FF')) // untick
    fireEvent.click(within(dialog).getByRole('button', { name: t('logbook.parkStates.apply', { count: 1 }) }))
    await waitFor(() => expect(apply).toHaveBeenCalledTimes(1))
    expect(apply).toHaveBeenCalledWith([{ id: 'id-K1AA', state: 'OH', parkState: 'ND' }])
    await waitFor(() => expect(onApplied).toHaveBeenCalled())
    expect(onClose).toHaveBeenCalled()
  })

  it('sends a confirmed contact only when the operator ticks it', async () => {
    const { dialog } = await opened(ROWS)
    fireEvent.click(boxOf(dialog, 'K7GG'))
    fireEvent.click(within(dialog).getByRole('button', { name: t('logbook.parkStates.apply', { count: 3 }) }))
    await waitFor(() => expect(apply).toHaveBeenCalledTimes(1))
    expect(apply.mock.calls[0][0].map((c) => c.id)).toEqual(['id-K1AA', 'id-K6FF', 'id-K7GG'])
  })

  it('sends nothing on Cancel or Escape', async () => {
    const { dialog, onClose } = await opened(ROWS)
    fireEvent.click(within(dialog).getByRole('button', { name: t('logbook.parkStates.cancel') }))
    fireEvent.keyDown(window, { key: 'Escape' })
    expect(onClose).toHaveBeenCalledTimes(2)
    expect(apply).not.toHaveBeenCalled()
  })

  it('says so when there is nothing to check, and offers nothing to apply', async () => {
    const { dialog } = await opened([])
    expect(await within(dialog).findByText(t('logbook.parkStates.none'))).toBeTruthy()
    expect((within(dialog).getByRole('button', { name: t('logbook.parkStates.apply', { count: 0 }) }) as HTMLButtonElement).disabled).toBe(true)
  })
})
