// @vitest-environment jsdom
//
// ONE-TIME favorites seeding + status honesty in the Birds list.
//
// The operator's ask has two halves and this file pins both:
//
//  ① "if we are going to set some favorites, they should be set to the most
//     recent active birds, one time, then allow users to change from there."
//     So: it fires once on a genuinely empty first run, it says out loud that
//     it happened, every ★ it set is editable in place, and NOTHING re-seeds
//     — an operator who clears the set gets an empty sky, not a resurrection.
//
//  ② A ★ bird that goes dead, goes silent, or loses its elements must SAY so
//     in the list the operator stars from. Before this, existence was
//     elements-only and status was not rendered here at all: a re-entered
//     bird looked exactly like a working one, and a bird nothing carried
//     elements for simply vanished from the list with no row and no reason.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor, within } from '@testing-library/react'
import { SatellitesView } from './SatellitesView'
import type { SatDetail, SatPass, SatTrackStatus, SatView } from '../types'

const api = vi.hoisted(() => ({
  getSatellites: vi.fn((): Promise<SatView | null> => Promise.resolve(null)),
  getSatSchedule: vi.fn((): Promise<SatPass[]> => Promise.resolve([])),
  getSatPassNeeds: vi.fn((): Promise<SatPass[]> => Promise.resolve([])),
  getSatDetail: vi.fn((): Promise<SatDetail | null> => Promise.resolve(null)),
  getSettings: vi.fn(),
  setSettings: vi.fn((_s: unknown) => Promise.resolve({} as never)),
  setSatTransponder: vi.fn(() => Promise.resolve()),
  getSatTransponder: vi.fn(
    (): Promise<import('../types').SatTransponderHeld | null> => Promise.resolve(null),
  ),
  startSatTrack: vi.fn((): Promise<SatTrackStatus | null> => Promise.resolve(null)),
  stopSatTrack: vi.fn(() => Promise.resolve()),
  getSatTrackStatus: vi.fn((): Promise<SatTrackStatus | null> => Promise.resolve(null)),
  fetchTlesNow: vi.fn(),
}))
vi.mock('../api', () => api)
vi.mock('./MapView', () => ({ MapView: () => null }))
vi.mock('../toast', () => ({ pushToast: vi.fn() }))

const NOW = Math.floor(Date.now() / 1000)

type Bird = SatView['birds'][number]
const bird = (name: string, norad: number, over: Partial<Bird> = {}): Bird => ({
  name,
  norad,
  lat: 0,
  lon: 0,
  altKm: 500,
  footprintKm: 2000,
  track: [],
  status: 'alive',
  amateur: true,
  ...over,
})

const pass = (name: string, norad: number, maxElDeg = 45, n = 0): SatPass => ({
  name,
  norad,
  aosUnix: NOW + 600 + n * 6000,
  losUnix: NOW + 1200 + n * 6000,
  maxElDeg,
  aosAzDeg: 100,
  losAzDeg: 260,
  status: 'alive',
})

const view = (over: Partial<SatView> = {}): SatView => ({
  tleAgeDays: 1,
  tleFetchedAt: NOW - 3600,
  tleSource: 'mirror',
  birds: [],
  passes: [],
  excluded: [],
  ...over,
})

/** Three workable birds; SO-50 flies the most passes, RS-44 the highest. */
const seedable = () =>
  view({
    birds: [bird('SO-50', 27607), bird('RS-44', 44909), bird('AO-7', 7530)],
    passes: [
      pass('SO-50', 27607, 45, 0),
      pass('SO-50', 27607, 30, 1),
      pass('RS-44', 44909, 70, 0),
      pass('AO-7', 7530, 20, 0),
    ],
  })

const settings = (over: Record<string, unknown> = {}) => ({
  mygrid: 'EN52',
  rotatorModel: 0,
  rotatorHost: '',
  satDoppler: false,
  satVfoMap: 'off',
  ...over,
})

beforeEach(() => {
  localStorage.clear()
  api.getSatellites.mockReset()
  api.getSatellites.mockImplementation(() => Promise.resolve(seedable()))
  api.getSatSchedule.mockReset()
  api.getSatSchedule.mockImplementation(() => Promise.resolve([]))
  api.getSatPassNeeds.mockReset()
  api.getSatPassNeeds.mockImplementation(() => Promise.resolve([]))
  api.getSatDetail.mockReset()
  api.getSatDetail.mockImplementation(() => Promise.resolve(null))
  api.getSettings.mockReset()
  api.getSettings.mockImplementation(() => Promise.resolve(settings()))
  api.getSatTrackStatus.mockReset()
  api.getSatTrackStatus.mockImplementation(() => Promise.resolve(null))
})
afterEach(cleanup)

/** The Birds-list row for a bird, or undefined. */
const birdRow = (container: HTMLElement, name: string) =>
  Array.from(container.querySelectorAll('.sats-favmgr li')).find((li) =>
    li.querySelector('.sat-pick')?.textContent?.trim() === name,
  ) as HTMLElement | undefined

const starOn = (container: HTMLElement, name: string) =>
  !!birdRow(container, name)?.querySelector('.sat-star.on')

describe('one-time favorites seeding', () => {
  it('stars the ranked active birds on an empty first run and says it did', async () => {
    const { container } = render(<SatellitesView />)
    await waitFor(() => expect(starOn(container, 'SO-50')).toBe(true))
    expect(starOn(container, 'RS-44')).toBe(true)
    expect(starOn(container, 'AO-7')).toBe(true)
    expect(JSON.parse(localStorage.getItem('nexus.sats.chasing') as string).sort()).toEqual([
      'AO-7',
      'RS-44',
      'SO-50',
    ])
    // Visible: a one-line notice naming the count, not a silent mutation.
    expect(screen.getByText(/Starred 3 active birds/)).toBeTruthy()
    // Undoable in place: the row's own ★ turns it back off.
    fireEvent.click(birdRow(container, 'AO-7')!.querySelector('.sat-star') as HTMLElement)
    await waitFor(() => expect(starOn(container, 'AO-7')).toBe(false))
  })

  it('the notice is dismissible and stays dismissed across a remount', async () => {
    render(<SatellitesView />)
    const notice = await screen.findByText(/Starred 3 active birds/)
    expect(notice).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: /dismiss/i }))
    await waitFor(() => expect(screen.queryByText(/Starred 3 active birds/)).toBeNull())
    cleanup()
    render(<SatellitesView />)
    await waitFor(() => expect(api.getSatellites).toHaveBeenCalled())
    expect(screen.queryByText(/Starred 3 active birds/)).toBeNull()
  })

  it('NEVER re-seeds — an operator who unstars everything gets an empty sky', async () => {
    const { container } = render(<SatellitesView />)
    await waitFor(() => expect(starOn(container, 'SO-50')).toBe(true))
    for (const n of ['SO-50', 'RS-44', 'AO-7']) {
      fireEvent.click(birdRow(container, n)!.querySelector('.sat-star') as HTMLElement)
    }
    await waitFor(() => expect(starOn(container, 'SO-50')).toBe(false))
    cleanup()
    const second = render(<SatellitesView />)
    await waitFor(() => expect(birdRow(second.container, 'SO-50')).toBeTruthy())
    expect(starOn(second.container, 'SO-50')).toBe(false)
    expect(localStorage.getItem('nexus.sats.chasing')).toBe('[]')
  })

  it('never seeds without a grid — and the deferred seed still fires once one is set', async () => {
    api.getSettings.mockImplementation(() => Promise.resolve(settings({ mygrid: '' })))
    // No grid = no observer = no passes, which is what the backend returns.
    api.getSatellites.mockImplementation(() =>
      Promise.resolve(view({ birds: seedable().birds, passes: [] })),
    )
    const { container } = render(<SatellitesView />)
    await waitFor(() => expect(birdRow(container, 'SO-50')).toBeTruthy())
    expect(starOn(container, 'SO-50')).toBe(false)
    expect(localStorage.getItem('nexus.sats.seeded')).toBeNull() // deferred, not spent
    // The operator sets a grid; the next launch seeds.
    cleanup()
    api.getSettings.mockImplementation(() => Promise.resolve(settings()))
    api.getSatellites.mockImplementation(() => Promise.resolve(seedable()))
    const second = render(<SatellitesView />)
    await waitFor(() => expect(starOn(second.container, 'SO-50')).toBe(true))
  })

  it('never seeds over an operator who already has ★ birds', async () => {
    localStorage.setItem('nexus.sats.chasing', JSON.stringify(['AO-7']))
    const { container } = render(<SatellitesView />)
    await waitFor(() => expect(birdRow(container, 'SO-50')).toBeTruthy())
    expect(starOn(container, 'AO-7')).toBe(true)
    expect(starOn(container, 'SO-50')).toBe(false)
    expect(screen.queryByText(/to get you started/)).toBeNull()
  })

  it('never stars a dead or silent bird', async () => {
    api.getSatellites.mockImplementation(() =>
      Promise.resolve(
        view({
          birds: [
            bird('AO-85', 40967, { status: 'dead' }),
            bird('QUIET-1', 90001, { amateur: false }),
            bird('SO-50', 27607),
          ],
          passes: [
            pass('AO-85', 40967, 80),
            pass('QUIET-1', 90001, 75),
            pass('SO-50', 27607, 40),
          ],
        }),
      ),
    )
    const { container } = render(<SatellitesView />)
    await waitFor(() => expect(starOn(container, 'SO-50')).toBe(true))
    expect(starOn(container, 'AO-85')).toBe(false)
    expect(starOn(container, 'QUIET-1')).toBe(false)
  })
})

describe('status honesty in the Birds list', () => {
  it('marks a dead bird in place — never hides it', async () => {
    api.getSatellites.mockImplementation(() =>
      Promise.resolve(
        view({
          birds: [bird('AO-85', 40967, { status: 'dead' }), bird('SO-50', 27607)],
          passes: [pass('SO-50', 27607, 40)],
        }),
      ),
    )
    const { container } = render(<SatellitesView />)
    await waitFor(() => expect(birdRow(container, 'AO-85')).toBeTruthy())
    expect(within(birdRow(container, 'AO-85')!).getByText('dead')).toBeTruthy()
    expect(birdRow(container, 'SO-50')!.querySelector('.sat-chip')).toBeNull()
  })

  it('marks an alive bird whose amateur transmitters have all gone quiet', async () => {
    api.getSatellites.mockImplementation(() =>
      Promise.resolve(
        view({ birds: [bird('QUIET-1', 90001, { amateur: false })], passes: [] }),
      ),
    )
    const { container } = render(<SatellitesView />)
    await waitFor(() => expect(birdRow(container, 'QUIET-1')).toBeTruthy())
    expect(within(birdRow(container, 'QUIET-1')!).getByText('silent')).toBeTruthy()
  })

  it('gives a ★ bird with NO elements a row that says exactly that', async () => {
    // The bird is in the amateur catalog and starred, but nothing carries
    // current elements — so it has no position, no passes, and used to have
    // no row anywhere in the app.
    localStorage.setItem('nexus.sats.chasing', JSON.stringify(['AO-85']))
    api.getSatellites.mockImplementation(() =>
      Promise.resolve(
        view({
          birds: [bird('SO-50', 27607)],
          passes: [pass('SO-50', 27607, 40)],
          excluded: [
            { name: 'AO-85', norad: 40967, status: 'alive', reason: 'noElements' },
            // NOT starred — the operator's list must not grow by 280 rows.
            { name: 'PACSAT', norad: 20439, status: 'alive', reason: 'noElements' },
          ],
        }),
      ),
    )
    const { container } = render(<SatellitesView />)
    await waitFor(() => expect(birdRow(container, 'AO-85')).toBeTruthy())
    expect(within(birdRow(container, 'AO-85')!).getByText('no elements')).toBeTruthy()
    expect(starOn(container, 'AO-85')).toBe(true)
    expect(birdRow(container, 'PACSAT')).toBeUndefined()
  })

  it('a ★ bird that STOPPED being workable keeps its row, wearing both facts', async () => {
    // The production shape: the mirror publishes elements only for birds that
    // are alive AND still transmitting, so one that dies or goes quiet arrives
    // only in `excluded` — carrying the catalog's status and its transmitter
    // answer. Both belong on the row: "no elements" alone would not say that
    // there is nothing left to work even if we had them.
    localStorage.setItem('nexus.sats.chasing', JSON.stringify(['IO-117', 'AO-85']))
    api.getSatellites.mockImplementation(() =>
      Promise.resolve(
        view({
          birds: [bird('SO-50', 27607)],
          passes: [pass('SO-50', 27607, 40)],
          excluded: [
            { name: 'IO-117', norad: 53106, status: 'dead', amateur: true, reason: 'noElements' },
            { name: 'AO-85', norad: 40967, status: 'alive', amateur: false, reason: 'noElements' },
          ],
        }),
      ),
    )
    const { container } = render(<SatellitesView />)
    await waitFor(() => expect(birdRow(container, 'IO-117')).toBeTruthy())
    expect(within(birdRow(container, 'IO-117')!).getByText('dead')).toBeTruthy()
    expect(within(birdRow(container, 'IO-117')!).getByText('no elements')).toBeTruthy()
    expect(within(birdRow(container, 'AO-85')!).getByText('silent')).toBeTruthy()
    expect(starOn(container, 'AO-85')).toBe(true) // and it is still unstarrable
  })

  it('a SEARCH finds an unplaceable bird — the ★ stays a two-way door', async () => {
    // Unstarring an element-less bird removes its only row, so without this
    // the star was one-way: nothing left anywhere to click again.
    api.getSatellites.mockImplementation(() =>
      Promise.resolve(
        view({
          birds: [bird('SO-50', 27607)],
          passes: [pass('SO-50', 27607, 40)],
          excluded: [{ name: 'PACSAT', norad: 20439, status: 'alive', reason: 'noElements' }],
        }),
      ),
    )
    const { container } = render(<SatellitesView />)
    await waitFor(() => expect(birdRow(container, 'SO-50')).toBeTruthy())
    expect(birdRow(container, 'PACSAT')).toBeUndefined() // not starred: hidden
    fireEvent.change(screen.getByPlaceholderText('search…'), { target: { value: 'PACSAT' } })
    await waitFor(() => expect(birdRow(container, 'PACSAT')).toBeTruthy())
    expect(within(birdRow(container, 'PACSAT')!).getByText('no elements')).toBeTruthy()
    fireEvent.click(birdRow(container, 'PACSAT')!.querySelector('.sat-star') as HTMLElement)
    await waitFor(() => expect(starOn(container, 'PACSAT')).toBe(true))
  })

  it('says which KIND of element gap it is — stale is not the same as missing', async () => {
    localStorage.setItem('nexus.sats.chasing', JSON.stringify(['OLD-1', 'DECAY-1']))
    api.getSatellites.mockImplementation(() =>
      Promise.resolve(
        view({
          birds: [bird('SO-50', 27607)],
          passes: [pass('SO-50', 27607, 40)],
          excluded: [
            { name: 'OLD-1', norad: 90010, status: 'alive', reason: 'staleElements' },
            { name: 'DECAY-1', norad: 90011, status: 'alive', reason: 'noPosition' },
          ],
        }),
      ),
    )
    const { container } = render(<SatellitesView />)
    await waitFor(() => expect(birdRow(container, 'OLD-1')).toBeTruthy())
    expect(within(birdRow(container, 'OLD-1')!).getByText('stale elements')).toBeTruthy()
    expect(within(birdRow(container, 'DECAY-1')!).getByText('no position')).toBeTruthy()
  })
})
