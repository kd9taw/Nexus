// @vitest-environment jsdom
//
// SKED WITH A STATION — the two-observer band on the Satellites section.
//
// The geometry is pinned in Rust (`crates/propagation/src/satsked.rs`, where a
// mutual window has an exact answer and the fixtures are stations far enough
// apart that a naive "both have a pass today" verdict is WRONG). This file
// pins the things only the surface can get wrong:
//
//   - the ask carries the ★ set and the typed peer, and nothing else is
//     demanded of the operator (no grid field, no bird picker, no horizon);
//   - an EMPTY answer renders as an ANSWER — the separation and the birds
//     scanned — because for most pairs on most birds "no window" is correct
//     and a blank list would read as a broken feature;
//   - a window past the element-age tier is LABELLED, not hidden, and a firm
//     one is not labelled (the assertion that can differ — a badge every row
//     carries proves nothing about the rule that puts it there);
//   - the per-station elevations reach the row, not just the shared ceiling:
//     they are the two numbers that are routinely tens of degrees apart, and a
//     surface that showed one number would hide that one end is on the
//     treeline;
//   - a backend refusal is SHOWN. Every one of them names something the
//     operator can fix in the box they just typed into.
//
// ⚠️ jsdom does not lay out: nothing here can see that the band fits, that the
// disclosure is reachable or that a row is one line. `scripts/browser-probe`
// and CI's remote-browser job own that.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
import { SatellitesView } from './SatellitesView'
import type { SatPass, SatSked, SatSkedWindow, SatView } from '../types'

const api = vi.hoisted(() => ({
  getSatellites: vi.fn((): Promise<SatView | null> => Promise.resolve(null)),
  getSatSchedule: vi.fn((): Promise<SatPass[]> => Promise.resolve([])),
  getSatPassNeeds: vi.fn((): Promise<SatPass[]> => Promise.resolve([])),
  getSatSked: vi.fn((): Promise<SatSked> => Promise.resolve(emptySked())),
  getSatDetail: vi.fn(() => Promise.resolve(null)),
  getSettings: vi.fn(() => Promise.resolve({ mygrid: 'EN52', rotatorModel: 0, satVfoMap: 'off' })),
  setSettings: vi.fn(() => Promise.resolve({} as never)),
  setSatTransponder: vi.fn(() => Promise.resolve()),
  getSatTransponder: vi.fn(() => Promise.resolve(null)),
  startSatTrack: vi.fn(() => Promise.resolve(null)),
  stopSatTrack: vi.fn(() => Promise.resolve()),
  getSatTrackStatus: vi.fn(() => Promise.resolve(null)),
}))
vi.mock('../api', () => api)
vi.mock('./MapView', () => ({ MapView: () => null }))
vi.mock('../toast', () => ({ pushToast: vi.fn() }))

const NOW = Math.floor(Date.now() / 1000)

/** A window tomorrow on fresh elements — the FIRM control. Numbers are the
 * shape the Rust fixture actually produces on a ~1,400 km pair: a short window
 * whose two ends are nothing like each other. */
const firmWindow = (over: Partial<SatSkedWindow> = {}): SatSkedWindow => ({
  satName: 'RS-44',
  norad: 44909,
  startUnix: NOW + 86_400,
  endUnix: NOW + 86_400 + 250,
  peakUnix: NOW + 86_400 + 120,
  mutualElDeg: 20.5,
  maxElHereDeg: 66.2,
  maxElThereDeg: 20.4,
  peakAzHereDeg: 315,
  elementAgeDays: 2.1,
  firm: true,
  ...over,
})

/** …and one eleven days out, by which time the elements are over the 14-day
 * tier. Same bird, same shape — the ONLY difference is the age, so a row that
 * renders the tag on both is not reading `firm`. */
const softWindow = (): SatSkedWindow =>
  firmWindow({
    satName: 'AO-91',
    norad: 43017,
    startUnix: NOW + 11 * 86_400,
    endUnix: NOW + 11 * 86_400 + 180,
    peakUnix: NOW + 11 * 86_400 + 90,
    mutualElDeg: 7.5,
    maxElHereDeg: 13.9,
    maxElThereDeg: 14.4,
    elementAgeDays: 16.4,
    firm: false,
  })

function emptySked(over: Partial<SatSked> = {}): SatSked {
  return {
    theirGrid: 'FN42',
    separationKm: 1473,
    minElDeg: 5,
    days: 14,
    birds: ['AO-91', 'RS-44'],
    windows: [],
    ...over,
  }
}

const theView = (): SatView => ({
  tleAgeDays: 1,
  usableCount: 300,
  agingCount: 0,
  heldBackCount: 0,
  tleFetchedAt: NOW,
  tleSource: 'mirror',
  birds: [
    { name: 'RS-44', norad: 44909, lat: 0, lon: 0, altKm: 500, footprintKm: 2000, track: [], status: 'alive', amateur: true },
    { name: 'AO-91', norad: 43017, lat: 0, lon: 0, altKm: 500, footprintKm: 2000, track: [], status: 'alive', amateur: true },
  ],
  passes: [],
  excluded: [],
})

beforeEach(() => {
  localStorage.clear()
  localStorage.setItem('nexus.sats.chasing', JSON.stringify(['RS-44', 'AO-91']))
  for (const m of Object.values(api)) m.mockClear()
  api.getSatellites.mockImplementation(() => Promise.resolve(theView()))
  api.getSatSked.mockImplementation(() => Promise.resolve(emptySked()))
})
afterEach(cleanup)

/** Open the band and ask about `peer`. Returns once the answer has landed. */
async function ask(peer: string) {
  render(<SatellitesView />)
  fireEvent.click(await screen.findByRole('button', { name: /Sked with a station/ }))
  fireEvent.change(screen.getByLabelText(/grid square or callsign/i), {
    target: { value: peer },
  })
  fireEvent.click(screen.getByRole('button', { name: 'Find windows' }))
  await waitFor(() => expect(api.getSatSked).toHaveBeenCalled())
}

describe('the sked band asks for one thing', () => {
  it('is collapsed until the operator opens it', async () => {
    render(<SatellitesView />)
    expect(await screen.findByRole('button', { name: /Sked with a station/ })).toBeTruthy()
    // CONTROL: the input is what appears on the disclosure, so its absence
    // here is the collapsed state and not a missing band.
    expect(screen.queryByLabelText(/grid square or callsign/i)).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: /Sked with a station/ }))
    expect(screen.getByLabelText(/grid square or callsign/i)).toBeTruthy()
  })

  it('sends the ★ set and the typed peer — and asks for nothing else', async () => {
    await ask('FN42')
    // The operator's own grid is NOT an argument: it is already known. The
    // horizon is not one either — it is the constant, not a choice.
    expect(api.getSatSked).toHaveBeenCalledWith(['AO-91', 'RS-44'], 'FN42', 14)
  })

  it('takes a callsign as readily as a grid — the backend resolves it', async () => {
    api.getSatSked.mockImplementation(() =>
      Promise.resolve(emptySked({ theirGrid: 'FN31PR', windows: [firmWindow()] })),
    )
    await ask('w1aw')
    expect(api.getSatSked).toHaveBeenCalledWith(['AO-91', 'RS-44'], 'w1aw', 14)
    // …and the square the answer is ABOUT is echoed back, because the operator
    // never typed it and is entitled to check it.
    expect(screen.getByTestId('sat-sked-note').textContent).toMatch(/FN31PR/)
  })

  it('does not ask on an empty box', async () => {
    render(<SatellitesView />)
    fireEvent.click(await screen.findByRole('button', { name: /Sked with a station/ }))
    const go = screen.getByRole('button', { name: 'Find windows' }) as HTMLButtonElement
    expect(go.disabled).toBe(true)
    fireEvent.click(go)
    expect(api.getSatSked).not.toHaveBeenCalled()
  })
})

describe('an empty answer is an answer', () => {
  it('says what was searched and how far apart the pair is', async () => {
    await ask('IO91')
    const empty = await screen.findByTestId('sat-sked-empty')
    expect(empty.textContent).toMatch(/No window with FN42/)
    // The note is what stops "no windows" reading as "it did not look".
    const note = screen.getByTestId('sat-sked-note').textContent ?? ''
    expect(note).toMatch(/1473 km/)
    expect(note).toMatch(/2 ★ birds/)
    expect(note).toMatch(/5° at BOTH ends/)
  })

  it('renders no rows at all — the empty state is not a header over a list', async () => {
    const { container } = render(<SatellitesView />)
    fireEvent.click(await screen.findByRole('button', { name: /Sked with a station/ }))
    fireEvent.change(screen.getByLabelText(/grid square or callsign/i), {
      target: { value: 'IO91' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Find windows' }))
    await screen.findByTestId('sat-sked-empty')
    expect(container.querySelectorAll('.sats-sked-row').length).toBe(0)
  })
})

describe('the rows', () => {
  const both = () => emptySked({ windows: [firmWindow(), softWindow()] })

  it('carry the shared ceiling AND both stations own elevations', async () => {
    api.getSatSked.mockImplementation(() => Promise.resolve(both()))
    const { container } = render(<SatellitesView />)
    fireEvent.click(await screen.findByRole('button', { name: /Sked with a station/ }))
    fireEvent.change(screen.getByLabelText(/grid square or callsign/i), {
      target: { value: 'FN42' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Find windows' }))
    await waitFor(() => expect(container.querySelectorAll('.sats-sked-row').length).toBe(2))

    const rs = Array.from(container.querySelectorAll('.sats-sked-row')).find((r) =>
      /RS-44/.test(r.textContent ?? ''),
    )!
    expect(rs.querySelector('.sats-sked-el')!.textContent).toBe('21°')
    // 66° here against 20° there — the asymmetry a single number would hide.
    expect(rs.querySelector('.sats-sked-ends')!.textContent).toBe('66° / 20°')
    expect(rs.querySelector('.sats-sked-az')!.textContent).toBe('NW')
    expect(rs.querySelector('.sats-sked-dur')!.textContent).toBe('4m')
  })

  it('labels the row whose elements will be stale, and only that row', async () => {
    api.getSatSked.mockImplementation(() => Promise.resolve(both()))
    const { container } = render(<SatellitesView />)
    fireEvent.click(await screen.findByRole('button', { name: /Sked with a station/ }))
    fireEvent.change(screen.getByLabelText(/grid square or callsign/i), {
      target: { value: 'FN42' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Find windows' }))
    await waitFor(() => expect(container.querySelectorAll('.sats-sked-row').length).toBe(2))

    const rows = Array.from(container.querySelectorAll('.sats-sked-row'))
    const firm = rows.find((r) => /RS-44/.test(r.textContent ?? ''))!
    const soft = rows.find((r) => /AO-91/.test(r.textContent ?? ''))!
    // BOTH directions — a tag on every row would prove nothing about `firm`.
    expect(firm.querySelector('.sats-sked-soft')).toBeNull()
    expect(soft.querySelector('.sats-sked-soft')!.textContent).toBe('re-check')
    expect(soft.className).toMatch(/\bsoft\b/)
    // The tooltip names the age, so "re-check" is a reason and not a mood.
    expect(soft.querySelector('.sats-sked-soft')!.getAttribute('title')).toMatch(/16 days old/)
  })

  it('drops a window that has already ended', async () => {
    api.getSatSked.mockImplementation(() =>
      Promise.resolve(
        emptySked({
          windows: [
            firmWindow({ satName: 'GONE', startUnix: NOW - 900, endUnix: NOW - 600 }),
            firmWindow(),
          ],
        }),
      ),
    )
    const { container } = render(<SatellitesView />)
    fireEvent.click(await screen.findByRole('button', { name: /Sked with a station/ }))
    fireEvent.change(screen.getByLabelText(/grid square or callsign/i), {
      target: { value: 'FN42' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Find windows' }))
    await waitFor(() => expect(container.querySelectorAll('.sats-sked-row').length).toBe(1))
    expect(container.textContent).not.toMatch(/GONE/)
  })
})

describe('failures reach the operator', () => {
  it('shows the backend refusal instead of an empty list', async () => {
    api.getSatSked.mockImplementation(() =>
      Promise.reject('No grid on file for W1AW — enter their square instead'),
    )
    await ask('W1AW')
    const err = await screen.findByTestId('sat-sked-error')
    expect(err.textContent).toMatch(/No grid on file for W1AW/)
    // …and it does NOT also claim an empty result.
    expect(screen.queryByTestId('sat-sked-empty')).toBeNull()
  })

  it('says when there are no ★ birds to search', async () => {
    localStorage.setItem('nexus.sats.chasing', JSON.stringify([]))
    render(<SatellitesView />)
    fireEvent.click(await screen.findByRole('button', { name: /Sked with a station/ }))
    expect(screen.getByText(/Star a bird or two first/)).toBeTruthy()
  })
})
