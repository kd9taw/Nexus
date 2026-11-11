// @vitest-environment jsdom
//
// THE IOTA REFERENCE IN THE POPUP (operator ruling, 2026-11-10): the reference the station names
// from a spot comment or the callbook is put in the popup's IOTA box and logged with the contact as
// the box holds it, so the operator can change or clear it first. A reference the contact already
// holds (the hold filled it) is shown and never replaced, and a new call starts over.
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

const GROUPS = [
  { reference: 'EU-028', name: 'Example Isles group' },
  { reference: 'EU-001', name: 'First Isles group' },
]
const status = { enabled: true, count: 2, fetchedAt: 0, bundled: true, lastError: null }

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

const field = () => screen.queryByRole('group', { name: t('iota.prefill.aria') })
const box = () => screen.getByTitle(t('iota.prefill.hint')) as HTMLInputElement
const logged = (p: ReturnType<typeof open>, i = 0) => (p.onConfirm.mock.calls[i][0] as LoggedQso).ota?.iota ?? null

describe('the confirm-before-log popup', () => {
  it('fills the box from the station and logs it as shown, marked by its source', async () => {
    api.getIotaOffer.mockResolvedValue({ reference: 'EU-028', source: 'callbook' })
    const p = open()
    await waitFor(() => expect(box().value).toBe('EU-028'))
    expect(api.getIotaOffer).toHaveBeenCalledWith('IA5X')
    expect(screen.getByText(t('iota.offer.fromCallbook'))).toBeTruthy()
    expect(screen.getByText('Example Isles group')).toBeTruthy()
    p.log()
    expect(logged(p), 'logged with no press').toBe('EU-028')
    expect(p.onConfirm.mock.calls[0][1]).toEqual({ reference: 'EU-028', source: 'callbook' })
  })

  it('the operator can change it: their reference is logged, not marked as the station’s', async () => {
    api.getIotaOffer.mockResolvedValue({ reference: 'EU-028', source: 'spot' })
    const p = open()
    await waitFor(() => expect(box().value).toBe('EU-028'))
    fireEvent.change(box(), { target: { value: 'eu-001' } })
    expect(box().value).toBe('EU-001')
    expect(screen.queryByText(t('iota.offer.fromSpot'))).toBeNull()
    p.log()
    expect(logged(p)).toBe('EU-001')
    expect(p.onConfirm.mock.calls[0][1]).toBeNull()
  })

  it('× clears it: the contact logs with none, and nothing fills it again', async () => {
    api.getIotaOffer.mockResolvedValue({ reference: 'EU-028', source: 'spot' })
    const p = open()
    await waitFor(() => expect(box().value).toBe('EU-028'))
    fireEvent.click(screen.getByRole('button', { name: t('iota.offer.dismiss') }))
    expect(box().value).toBe('')
    await new Promise((r) => setTimeout(r, 400))
    expect(box().value, 'not filled again').toBe('')
    p.log()
    expect(logged(p)).toBeNull()
    expect(p.onConfirm.mock.calls[0][1]).toBeNull()
  })

  it('a reference the contact holds is shown and never replaced', async () => {
    api.getIotaOffer.mockResolvedValue({ reference: 'EU-028', source: 'spot' })
    const p = open({ ...QSO, ota: { iota: 'EU-001' } } as unknown as LoggedQso)
    await new Promise((r) => setTimeout(r, 400))
    expect(field()).toBeTruthy()
    expect(box().value).toBe('EU-001')
    p.log()
    expect(logged(p)).toBe('EU-001')
    expect(p.onConfirm.mock.calls[0][1]).toBeNull()
  })

  it('no source: no box, and nothing logged', async () => {
    api.getIotaOffer.mockResolvedValue(null)
    const p = open()
    await new Promise((r) => setTimeout(r, 400))
    expect(field()).toBeNull()
    p.log()
    expect(logged(p)).toBeNull()
  })

  it('a call changed in the popup starts over: the old call’s fill goes, the new one’s comes', async () => {
    api.getIotaOffer.mockImplementation(async (call: string) =>
      call === 'IA5X' ? { reference: 'EU-028', source: 'spot' } : { reference: 'EU-001', source: 'callbook' },
    )
    const p = open()
    await waitFor(() => expect(box().value).toBe('EU-028'))
    const call = screen.getByDisplayValue('IA5X') as HTMLInputElement
    fireEvent.change(call, { target: { value: 'IA5Y' } })
    await waitFor(() => expect(api.getIotaOffer).toHaveBeenCalledWith('IA5Y'))
    await waitFor(() => expect(box().value).toBe('EU-001'))
    p.log()
    expect(logged(p)).toBe('EU-001')
    expect(p.onConfirm.mock.calls[0][1]).toEqual({ reference: 'EU-001', source: 'callbook' })
  })

  // REVIEW REPRO: production's popup opens with the hold's fill (marked as Nexus's). A corrected
  // call must not log the busted call's group.
  const FILLED = {
    ...QSO,
    ota: { iota: 'EU-028' },
    extra: [['APP_NEXUS_OFFER_HELD', 'IOTA']],
  } as unknown as LoggedQso

  it('a call corrected in the popup lets go of the hold’s fill: none when the new call has none', async () => {
    api.getIotaOffer.mockImplementation(async (call: string) => (call === 'IA5X' ? { reference: 'EU-028', source: 'spot' } : null))
    const p = open(FILLED)
    expect(box().value, 'premise: opened filled').toBe('EU-028')
    fireEvent.change(screen.getByDisplayValue('IA5X'), { target: { value: 'IA5Y' } })
    await waitFor(() => expect(api.getIotaOffer).toHaveBeenCalledWith('IA5Y'))
    await new Promise((r) => setTimeout(r, 50))
    p.log()
    expect(logged(p), 'not the other station’s group').toBeNull()
  })

  it('…and the corrected call’s own when it has one', async () => {
    api.getIotaOffer.mockImplementation(async (call: string) =>
      call === 'IA5X' ? { reference: 'EU-028', source: 'spot' } : { reference: 'EU-001', source: 'callbook' },
    )
    const p = open(FILLED)
    fireEvent.change(screen.getByDisplayValue('IA5X'), { target: { value: 'IA5Y' } })
    await waitFor(() => expect(box().value).toBe('EU-001'))
    p.log()
    expect(logged(p)).toBe('EU-001')
    expect(p.onConfirm.mock.calls[0][1]).toEqual({ reference: 'EU-001', source: 'callbook' })
  })

  it('a reference the contact holds that Nexus did not fill stays through a call correction', async () => {
    api.getIotaOffer.mockResolvedValue(null)
    const p = open({ ...QSO, ota: { iota: 'EU-001' } } as unknown as LoggedQso)
    fireEvent.change(screen.getByDisplayValue('IA5X'), { target: { value: 'IA5Y' } })
    await waitFor(() => expect(api.getIotaOffer).toHaveBeenCalledWith('IA5Y'))
    p.log()
    expect(logged(p)).toBe('EU-001')
  })


  // A BADLY TYPED REFERENCE IS NEVER DROPPED SILENTLY: an obvious spelling is logged in the log's
  // own; anything else says so beside the box and holds Log.
  it('an obvious spelling is logged as the reference it is', async () => {
    api.getIotaOffer.mockResolvedValue(null)
    const p = open({ ...QSO, ota: { iota: 'EU-001' } } as unknown as LoggedQso)
    fireEvent.change(box(), { target: { value: 'eu 5' } })
    expect(screen.queryByRole('alert')).toBeNull()
    p.log()
    expect(logged(p)).toBe('EU-005')
  })

  it('anything else says it is not a reference and holds Log', async () => {
    api.getIotaOffer.mockResolvedValue(null)
    const p = open({ ...QSO, ota: { iota: 'EU-001' } } as unknown as LoggedQso)
    fireEvent.change(box(), { target: { value: 'XX-001' } })
    expect(screen.getByRole('alert').textContent).toBe(t('logbook.form.iotaInvalid'))
    const log = screen.getByRole('button', { name: t('logPrompt.log') }) as HTMLButtonElement
    expect(log.disabled).toBe(true)
    p.log()
    expect(p.onConfirm).not.toHaveBeenCalled()
    // Cleared, it logs with none, as the operator chose.
    fireEvent.click(screen.getByRole('button', { name: t('iota.offer.dismiss') }))
    expect(screen.queryByRole('alert')).toBeNull()
    p.log()
    expect(logged(p)).toBeNull()
  })


  it('a malformed answer ({}) fills nothing and the popup still logs', async () => {
    api.getIotaOffer.mockResolvedValue({})
    const p = open()
    await waitFor(() => expect(api.getIotaOffer).toHaveBeenCalledWith('IA5X'))
    await new Promise((r) => setTimeout(r, 50))
    expect(field()).toBeNull()
    p.log()
    expect(logged(p)).toBeNull()
    expect(p.onConfirm.mock.calls[0][1]).toBeNull()
  })

})
