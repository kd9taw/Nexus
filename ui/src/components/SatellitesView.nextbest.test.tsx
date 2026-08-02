// @vitest-environment jsdom
//
// The Next/Best strip (operator ruling, 2026-08 field report: "'Next up'
// sometimes contains entries that are further away than what's in the
// schedule" — the strip ranked by pass QUALITY while claiming to be next, and
// read favorites-only). The ruling: TWO labelled pairs — "Next" = the 2
// soonest workable passes by AOS (clock order), "Best 24 h" = the 2 highest
// passScore passes — over ALL admitted birds, ★ marking favourites, the mode
// pill on every classified row.
//
// The four pins:
//  (a) a sooner non-★ pass outranks a later ★ pass in Next — the strip is the
//      source for current birds, not a favourites echo.
//  (b) Best is passScore order and never duplicates a Next row — a pass in
//      both ranks renders once (in Next); Best backfills with the next-ranked.
//  (c) a bird the discovery admission refuses (positively dead, placeholder
//      name) never appears in either group — ONE admission rule, never two.
//  (d) in-progress passes lead Next, with the backscan's already-up honesty —
//      never the scan-window edge dressed as a rise time.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, fireEvent, waitFor } from '@testing-library/react'
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

const pass = (
  name: string,
  norad: number,
  maxElDeg: number,
  aosOffsetMin: number,
  over: Partial<SatPass> = {},
): SatPass => ({
  name,
  norad,
  aosUnix: NOW + aosOffsetMin * 60,
  losUnix: NOW + aosOffsetMin * 60 + 600,
  maxElDeg,
  aosAzDeg: 10,
  losAzDeg: 190,
  ...over,
})

const mkView = (birds: Bird[], passes: SatPass[]): SatView => ({
  tleAgeDays: 1,
  usableCount: 300,
  agingCount: 0,
  heldBackCount: 0,
  tleFetchedAt: NOW,
  tleSource: 'mirror',
  birds,
  passes,
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
  norad: null,
  status: 'alive',
  transmitters: [],
  dataFetchedAt: null,
  pass: null,
  passTrack: [],
})

/** The strip's labelled groups, in render order. */
const groups = (c: HTMLElement) => Array.from(c.querySelectorAll('.sats-best-group'))
const groupRows = (g: Element) => Array.from(g.querySelectorAll('.sats-best-row'))
const stripText = (c: HTMLElement) => c.querySelector('.sats-best')?.textContent ?? ''

beforeEach(() => {
  localStorage.clear()
  document.documentElement.removeAttribute('data-viewport')
  api.getSatellites.mockReset()
  api.getSatPassNeeds.mockReset()
  api.getSatPassNeeds.mockImplementation(() => Promise.resolve([]))
  api.getSatDetail.mockReset()
  api.getSatDetail.mockImplementation((n: string) => Promise.resolve(detail(n)))
  api.getSettings.mockReset()
  api.getSettings.mockImplementation(() => Promise.resolve(settings()))
  api.startSatTrack.mockReset()
  api.startSatTrack.mockImplementation(() => Promise.resolve(null))
  api.getSatTrackStatus.mockReset()
  api.getSatTrackStatus.mockImplementation(() => Promise.resolve(null))
})
afterEach(() => {
  cleanup()
  document.documentElement.removeAttribute('data-viewport')
})

describe('PIN (a): Next is clock order over ALL birds — a sooner non-★ pass outranks a later ★ pass', () => {
  it('puts the unstarred sooner pass first, with the pill, no ★, no earn — and the same ▶ Work chain', async () => {
    localStorage.setItem('nexus.sats.chasing', JSON.stringify(['RS-44']))
    api.getSatellites.mockImplementation(() =>
      Promise.resolve(
        mkView(
          [bird('RS-44', 44909), bird('CAS-4B', 42759, { classes: ['linear'] })],
          [
            pass('RS-44', 44909, 62, 60), // ★, later, higher quality
            pass('CAS-4B', 42759, 20, 10), // non-★, sooner, lower quality
          ],
        ),
      ),
    )
    // The favourites schedule row carries the stamped earn for RS-44.
    api.getSatPassNeeds.mockImplementation(() =>
      Promise.resolve([
        pass('RS-44', 44909, 62, 60, {
          status: 'alive',
          earn: { newGrids: 2, gridSample: ['EN12'], newEntities: 0, entitySample: [], score: 2 },
        }),
      ]),
    )
    const { container } = render(<SatellitesView />)
    await waitFor(() => expect(groups(container).length).toBeGreaterThan(0))
    const next = groups(container)[0]
    expect(next.querySelector('h2')?.textContent).toMatch(/^Next$/)
    const rows = groupRows(next)
    expect(rows.length).toBe(2)
    expect(rows[0].textContent, 'the sooner non-★ pass must lead Next').toMatch(/CAS-4B/)
    expect(rows[1].textContent).toMatch(/RS-44/)
    // The non-★ row: mode pill, no ★ mark, no earn chips (earn is ★-only by design).
    expect(rows[0].querySelector('.sat-mode-pill')?.textContent).toBe('Linear SSB/CW')
    expect(rows[0].querySelector('.sat-fav-mark')).toBeNull()
    expect(rows[0].querySelector('.need-chip')).toBeNull()
    // The ★ row keeps its mark and its stamped earn.
    expect(rows[1].querySelector('.sat-fav-mark')).toBeTruthy()
    expect(rows[1].querySelector('.need-chip')).toBeTruthy()
    // ▶ Work runs the same chain for the unstarred bird.
    fireEvent.click(rows[0].querySelector('.sat-work')!)
    await waitFor(() =>
      expect(api.startSatTrack).toHaveBeenCalledWith('CAS-4B', NOW + 600),
    )
  })
})

describe('PIN (b): Best is passScore order and never duplicates a Next row', () => {
  it('backfills Best past the pass already shown in Next', async () => {
    localStorage.setItem('nexus.sats.chasing', '[]')
    api.getSatellites.mockImplementation(() =>
      Promise.resolve(
        mkView(
          [
            bird('AO-7', 7530),
            bird('AO-91', 43017),
            bird('CAS-4B', 42759),
            bird('JO-97', 43803),
            bird('SO-50', 27607),
          ],
          [
            pass('AO-7', 7530, 80, 5), // Next #1 AND passScore #1
            pass('AO-91', 43017, 20, 10), // Next #2
            pass('CAS-4B', 42759, 70, 45), // passScore #2 → Best #1
            pass('JO-97', 43803, 60, 90), // passScore #3 → Best #2
            pass('SO-50', 27607, 15, 120), // admitted, outranked everywhere
          ],
        ),
      ),
    )
    const { container } = render(<SatellitesView />)
    await waitFor(() => expect(groups(container).length).toBe(2))
    const [next, best] = groups(container)
    expect(best.querySelector('h2')?.textContent).toMatch(/Best/)
    expect(groupRows(next).map((r) => r.textContent)).toEqual([
      expect.stringMatching(/AO-7\b/),
      expect.stringMatching(/AO-91/),
    ])
    // Best: passScore order (70° then 60°), with the top-scored pass (AO-7,
    // already in Next) skipped — backfilled, never rendered twice.
    expect(groupRows(best).map((r) => r.textContent)).toEqual([
      expect.stringMatching(/CAS-4B/),
      expect.stringMatching(/JO-97/),
    ])
    const all = Array.from(container.querySelectorAll('.sats-best-row'))
    expect(
      all.filter((r) => /AO-7 /.test((r.textContent ?? '') + ' ')).length,
      'the same pass rendered twice across Next and Best',
    ).toBe(1)
  })
})

describe('PIN (c): what the discovery admission refuses never appears in either group', () => {
  it('excludes a positively-dead bird and a placeholder name, however good their geometry', async () => {
    localStorage.setItem('nexus.sats.chasing', '[]')
    api.getSatellites.mockImplementation(() =>
      Promise.resolve(
        mkView(
          [
            bird('DEAD-1', 90001, { status: 'dead' }),
            bird('OBJECT D', 90002),
            bird('AO-91', 43017),
            bird('CAS-4B', 42759),
          ],
          [
            pass('DEAD-1', 90001, 89, 1), // would top both groups
            pass('OBJECT D', 90002, 88, 2), // placeholder catalog name
            pass('AO-91', 43017, 30, 10),
            pass('CAS-4B', 42759, 70, 45),
          ],
        ),
      ),
    )
    const { container } = render(<SatellitesView />)
    await waitFor(() => expect(groups(container).length).toBeGreaterThan(0))
    expect(stripText(container)).not.toMatch(/DEAD-1/)
    expect(stripText(container)).not.toMatch(/OBJECT D/)
    // The admitted birds still fill the strip — refusal excludes, never blanks.
    expect(stripText(container)).toMatch(/AO-91/)
    expect(stripText(container)).toMatch(/CAS-4B/)
  })
})

describe('PIN (d): in-progress passes lead Next with the already-up honesty', () => {
  it('a clamped in-progress pass is Next #1, saying "already up" — never the window-edge clock time', async () => {
    localStorage.setItem('nexus.sats.chasing', '[]')
    api.getSatellites.mockImplementation(() =>
      Promise.resolve(
        mkView(
          [bird('PHASE-3B', 14129), bird('AO-91', 43017), bird('CAS-4B', 42759)],
          [
            // Out-lasted the 6 h backscan: aosUnix is the scan window's edge,
            // not a rise time the sky ever saw (the wire says so: aosClamped).
            pass('PHASE-3B', 14129, 40, 0, {
              aosUnix: NOW - 21_600,
              losUnix: NOW + 3_600,
              aosClamped: true,
            }),
            pass('AO-91', 43017, 30, 10),
            pass('CAS-4B', 42759, 70, 45),
          ],
        ),
      ),
    )
    const { container } = render(<SatellitesView />)
    await waitFor(() => expect(groups(container).length).toBeGreaterThan(0))
    const rows = groupRows(groups(container)[0])
    expect(rows[0].textContent, 'the in-progress pass must lead Next').toMatch(/PHASE-3B/)
    expect(rows[0].classList.contains('live')).toBe(true)
    expect(rows[0].textContent).toMatch(/already up/)
    // Never a fabricated hh:mm rise time — and the duration is a lower bound.
    expect(rows[0].textContent).not.toMatch(/\b\d{2}:\d{2}\b/)
    expect(rows[0].textContent).toMatch(/\d+\+ min/)
  })
})
