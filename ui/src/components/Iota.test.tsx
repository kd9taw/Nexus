// @vitest-environment jsdom
//
// IOTA ON SCREEN: the island list ships with Nexus (IOTA Ltd's permission) and is on by default, so
// a name shows with no question asked; every island name shown carries IOTA Ltd's credit and a
// link; with the list off a reference is checked by its shape alone; and a log form's IOTA box shows
// the reference the station filled it with, where it came from, and lets the operator clear it.
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest'
import { render, screen, fireEvent, cleanup, waitFor } from '@testing-library/react'
import { IotaCell, IotaPrefillField, IotaReferenceNote, IotaListSettings, iotaInvalid, iotaToLog, type IotaPrefillState } from './Iota'
import { iotaKnown, resetIotaGroupsForTest, IOTA_SITE, IOTA_LIST_EVENT } from '../features/iotaGroups'
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

describe('the filled IOTA box', () => {
  const offer = { reference: 'EU-901', source: 'spot' as const }
  const state = (taken: boolean): IotaPrefillState => ({
    offer,
    taken: taken ? offer : null,
    edit: vi.fn(),
    clear: vi.fn(),
    reset: vi.fn(),
  })

  it('shows the reference, its group, and where it came from while it is the station’s', () => {
    resetIotaGroupsForTest(GROUPS, status(true, 1))
    const s = state(true)
    render(<IotaPrefillField state={s} value="EU-901" />)
    expect((screen.getByTitle(t('iota.prefill.hint')) as HTMLInputElement).value).toBe('EU-901')
    expect(screen.getByText(t('iota.offer.fromSpot'))).toBeTruthy()
    expect(screen.getByText('Example Isles group')).toBeTruthy()
    fireEvent.change(screen.getByTitle(t('iota.prefill.hint')), { target: { value: 'eu-902' } })
    expect(s.edit).toHaveBeenCalledWith('EU-902')
    fireEvent.click(screen.getByRole('button', { name: t('iota.offer.dismiss') }))
    expect(s.clear).toHaveBeenCalled()
  })

  it('a reference the operator changed says no source', () => {
    resetIotaGroupsForTest(GROUPS, status(true, 1))
    render(<IotaPrefillField state={state(false)} value="EU-902" />)
    expect(screen.queryByText(t('iota.offer.fromSpot'))).toBeNull()
  })

  it('a reference in any obvious spelling is logged in its one spelling; anything else is invalid, not empty', () => {
    expect(iotaToLog(' eu-901 ')).toBe('EU-901')
    expect(iotaToLog('EU-9')).toBe('EU-009')
    expect(iotaToLog('')).toBeNull()
    expect(iotaInvalid('')).toBe(false)
    expect(iotaInvalid('eu 9')).toBe(false)
    expect(iotaToLog('XX-901')).toBeNull()
    expect(iotaInvalid('XX-901')).toBe(true)
  })
})

describe("the Logbook's IOTA cell", () => {
  it('shows the group name beside the reference, with the credit on hover', () => {
    resetIotaGroupsForTest(GROUPS, status(true, 1))
    const { container } = render(<IotaCell reference="EU-901" />)
    expect(container.textContent).toBe('EU-901 Example Isles group')
    expect(container.querySelector('.log-cell')?.getAttribute('title')).toContain(t('iota.credit.title'))
  })

  it('without the list, or for a group it lacks, the reference alone; none, a dash', () => {
    resetIotaGroupsForTest(null, status(false))
    expect(render(<IotaCell reference="EU-901" />).container.textContent).toBe('EU-901')
    cleanup()
    resetIotaGroupsForTest(GROUPS, status(true, 1))
    expect(render(<IotaCell reference="EU-999" />).container.textContent).toBe('EU-999')
    cleanup()
    expect(render(<IotaCell reference={null} />).container.textContent).toBe('—')
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

  it('on with no list in use yet says so, not "Off"', () => {
    resetIotaGroupsForTest(null, status(true, 0))
    render(<IotaListSettings />)
    expect(screen.getByRole('switch').getAttribute('aria-checked')).toBe('true')
    expect(screen.getByText(t('settings.iotaList.none'), { exact: false })).toBeTruthy()
    expect(screen.queryByText(t('settings.iotaList.empty'), { exact: false })).toBeNull()
  })

  it("the station's first download reaches the open window without a restart", async () => {
    let heard: (() => void) | null = null
    const listen = vi.fn(async (name: string, cb: () => void) => {
      if (name === IOTA_LIST_EVENT) heard = cb
      return () => {}
    })
    vi.stubGlobal('__TAURI__', { event: { listen } })
    try {
      resetIotaGroupsForTest()
      api.getIotaListStatus.mockResolvedValue(status(true, 0))
      api.getIotaGroups.mockResolvedValue([])
      render(<IotaListSettings />)
      await screen.findByText(t('settings.iotaList.none'), { exact: false })
      expect(heard).not.toBeNull()
      // CONTROL: nothing is read again until the station says so.
      expect(api.getIotaGroups).toHaveBeenCalledTimes(1)
      api.getIotaListStatus.mockResolvedValue(status(true, 1, false))
      api.getIotaGroups.mockResolvedValue(GROUPS)
      heard!()
      await screen.findByText(t('settings.iotaList.status', { count: '1', date: '2026-10-29' }), { exact: false })
      expect(iotaKnown('EU-999')).toBe(false) // the list is the check now
    } finally {
      vi.unstubAllGlobals()
    }
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
