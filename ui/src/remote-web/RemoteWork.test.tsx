// @vitest-environment jsdom
// Remote parity batch 1: Work a spot from the Spots board, the Needed board, the map and the
// DXpedition board. The browser tunes the station (and sets its mode and FT8/FT4 tier); it never
// starts a QSO or enables transmit. These pin which rows are workable from a browser.
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import type { ReactNode } from 'react'
import { SpotsPanel } from '../components/SpotsPanel'
import { NeededPanel } from '../components/NeededPanel'
import { RemoteDxpeditions } from './RemoteDxpeditions'
import { RemoteCollections, RemoteCollectionsContext } from './collections'
import { StationControlContext, StationDataContext } from '../stationAccess'
import type { ApplicationClient } from './application-client'
import type { QueryPage } from './application-query-protocol'
import type { NeedAlert, SpotRow } from '../types'
import { remoteWorkable, remoteWorkTier, spotNeed } from './remote-work'
import fixture from './__fixtures__/dxpeditions.json'

const originalScroll = Element.prototype.scrollIntoView
beforeEach(() => { Element.prototype.scrollIntoView = vi.fn() })
afterEach(() => { cleanup(); vi.restoreAllMocks(); localStorage.clear(); Element.prototype.scrollIntoView = originalScroll })

const browser = (children: ReactNode) => <StationControlContext.Provider value={false}>{children}</StationControlContext.Provider>
const grants = (over: Partial<Parameters<typeof remoteWorkable>[2]> = {}) =>
  ({ workSpot: true, workDigitalSpot: true, cwEnabled: true, phoneEnabled: true, ...over })

const spot = (over: Partial<SpotRow>): SpotRow => ({
  call: 'JA2DEF', entity: 'Japan', zone: 25, band: '20m', freqMhz: 14.0765, mode: 'Digital', submode: 'FT8',
  spotter: 'K3LR', corroborators: [], ageSecs: 30, comment: '', licensed: true, ...over,
}) as SpotRow
const need = (over: Partial<NeedAlert>): NeedAlert => ({
  call: 'VK9XYZ', entity: 'Christmas Island', band: '20m', zone: 29, tags: [], priority: 1, headline: 'New one',
  mode: 'FT8', freqMhz: 14.0765, ...over,
}) as NeedAlert

it('admits CW and Phone under workSpot and only FT8/FT4 under workDigitalSpot', () => {
  expect(remoteWorkable(need({ mode: 'FT8' }), [], grants())).toBe(true)
  expect(remoteWorkable(need({ mode: 'FT4' }), [], grants())).toBe(true)
  expect(remoteWorkable(need({ mode: 'CW', freqMhz: 14.025 }), [], grants())).toBe(true)
  expect(remoteWorkable(need({ mode: 'Phone', freqMhz: 14.250 }), [], grants())).toBe(true)
  // Digital needs without a named FT8/FT4 tier have no remote transaction.
  for (const mode of ['Digital', 'JS8', 'FT2', 'RTTY', 'MSK144']) expect(remoteWorkable(need({ mode }), [], grants()), mode).toBe(false)
  // Each hint admits only its own rows: an older desktop without workDigitalSpot keeps CW/Phone.
  expect(remoteWorkable(need({ mode: 'FT8' }), [], grants({ workDigitalSpot: false }))).toBe(false)
  expect(remoteWorkable(need({ mode: 'CW', freqMhz: 14.025 }), [], grants({ workDigitalSpot: false }))).toBe(true)
  expect(remoteWorkable(need({ mode: 'CW', freqMhz: 14.025 }), [], grants({ workSpot: false }))).toBe(false)
  expect(remoteWorkable(need({ mode: 'CW', freqMhz: 14.025 }), [], grants({ cwEnabled: false }))).toBe(false)
  expect(remoteWorkable(need({ mode: 'FT8', freqMhz: null, band: '17m' }), [], grants())).toBe(false)
  expect(remoteWorkTier(need({ mode: 'ft4' }))).toBe('FT4')
  expect(remoteWorkTier(need({ mode: 'Digital' }))).toBeNull()
  expect(spotNeed(spot({ submode: 'FT4' })).mode).toBe('FT4')
  expect(spotNeed(spot({ mode: 'CW', submode: undefined })).mode).toBe('CW')
})

it('works a Spots row from a browser only when the station would admit it', () => {
  const work = vi.fn()
  const view = (canWork?: (s: SpotRow) => boolean) => browser(
    <SpotsPanel spots={[spot({})]} bandPlan={[]} selectedCall={null} onSelect={() => {}} onWork={work} canWork={canWork} />)
  const first = render(view(s => remoteWorkable(spotNeed(s), [], grants())))
  fireEvent.click(screen.getByText('JA2DEF').closest('.np-row')!)
  expect(work).toHaveBeenCalledTimes(1)
  expect(work.mock.calls[0][0]).toMatchObject({ call: 'JA2DEF', submode: 'FT8' })
  // Without the hint (an older desktop), and with no predicate at all, a browser click works nothing.
  first.rerender(view(s => remoteWorkable(spotNeed(s), [], grants({ workDigitalSpot: false }))))
  fireEvent.click(screen.getByText('JA2DEF').closest('.np-row')!)
  first.rerender(view())
  fireEvent.click(screen.getByText('JA2DEF').closest('.np-row')!)
  expect(work).toHaveBeenCalledTimes(1)
})

it('works a digital Needed row from a browser with the hint and not without it', () => {
  const work = vi.fn(), qsy = vi.fn()
  const view = (digital: boolean) => browser(
    <NeededPanel alerts={[need({})]} bandPlan={[]} selectedCall={null} onQsy={qsy} onSelect={() => {}} onWork={work}
      canWork={a => remoteWorkable(a, [], grants({ workDigitalSpot: digital }))} />)
  const test = render(view(true))
  fireEvent.click(screen.getByText('VK9XYZ').closest('.np-row')!)
  expect(work).toHaveBeenCalledTimes(1)
  test.rerender(view(false))
  fireEvent.click(screen.getByText('VK9XYZ').closest('.np-row')!)
  expect(work).toHaveBeenCalledTimes(1)
  expect(qsy).not.toHaveBeenCalled()
})

const page = (): QueryPage => ({ type: 'applicationPage', requestId: crypto.randomUUID(), snapshotId: crypto.randomUUID(),
  collection: 'dxpeditions', offset: 0, total: 0, retained: 0, nextCursor: null, ageMs: 0, rows: [],
  meta: { capturedAgeMs: 0, source: structuredClone(fixture) } })

it('offers Work on the hosted DXpedition board only when App passes the station Work path', async () => {
  const client = { invoke: vi.fn(async () => page()), supports: () => true, getPhase: () => 'ready' } as unknown as ApplicationClient
  const source = new RemoteCollections(client), work = vi.fn()
  const view = (onWorkSpot?: typeof work) => <StationDataContext.Provider value={true}>
    <RemoteCollectionsContext.Provider value={source}>{browser(<RemoteDxpeditions onWorkSpot={onWorkSpot} />)}</RemoteCollectionsContext.Provider>
  </StationDataContext.Provider>
  const test = render(view(work))
  await screen.findByText('Bouvet Island')
  fireEvent.click(test.container.querySelector('.wn-work')!)
  expect(work).toHaveBeenCalledWith({ call: '3Y0TEST', band: '20m', mode: 'CW', freqMhz: null })
  // Browser-local chase flags, alarms, pop-out and show-on-map stay native-only.
  expect(test.container.querySelector('.wn-chase, .dx-map-link, .dxped-popout, .dc-alarm, .dc-chase')).toBeNull()
  test.rerender(view())
  expect(test.container.querySelector('.wn-work')).toBeNull()
})
