// @vitest-environment jsdom
//
// IOTA ON SCREEN: the island list ships with Nexus (IOTA Ltd's permission) and is on by default, so
// a name shows with no question asked; every island name shown carries IOTA Ltd's credit and a
// link; with the list off a reference is checked by its shape alone; and a reference is OFFERED to
// a contact, never written by itself.
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest'
import { render, screen, fireEvent, cleanup, waitFor } from '@testing-library/react'
import { IotaOffer, IotaReferenceNote, IotaListSettings } from './Iota'
import { iotaKnown, resetIotaGroupsForTest, IOTA_SITE } from '../features/iotaGroups'
import { iotaNeeded } from './SpotsPanel'
import { t } from '../i18n'
import type { NeedAlert } from '../types'

const api = vi.hoisted(() => ({
  setIotaListEnabled: vi.fn(),
  getIotaListStatus: vi.fn(),
  getIotaGroups: vi.fn(),
}))
vi.mock('../api', () => api)

const GROUPS = [{ reference: 'EU-901', name: 'Example Isles group' }]
const SHIPPED_AT = Date.UTC(2026, 9, 29) / 1000
const status = (enabled: boolean, count = 0, bundled = true) => ({ enabled, count, fetchedAt: count ? SHIPPED_AT : 0, bundled, lastError: null })

beforeEach(() => {
  vi.clearAllMocks()
})
afterEach(() => {
  cleanup()
  resetIotaGroupsForTest()
})

describe('island names out of the box', () => {
  it('a fresh station shows the shipped name with the credit, and asks nothing', async () => {
    resetIotaGroupsForTest()
    api.getIotaListStatus.mockResolvedValue(status(true, 1))
    api.getIotaGroups.mockResolvedValue(GROUPS)
    const { container } = render(<IotaReferenceNote reference="EU-901" />)
    await screen.findByText('Example Isles group')
    expect(screen.getByRole('link', { name: t('iota.credit') }).getAttribute('href')).toBe(IOTA_SITE)
    expect(container.querySelector('button')).toBeNull()
    expect(api.setIotaListEnabled).not.toHaveBeenCalled()
  })

  it('with the list off, a reference shows bare: no name, no question', () => {
    resetIotaGroupsForTest(null, status(false))
    const { container } = render(<IotaReferenceNote reference="EU-901" />)
    expect(container.textContent).toBe('')
    expect(container.querySelector('button')).toBeNull()
  })
})

describe('the list check', () => {
  it('without the list, a reference is checked by its shape alone', () => {
    resetIotaGroupsForTest(null, status(false))
    expect(iotaKnown('EU-999')).toBe(true)
    expect(iotaKnown('EU-99')).toBe(false)
  })

  it('with the list, a reference it lacks is no island group, and says so', () => {
    resetIotaGroupsForTest(GROUPS, status(true, 1))
    expect(iotaKnown('eu-901')).toBe(true)
    expect(iotaKnown('EU-999')).toBe(false)
    render(<IotaReferenceNote reference="EU-999" />)
    expect(screen.getByRole('note').textContent).toContain(t('iota.notOnList', { ref: 'EU-999' }))
  })
})

describe('the offer', () => {
  it('names the reference and where it came from, and writes nothing unless Use is pressed', () => {
    resetIotaGroupsForTest(GROUPS, status(true, 1))
    const onAccept = vi.fn()
    const onDismiss = vi.fn()
    render(<IotaOffer offer={{ reference: 'EU-901', source: 'spot' }} onAccept={onAccept} onDismiss={onDismiss} />)
    expect(screen.getByText(t('iota.offer.fromSpot'))).toBeTruthy()
    expect(screen.getByText('Example Isles group')).toBeTruthy()
    expect(onAccept).not.toHaveBeenCalled()
    fireEvent.click(screen.getByRole('button', { name: t('iota.offer.dismiss') }))
    expect(onDismiss).toHaveBeenCalled()
    expect(onAccept).not.toHaveBeenCalled()
    fireEvent.click(screen.getByRole('button', { name: t('iota.offer.use') }))
    expect(onAccept).toHaveBeenCalledWith('EU-901')
  })
})

describe('Settings', () => {
  it('on by default with the shipped copy; turning it off hides the names and says so', async () => {
    resetIotaGroupsForTest(GROUPS, status(true, 1))
    api.setIotaListEnabled.mockResolvedValue(status(false))
    api.getIotaListStatus.mockResolvedValue(status(false))
    api.getIotaGroups.mockResolvedValue([])
    render(<IotaListSettings />)
    expect(screen.getByText(t('settings.iotaList.statusShipped', { count: '1', date: '2026-10-29' }), { exact: false })).toBeTruthy()
    const sw = screen.getByRole('switch')
    expect(sw.getAttribute('aria-checked')).toBe('true')
    fireEvent.click(sw)
    await waitFor(() => expect(api.setIotaListEnabled).toHaveBeenCalledWith(false))
    await waitFor(() => expect(screen.getByRole('switch').getAttribute('aria-checked')).toBe('false'))
    expect(screen.getByText(t('settings.iotaList.empty'), { exact: false })).toBeTruthy()
    expect(iotaKnown('EU-999')).toBe(true) // the shape is the check again
  })

  it('a copy the monthly check downloaded is shown as updated, not shipped', () => {
    resetIotaGroupsForTest(GROUPS, status(true, 1, false))
    render(<IotaListSettings />)
    expect(screen.getByText(t('settings.iotaList.status', { count: '1', date: '2026-10-29' }), { exact: false })).toBeTruthy()
  })
})

describe('Spots', () => {
  it('marks the IOTA chip needed only for the group the board needs', () => {
    const a = { call: 'IA5X', tags: ['NewIota'], iota: 'EU-028' } as unknown as NeedAlert
    expect(iotaNeeded([a], 'EU-028')).toBe(true)
    expect(iotaNeeded([a], 'EU-031')).toBe(false)
    expect(iotaNeeded([{ ...a, tags: ['Confirm'] }], 'EU-028')).toBe(false)
    expect(iotaNeeded(undefined, 'EU-028')).toBe(false)
  })
})
