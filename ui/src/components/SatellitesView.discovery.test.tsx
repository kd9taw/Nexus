// @vitest-environment jsdom
//
// The discovery band + the navigation primitive (the schedule/favorites/next
// rethink, 2026-08). What is pinned:
//
//  - PIN (a): the band renders ZERO rows while collapsed — the calm default is
//    today's screen plus one chip — and real per-bird rows when expanded.
//  - PIN (b): starring a discovery row PROMOTES the bird — the ★ write re-keys
//    the favourites fetch and the bird appears in the schedule above (earn,
//    alarms, 48 h reach), leaving the band.
//  - Default-state exceptions: open by default at zero favourites (the band IS
//    the answer there); collapsed ALWAYS at sm/xs, where the page scroller
//    owns the column and twelve extra rows push the Birds list further away.
//  - The gate ladder: only !gridSet short-circuits now; an empty ★ set is an
//    inline line inside the schedule, not a branch that replaces the column.
//  - Discovery rows carry NO earn chips, NO status chip, NO ⏰ — each absence
//    is a documented refusal, not an omission (satDiscovery.ts header).
//  - The detail's ✕ / Escape close: setSelected(null) exists at last; closing
//    never stops a track.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
import { SatellitesView } from './SatellitesView'
import type { SatDetail, SatPass, SatTrackStatus, SatView } from '../types'

const api = vi.hoisted(() => ({
  getSatellites: vi.fn((): Promise<SatView | null> => Promise.resolve(null)),
  getSatSchedule: vi.fn((): Promise<SatPass[]> => Promise.resolve([])),
  getSatPassNeeds: vi.fn((_names: string[], _hours: number): Promise<SatPass[]> =>
    Promise.resolve([]),
  ),
  getSatDetail: vi.fn((_n: string): Promise<SatDetail | null> => Promise.resolve(null)),
  getSettings: vi.fn(),
  setSettings: vi.fn(() => Promise.resolve({} as never)),
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

const pass = (name: string, norad: number, maxElDeg = 45, aosOffsetMin = 30): SatPass => ({
  name,
  norad,
  aosUnix: NOW + aosOffsetMin * 60,
  losUnix: NOW + aosOffsetMin * 60 + 600,
  maxElDeg,
  aosAzDeg: 10,
  losAzDeg: 190,
  status: 'alive',
})

/** Two ★ birds (RS-44, AO-91) + two non-★ discovery candidates. */
const theView = (): SatView => ({
  tleAgeDays: 1,
  usableCount: 300,
  agingCount: 0,
  heldBackCount: 0,
  tleFetchedAt: NOW,
  tleSource: 'mirror',
  birds: [
    bird('RS-44', 44909),
    bird('AO-91', 43017),
    bird('CAS-4B', 42759),
    bird('JO-97', 43803),
  ],
  passes: [
    pass('RS-44', 44909, 62, 12),
    pass('AO-91', 43017, 30, 10),
    pass('CAS-4B', 42759, 71, 45),
    pass('JO-97', 43803, 28, 90),
  ],
  excluded: [],
})

const settings = () => ({
  mygrid: 'EN52',
  rotatorModel: 0,
  rotatorHost: '',
  satDoppler: false,
  satVfoMap: 'off',
})

const detail = (name: string): SatDetail => ({
  name,
  norad: 44909,
  status: 'alive',
  transmitters: [],
  dataFetchedAt: null,
  pass: null,
  passTrack: [],
})

const favRows = (c: HTMLElement) => Array.from(c.querySelectorAll('.sats-sched tbody.fav tr'))
const discRows = (c: HTMLElement) => Array.from(c.querySelectorAll('.sats-sched tbody.more tr'))

beforeEach(() => {
  localStorage.clear()
  localStorage.setItem('nexus.sats.chasing', JSON.stringify(['RS-44', 'AO-91']))
  document.documentElement.removeAttribute('data-viewport')
  api.getSatellites.mockReset()
  api.getSatellites.mockImplementation(() => Promise.resolve(theView()))
  api.getSatPassNeeds.mockReset()
  api.getSatPassNeeds.mockImplementation((names: string[]) =>
    Promise.resolve(
      [pass('AO-91', 43017, 30, 10), pass('RS-44', 44909, 62, 12)].filter((p) =>
        names.includes(p.name),
      ),
    ),
  )
  api.getSatDetail.mockReset()
  api.getSatDetail.mockImplementation((n: string) => Promise.resolve(detail(n)))
  api.getSettings.mockReset()
  api.getSettings.mockImplementation(() => Promise.resolve(settings()))
  api.getSatTrackStatus.mockReset()
  api.getSatTrackStatus.mockImplementation(() => Promise.resolve(null))
})
afterEach(() => {
  cleanup()
  document.documentElement.removeAttribute('data-viewport')
})

describe('PIN (a): the discovery band is collapsed by default and renders zero rows', () => {
  it('renders no discovery rows collapsed, real bird rows expanded, and the live count on the chip', async () => {
    const { container } = render(<SatellitesView />)
    await waitFor(() => expect(favRows(container).length).toBe(2))
    // Collapsed: ZERO rows below the ★ band — not hidden rows, none at all.
    expect(discRows(container).length).toBe(0)
    const chip = screen.getByRole('button', { name: /other birds overhead/i })
    // The live count: CAS-4B and JO-97 both clear the 10° floor.
    expect(chip.textContent).toMatch(/2 workable/)
    expect(chip.getAttribute('aria-expanded')).toBe('false')
    fireEvent.click(chip)
    await waitFor(() =>
      expect(container.querySelectorAll('.sats-sched tbody.more tr.sats-disc').length).toBe(2),
    )
    // Worth order: CAS-4B (71°) above JO-97 (28°).
    const rows = Array.from(container.querySelectorAll('.sats-sched tbody.more tr.sats-disc'))
    expect(rows[0].textContent).toMatch(/CAS-4B/)
    expect(rows[1].textContent).toMatch(/JO-97/)
    // Same controls as the schedule: ☆ and ▶ Work on every row.
    expect(rows[0].querySelector('.sat-star')).toBeTruthy()
    expect(rows[0].querySelector('.sat-track')?.textContent).toMatch(/Work/)
    // The documented refusals: no earn chips, no status chip, no ⏰ below the ★ line.
    expect(rows[0].querySelector('.need-chip')).toBeNull()
    expect(rows[0].querySelector('.sat-chip')).toBeNull()
    expect(rows[0].querySelector('.sat-bell')).toBeNull()
    // Collapse again: the rows leave the DOM.
    fireEvent.click(chip)
    await waitFor(() => expect(discRows(container).length).toBe(0))
  })
})

describe('PIN (b): starring a discovery row promotes the bird into the schedule', () => {
  it('☆ on a discovery row puts the bird in the favourites schedule on the next data pass', async () => {
    api.getSatPassNeeds.mockImplementation((names: string[]) =>
      Promise.resolve(
        [
          pass('AO-91', 43017, 30, 10),
          pass('RS-44', 44909, 62, 12),
          pass('CAS-4B', 42759, 71, 45),
        ].filter((p) => names.includes(p.name)),
      ),
    )
    const { container } = render(<SatellitesView />)
    await waitFor(() => expect(favRows(container).length).toBe(2))
    fireEvent.click(screen.getByRole('button', { name: /other birds overhead/i }))
    const cas = await waitFor(() => {
      const r = Array.from(
        container.querySelectorAll('.sats-sched tbody.more tr.sats-disc'),
      ).find((x) => /CAS-4B/.test(x.textContent ?? ''))
      expect(r).toBeTruthy()
      return r!
    })
    fireEvent.click(cas.querySelector('.sat-star')!)
    // The ★ write re-keys the favourites fetch with the promoted bird…
    await waitFor(() =>
      expect(api.getSatPassNeeds).toHaveBeenCalledWith(['AO-91', 'CAS-4B', 'RS-44'], 48),
    )
    // …the bird appears ABOVE the ★ line…
    await waitFor(() =>
      expect(favRows(container).some((r) => /CAS-4B/.test(r.textContent ?? ''))).toBe(true),
    )
    // …and leaves the band (promotion, not duplication).
    await waitFor(() =>
      expect(
        Array.from(container.querySelectorAll('.sats-sched tbody.more tr.sats-disc')).some((r) =>
          /CAS-4B/.test(r.textContent ?? ''),
        ),
      ).toBe(false),
    )
  })
})

describe('default-state exceptions', () => {
  it('zero favourites: the schedule renders with an inline line and the band OPEN', async () => {
    localStorage.setItem('nexus.sats.chasing', '[]') // cleared set — the seed must not refire
    const { container } = render(<SatellitesView />)
    await waitFor(() =>
      expect(container.querySelectorAll('.sats-sched tbody.more tr.sats-disc').length).toBeGreaterThan(0),
    )
    // The old dead-end branch is gone: no .sats-empty, an inline ★ invitation instead.
    expect(container.querySelector('.sats-empty')).toBeNull()
    expect(container.querySelector('.sats-sched tbody.fav .sats-inline-empty')?.textContent).toMatch(
      /No ★ birds yet/,
    )
    // All four birds qualify when none is starred.
    expect(container.querySelectorAll('.sats-sched tbody.more tr.sats-disc').length).toBe(4)
  })

  it('sm/xs: collapsed always, even at zero favourites (the page scroller owns the column)', async () => {
    document.documentElement.setAttribute('data-viewport', 'sm')
    localStorage.setItem('nexus.sats.chasing', '[]')
    const { container } = render(<SatellitesView />)
    await waitFor(() =>
      expect(screen.getByRole('button', { name: /other birds overhead/i })).toBeTruthy(),
    )
    expect(discRows(container).length).toBe(0)
  })

  it('no grid: the dashed empty box short-circuits everything, unchanged', async () => {
    api.getSettings.mockImplementation(() => Promise.resolve({ ...settings(), mygrid: '' }))
    const { container } = render(<SatellitesView />)
    await waitFor(() => expect(container.querySelector('.sats-empty')).toBeTruthy())
    expect(container.querySelector('.sats-sched')).toBeNull()
  })
})

describe('the open band survives the first star (the disclosure latch)', () => {
  it('starring a discovery row at zero favourites promotes it WITHOUT collapsing the band', async () => {
    localStorage.setItem('nexus.sats.chasing', '[]') // cleared set — the seed must not refire
    const { container } = render(<SatellitesView />)
    // Zero favourites at md+: the band is open by default and all four birds qualify.
    await waitFor(() =>
      expect(container.querySelectorAll('.sats-sched tbody.more tr.sats-disc').length).toBe(4),
    )
    const cas = Array.from(container.querySelectorAll('.sats-sched tbody.more tr.sats-disc')).find(
      (r) => /CAS-4B/.test(r.textContent ?? ''),
    )!
    fireEvent.click(cas.querySelector('.sat-star')!)
    // The ★ flips favs.size 0→1. A null disclosure default that has RENDERED
    // open must behave as an explicit open — re-evaluating it here collapses
    // all twelve rows under the operator's cursor, punishing exactly the
    // promotion act the band exists for.
    expect(
      container.querySelectorAll('.sats-sched tbody.more tr.sats-disc').length,
      'the first ★ collapsed the open band mid-interaction',
    ).toBe(3)
    expect(
      screen.getByRole('button', { name: /other birds overhead/i }).getAttribute('aria-expanded'),
    ).toBe('true')
    // The operator's own toggle is still the thing that closes it.
    fireEvent.click(screen.getByRole('button', { name: /other birds overhead/i }))
    await waitFor(() => expect(discRows(container).length).toBe(0))
  })
})

describe('unstarring the LAST favourite', () => {
  it('never paints stale fav rows or Next-up from the dead schedule state', async () => {
    localStorage.setItem('nexus.sats.chasing', JSON.stringify(['RS-44']))
    api.getSatPassNeeds.mockImplementation((names: string[]) =>
      Promise.resolve([pass('RS-44', 44909, 62, 12)].filter((p) => names.includes(p.name))),
    )
    const { container } = render(<SatellitesView />)
    await waitFor(() =>
      expect(favRows(container).some((r) => /RS-44/.test(r.textContent ?? ''))).toBe(true),
    )
    await waitFor(() => expect(container.querySelector('.sats-best')).toBeTruthy())
    // The frame BETWEEN the ★ write and the schedule effect is what the
    // operator sees: opt out of act so the passive effect that rescues the
    // stale state does NOT flush inside the click. React commits the
    // discrete-event update synchronously; the effect stays queued.
    const g = globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }
    g.IS_REACT_ACT_ENVIRONMENT = false
    try {
      const star = favRows(container)
        .find((r) => /RS-44/.test(r.textContent ?? ''))!
        .querySelector('.sat-star')!
      star.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }))
      // The click's sync-lane render is scheduled in a MICROTASK (React 18
      // root scheduling); the schedule-fetch passive effect that rescues the
      // stale state goes through the macrotask Scheduler. Two microtask hops
      // land the assertions exactly ON the painted transient frame — after
      // the commit, before the rescue.
      await Promise.resolve()
      await Promise.resolve()
      expect(
        favRows(container).filter((r) => /RS-44/.test(r.textContent ?? '')).length,
        'a stale schedule row survived the unstar into the painted frame',
      ).toBe(0)
      expect(
        container.querySelector('.sats-best'),
        'Next-up rendered from stale schedule state after the last unstar',
      ).toBeNull()
      expect(
        container.querySelector('.sats-sched tbody.fav .sats-inline-empty')?.textContent,
      ).toMatch(/No ★ birds yet/)
    } finally {
      g.IS_REACT_ACT_ENVIRONMENT = true
    }
    // The settled state agrees with the frame.
    await waitFor(() =>
      expect(favRows(container).filter((r) => /RS-44/.test(r.textContent ?? '')).length).toBe(0),
    )
  })
})

describe('a pass longer than the 6 h backscan (AO-10 class)', () => {
  it('says "already up" — never the fabricated window-edge clock time', async () => {
    const v = theView()
    // The scan window opened mid-pass; the WIRE says so (aosClamped) instead
    // of leaving every consumer to guess from aosUnix's shape.
    v.birds.push(bird('PHASE-3B', 14129))
    v.passes.push({
      name: 'PHASE-3B',
      norad: 14129,
      aosUnix: NOW - 21_600,
      losUnix: NOW + 3_600,
      maxElDeg: 40,
      aosAzDeg: 120,
      losAzDeg: 240,
      status: 'alive',
      aosClamped: true,
    })
    api.getSatellites.mockImplementation(() => Promise.resolve(v))
    const { container } = render(<SatellitesView />)
    await waitFor(() => expect(favRows(container).length).toBe(2))
    fireEvent.click(screen.getByRole('button', { name: /other birds overhead/i }))
    const row = await waitFor(() => {
      const r = Array.from(
        container.querySelectorAll('.sats-sched tbody.more tr.sats-disc'),
      ).find((x) => /PHASE-3B/.test(x.textContent ?? ''))
      expect(r).toBeTruthy()
      return r!
    })
    const aosCell = row.children[2] as HTMLElement
    expect(
      aosCell.textContent,
      'the AOS cell prints a clock time the sky never saw',
    ).toMatch(/already up/)
    // And the duration states itself as a lower bound, not a measurement.
    expect((row.children[5] as HTMLElement).textContent).toMatch(/\+ m/)
  })
})

describe('the navigation primitive', () => {
  it('✕ on the detail heading closes it without stopping a track', async () => {
    const { container } = render(<SatellitesView focusSat="RS-44" />)
    await waitFor(() => expect(container.querySelector('.sats-detail')).toBeTruthy())
    fireEvent.click(screen.getByRole('button', { name: /close this bird/i }))
    await waitFor(() => expect(container.querySelector('.sats-detail')).toBeNull())
    expect(api.stopSatTrack).not.toHaveBeenCalled()
  })

  it('Escape closes the detail too', async () => {
    const { container } = render(<SatellitesView focusSat="RS-44" />)
    await waitFor(() => expect(container.querySelector('.sats-detail')).toBeTruthy())
    fireEvent.keyDown(window, { key: 'Escape' })
    await waitFor(() => expect(container.querySelector('.sats-detail')).toBeNull())
  })

  it('Escape inside an inner control stays with the control — the detail survives', async () => {
    const { container } = render(<SatellitesView focusSat="RS-44" />)
    await waitFor(() => expect(container.querySelector('.sats-detail')).toBeTruthy())
    await waitFor(() => expect(favRows(container).length).toBe(2))
    // The dossier's case: arm the RS-44 alarm so the lead <select> exists.
    const rsRow = favRows(container).find((r) => /RS-44/.test(r.textContent ?? ''))!
    fireEvent.click(rsRow.querySelector('.sat-bell')!)
    const lead = await waitFor(() => {
      const s = container.querySelector('.sat-lead')
      expect(s).toBeTruthy()
      return s!
    })
    fireEvent.keyDown(lead, { key: 'Escape' })
    expect(
      container.querySelector('.sats-detail'),
      'Escape in the alarm-lead select must not close the detail underneath it',
    ).toBeTruthy()
    // The Birds-list search too.
    fireEvent.keyDown(container.querySelector('.sats-search')!, { key: 'Escape' })
    expect(
      container.querySelector('.sats-detail'),
      'Escape in the search input must not close the detail underneath it',
    ).toBeTruthy()
    // From anywhere non-interactive it still closes.
    fireEvent.keyDown(document.body, { key: 'Escape' })
    await waitFor(() => expect(container.querySelector('.sats-detail')).toBeNull())
  })
})
