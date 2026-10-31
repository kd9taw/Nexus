// @vitest-environment jsdom
//
// THE IOTA OFFER AT LOG TIME (operator decision, 2026-10-27): the station offers a reference from
// the spot comment or the callbook; the log form shows it as a chip; the operator takes it with Use,
// and only then does the contact log with it. Nothing is taken by itself, a reference the contact
// already holds is never offered over, and a new call starts over.
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest'
import { render, screen, fireEvent, cleanup, waitFor } from '@testing-library/react'
import { LogConfirm } from './LogConfirm'
import { resetIotaGroupsForTest } from '../features/iotaGroups'
import { t } from '../i18n'
import type { LoggedQso } from '../types'

const api = vi.hoisted(() => ({ getIotaOffer: vi.fn() }))
vi.mock('../api', async (orig) => ({ ...(await orig<typeof import('../api')>()), getIotaOffer: api.getIotaOffer }))

const QSO = {
  call: 'IA5X',
  grid: 'JN52',
  band: '20m',
  mode: 'FT8',
  rstSent: '-12',
  rstRcvd: '-09',
  comment: null,
  txPower: null,
  operator: null,
  myGrid: null,
  propMode: null,
} as unknown as LoggedQso

const GROUPS = [{ reference: 'EU-028', name: 'Example Isles group' }]
const status = { enabled: true, count: 1, fetchedAt: 0, bundled: true, lastError: null }

beforeEach(() => {
  vi.clearAllMocks()
  resetIotaGroupsForTest(GROUPS, status)
})
afterEach(() => {
  cleanup()
  resetIotaGroupsForTest()
})

function open(record: LoggedQso = QSO) {
  const onConfirm = vi.fn()
  render(<LogConfirm record={record} onConfirm={onConfirm} onDiscard={() => {}} />)
  return { onConfirm, log: () => fireEvent.click(screen.getByRole('button', { name: t('logPrompt.log') })) }
}

const chip = () => screen.queryByRole('group', { name: t('iota.offer.aria', { ref: 'EU-028' }) })
const use = () => screen.getByRole('button', { name: t('iota.offer.use') })

describe('the confirm-before-log popup', () => {
  it('shows the offer with its source and logs it only when Use is pressed', async () => {
    api.getIotaOffer.mockResolvedValue({ reference: 'EU-028', source: 'callbook' })
    const p = open()
    await waitFor(() => expect(chip()).toBeTruthy())
    expect(api.getIotaOffer).toHaveBeenCalledWith('IA5X')
    expect(screen.getByText(t('iota.offer.fromCallbook'))).toBeTruthy()
    expect(use().getAttribute('aria-pressed')).toBe('false')
    p.log()
    expect(p.onConfirm.mock.calls[0][1], 'nothing taken by itself').toBeNull()

    fireEvent.click(use())
    expect(use().getAttribute('aria-pressed')).toBe('true')
    p.log()
    expect(p.onConfirm.mock.calls[1][1]).toEqual({ reference: 'EU-028', source: 'callbook' })

    // Pressed again, it is given back.
    fireEvent.click(use())
    p.log()
    expect(p.onConfirm.mock.calls[2][1]).toBeNull()
  })

  it('× leaves it out', async () => {
    api.getIotaOffer.mockResolvedValue({ reference: 'EU-028', source: 'spot' })
    const p = open()
    await waitFor(() => expect(chip()).toBeTruthy())
    expect(screen.getByText(t('iota.offer.fromSpot'))).toBeTruthy()
    fireEvent.click(use())
    fireEvent.click(screen.getByRole('button', { name: t('iota.offer.dismiss') }))
    expect(chip()).toBeNull()
    p.log()
    expect(p.onConfirm.mock.calls[0][1]).toBeNull()
  })

  it('offers nothing over a reference the contact holds', async () => {
    api.getIotaOffer.mockResolvedValue({ reference: 'EU-028', source: 'spot' })
    const p = open({ ...QSO, ota: { iota: 'EU-001' } } as unknown as LoggedQso)
    await new Promise((r) => setTimeout(r, 400))
    expect(api.getIotaOffer).not.toHaveBeenCalled()
    expect(chip()).toBeNull()
    p.log()
    expect(p.onConfirm.mock.calls[0][1]).toBeNull()
  })

  it('a call changed in the popup starts over: nothing taken for the new call', async () => {
    api.getIotaOffer.mockResolvedValue({ reference: 'EU-028', source: 'spot' })
    const p = open()
    await waitFor(() => expect(chip()).toBeTruthy())
    fireEvent.click(use())
    const call = screen.getByDisplayValue('IA5X') as HTMLInputElement
    fireEvent.change(call, { target: { value: 'IA5Y' } })
    await waitFor(() => expect(api.getIotaOffer).toHaveBeenCalledWith('IA5Y'))
    await waitFor(() => expect(chip()).toBeTruthy())
    expect(use().getAttribute('aria-pressed')).toBe('false')
    p.log()
    expect(p.onConfirm.mock.calls[0][1]).toBeNull()
  })
})
