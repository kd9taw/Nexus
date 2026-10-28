// @vitest-environment jsdom
//
// IOTA ON SCREEN (operator ruling, 2026-10-27): the island list is downloaded only with the
// operator's yes, after one line that names IOTA Ltd and what is fetched; every island name shown
// carries IOTA Ltd's credit and a link; without the list a reference is checked by its shape alone;
// and a reference is OFFERED to a contact, never written by itself.
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest'
import { render, screen, fireEvent, cleanup, waitFor } from '@testing-library/react'
import { IotaOffer, IotaReferenceNote, IotaListSettings } from './Iota'
import { iotaKnown, resetIotaGroupsForTest, IOTA_SITE } from '../features/iotaGroups'
import { iotaNeeded } from './SpotsPanel'
import { t } from '../i18n'
import type { NeedAlert } from '../types'

const api = vi.hoisted(() => ({
  setIotaListConsent: vi.fn(),
  getIotaListStatus: vi.fn(),
  getIotaGroups: vi.fn(),
}))
vi.mock('../api', () => api)

const GROUPS = [{ reference: 'EU-901', name: 'Example Isles group' }]
const status = (consent: boolean | null, count = 0) => ({ consent, count, fetchedAt: 0, lastError: null })

beforeEach(() => {
  vi.clearAllMocks()
})
afterEach(() => {
  cleanup()
  resetIotaGroupsForTest()
})

describe('the consent line', () => {
  it('asks once, naming IOTA Ltd and what is fetched, and downloads only on a yes', async () => {
    resetIotaGroupsForTest(null, status(null))
    api.setIotaListConsent.mockResolvedValue(status(true, 1))
    api.getIotaListStatus.mockResolvedValue(status(true, 1))
    api.getIotaGroups.mockResolvedValue(GROUPS)
    render(<IotaReferenceNote reference="EU-901" />)
    const line = screen.getByText(t('iota.consent.line'))
    expect(line.textContent).toContain('IOTA Ltd')
    expect(line.textContent).toContain('iota-world.org')
    expect(api.setIotaListConsent).not.toHaveBeenCalled()
    fireEvent.click(screen.getByRole('button', { name: t('iota.consent.yes') }))
    await waitFor(() => expect(api.setIotaListConsent).toHaveBeenCalledWith(true))
    // With the list held, the name shows with IOTA Ltd's credit and a link to its site.
    await screen.findByText('Example Isles group')
    expect(screen.getByRole('link', { name: t('iota.credit') }).getAttribute('href')).toBe(IOTA_SITE)
  })

  it('says nothing once the operator said no', () => {
    resetIotaGroupsForTest(null, status(false))
    const { container } = render(<IotaReferenceNote reference="EU-901" />)
    expect(container.textContent).toBe('')
  })

  it('a no is a no: the download is never asked for', async () => {
    resetIotaGroupsForTest(null, status(null))
    api.setIotaListConsent.mockResolvedValue(status(false))
    api.getIotaListStatus.mockResolvedValue(status(false))
    api.getIotaGroups.mockResolvedValue([])
    render(<IotaReferenceNote reference="EU-901" />)
    fireEvent.click(screen.getByRole('button', { name: t('iota.consent.no') }))
    await waitFor(() => expect(api.setIotaListConsent).toHaveBeenCalledWith(false))
    expect(api.setIotaListConsent).not.toHaveBeenCalledWith(true)
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
  it('turning the list off deletes it through the consent verb', async () => {
    resetIotaGroupsForTest(GROUPS, status(true, 1))
    api.setIotaListConsent.mockResolvedValue(status(false))
    api.getIotaListStatus.mockResolvedValue(status(false))
    api.getIotaGroups.mockResolvedValue([])
    render(<IotaListSettings />)
    const sw = screen.getByRole('switch')
    expect(sw.getAttribute('aria-checked')).toBe('true')
    fireEvent.click(sw)
    await waitFor(() => expect(api.setIotaListConsent).toHaveBeenCalledWith(false))
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
