// @vitest-environment jsdom
//
// THE TORN-OFF NEEDED BOARD KEEPS THE NEW GRID BAND SCOPE, AS THE DOCKED ONE DOES. Report on a release
// candidate: "I am now seeing hf grids in my needed when I dont have the filter chip selected". The board
// opens in its own window at launch by default, and that window — and the dashboard window's Needed box,
// which shares its board — took the engine's rows as they came, so with New grid at VHF+ in Settings ▸
// Spots & Alerts every 20 m grid was listed with All lit. The docked board has kept the scope since
// `boardNeeds` (features/needs.ts); App.neededGridScope.test.tsx reads it against the same list.
// THE REAL NeededPanel and ConnectView are mounted; only the backend is stubbed.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, waitFor, fireEvent, screen } from '@testing-library/react'
import type { AppSnapshot, NeedAlert, Settings } from './types'

const snapshot = {
  mycall: 'KD9TAW', mygrid: 'EN52', mode: 'Normal', activePeer: null, hunt: null, logTick: 1,
  radio: { dialMhz: 14.074, band: '20m', catOk: true, sideband: 'USB', transmitting: false, txEnabled: false, txAllowed: true },
  link: { tier: 'FT8' }, stations: [],
} as unknown as AppSnapshot

const backend = vi.hoisted(() => ({ needs: [] as unknown[], settings: null as unknown }))

vi.mock('./api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) {
    auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => null) : actual[k]
  }
  return {
    ...auto,
    subscribeSnapshot: vi.fn((cb: (s: AppSnapshot) => void) => {
      cb(snapshot)
      return () => {}
    }),
    selectPeer: vi.fn(async () => snapshot),
    getBandPlan: vi.fn(async () => []),
    getPropagation: vi.fn(async () => null),
    getNeedAlerts: vi.fn(async () => backend.needs),
    getSettings: vi.fn(async () => backend.settings),
    getAllSpots: vi.fn(async () => []),
    getOtaSpots: vi.fn(async () => []),
    getActivation: vi.fn(async () => ({ program: null, reference: null, qsoCount: 0 })),
    parksCount: vi.fn(async () => 0),
    huntedParksCount: vi.fn(async () => 0),
    getBandOutlook: vi.fn(async () => ({ bands: [], asOf: 0 })),
    getGettingOut: vi.fn(async () => null),
    getSpaceWxScales: vi.fn(async () => ({ scales: null, alerts: [] })),
    getKc2gMuf: vi.fn(async () => []),
    getDxpedWindows: vi.fn(async () => []),
    getOtaMapSpots: vi.fn(async () => []),
    getPotaActivator: vi.fn(async () => ({ calls: [], parks: [] })),
    getContests: vi.fn(async () => []),
    getWindowBehind: vi.fn(async () => ({ supported: false, on: false })),
    workSpot: vi.fn(async () => snapshot),
    setHuntTarget: vi.fn(async () => snapshot),
  }
})
vi.mock('./toast', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))

import { DetachedPanel } from './DetachedPanel'
import { setHuntTarget, workSpot } from './api'
import { pastTheSwitch } from './components/ConnectView.testkit'
import { REPORT_BOARD, REPORT_SETTINGS, ROWS, boardRows } from './neededGridScope.testkit'

// THE BUDGET: DetachedPanel.connectBoards.test.tsx's, for the same mounts (15 s; a hang still fails).
vi.setConfig({ testTimeout: 15_000 })

const rowsOf = (...calls: string[]): NeedAlert[] => ROWS.filter((r) => calls.includes(r.call))

beforeEach(() => {
  localStorage.clear()
  sessionStorage.clear()
  pastTheSwitch()
  backend.needs = ROWS
  backend.settings = REPORT_SETTINGS
  // The CW cockpit switched off: the board's own mode chips decide its CW rows, not the feature.
  localStorage.setItem('nexus.features.v1', JSON.stringify({ profile: 'custom', enabled: { cw: false, phone: true } }))
  vi.mocked(setHuntTarget).mockClear()
  vi.mocked(workSpot).mockClear()
  globalThis.ResizeObserver = class { observe() {} unobserve() {} disconnect() {} } as unknown as typeof ResizeObserver
})
afterEach(() => {
  cleanup()
  window.history.replaceState(null, '', '/')
})

describe('the torn-off Needed board keeps the New grid band scope', () => {
  it('the Needed window, All lit: no 20 m row for its grid alone, and every other reason as the docked board lists it', async () => {
    render(<DetachedPanel panel="needed" />)
    await waitFor(() => expect(boardRows(document)).toEqual([...REPORT_BOARD].sort()))
  })

  it("the dashboard window's Needed box lists the same rows", async () => {
    localStorage.setItem(
      'nexus.connect.config',
      JSON.stringify({
        slots: { left1: 'needed', left2: 'bandTiles', right1: 'pota', right2: 'outlook', bottom1: 'openings', bottom2: 'spacewx', bottom3: 'getout' },
        overlays: {},
      }),
    )
    window.history.replaceState(null, '', '/?panel=connect')
    render(<DetachedPanel panel="connect" />)
    await waitFor(() => {
      const box = document.querySelector('.pane-frame[data-pane="needed"]')
      expect(box, 'the Needed box never mounted').not.toBeNull()
      expect(boardRows(box!)).toEqual([...REPORT_BOARD].sort())
    })
  })

  // CONTROL — the scope is the operator's: one who keeps New grid on all bands gets the 20 m grid row.
  it('New grid scoped to all bands: the 20 m grid row is listed', async () => {
    backend.needs = rowsOf('K1GRD', 'EA8TWO')
    backend.settings = { ...REPORT_SETTINGS, alertGridBands: 'all' } as Settings
    render(<DetachedPanel panel="needed" />)
    await waitFor(() => expect(boardRows(document)).toEqual(['EA8TWO NewGrid NewBand', 'K1GRD NewGrid']))
  })

  // CONTROL — the park row the scope took the grid off is the same park row: its Work hands the park
  // to the Phone log line and hunts nothing, as DetachedPanel.neededPark.test.tsx holds for any park row.
  it('a Phone park row in a new 20 m grid still hands its park to the log line, and hunts nothing', async () => {
    render(<DetachedPanel panel="needed" />)
    const park = () =>
      screen.getAllByRole('row').find((r) => r.getAttribute('aria-label')?.startsWith('K9ABC,')) as HTMLElement
    await waitFor(() => expect(park()?.getAttribute('aria-label')).toMatch(/needed NewPark Pota$/))
    fireEvent.click(park())
    await waitFor(() =>
      expect(workSpot).toHaveBeenCalledWith('phone', 14.285, '20m', 'K9ABC', undefined, {
        program: 'POTA',
        reference: 'US-1000',
      }),
    )
    expect(setHuntTarget, 'a Needed click set a hunt').not.toHaveBeenCalled()
  })
})

// CONTROL — the need-type chips were never what listed the 20 m grids: a row whose only reason is a grid
// shows only while New grid is on, and a row with two reasons shows while either one is. 6 m rows, so the
// band scope admits every grid here and only the chips decide.
describe('the need-type chips on the torn-off board', () => {
  it('a grid-only row shows only while New grid is on; a grid-and-band row while either is', async () => {
    backend.needs = rowsOf('VK9XQ', 'K2SIX', 'W5BTH', 'K1CW')
    render(<DetachedPanel panel="needed" />)
    const all = ['K1CW NewBand', 'K2SIX NewGrid', 'VK9XQ NewEntity', 'W5BTH NewGrid NewBand']
    await waitFor(() => expect(boardRows(document)).toEqual(all))
    const chip = (word: string) => {
      const group = document.querySelector('.np-filters .np-filter-group') as HTMLElement
      const btn = [...group.querySelectorAll('button')].find((b) => b.textContent === word)
      expect(btn, `no "${word}" chip`).toBeTruthy()
      fireEvent.click(btn!)
    }
    fireEvent.click(document.querySelector('.np-filter-toggle') as HTMLElement)
    chip('New band')
    expect(boardRows(document), 'New band only').toEqual(['K1CW NewBand', 'W5BTH NewGrid NewBand'])
    chip('New grid')
    expect(boardRows(document), 'New band and New grid').toEqual(['K1CW NewBand', 'K2SIX NewGrid', 'W5BTH NewGrid NewBand'])
    chip('New band')
    expect(boardRows(document), 'New grid only').toEqual(['K2SIX NewGrid', 'W5BTH NewGrid NewBand'])
    chip('ATNO')
    chip('New grid')
    expect(boardRows(document), 'ATNO only').toEqual(['VK9XQ NewEntity'])
    chip('All')
    expect(boardRows(document), 'All again').toEqual(all)
  })
})
