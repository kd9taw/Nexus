// @vitest-environment jsdom
//
// Phase 2 — the needs-aware layer on the Satellites section (spec §3, litigation top-5 ⑤)
// plus the uplink-sideband display and the working-state ordering (top-5 ②).
//
// What is pinned:
//
//  - The schedule is fetched through get_sat_pass_needs, so rows arrive with `earn`
//    stamped (needed grids / entities reachable through the pass footprint).
//  - Earn renders as the app's ONE need-chip vocabulary (NEED_CHIP: "NEW ONE"/"GRID",
//    the --need-* palette) — the Satellites section reads like the Needed board, not a
//    second dialect. A pass with nothing to earn shows NO chips: absent, not zero.
//  - "Needed" is a SORT KEY the operator clicks — never a silent reorder. Default
//    order stays soonest-AOS.
//  - The pass timeline is Phase 2's visual home: the armed pass's earn renders as a
//    lane under the rail (sample grids + "+N more", counts complete).
//  - Uplink sideband: what the radio's TX (split) VFO will be commanded to is SHOWN in
//    the Doppler readout and the transponder chooser — display only, engine owns the
//    command. No claim when the legs share a mode (nothing is commanded).
//  - Working-state order: passband strip and transponder chooser sit TOGETHER, above
//    the globe — the two controls used together are never separated by a hemisphere.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor, within } from '@testing-library/react'
import { SatellitesView } from './SatellitesView'
import type { SatDetail, SatPass, SatPassEarn, SatTrackStatus, SatView } from '../types'

const api = vi.hoisted(() => ({
  getSatellites: vi.fn((): Promise<SatView | null> => Promise.resolve(null)),
  getSatSchedule: vi.fn((): Promise<SatPass[]> => Promise.resolve([])),
  getSatPassNeeds: vi.fn((): Promise<SatPass[]> => Promise.resolve([])),
  getSatDetail: vi.fn(),
  getSettings: vi.fn(),
  setSettings: vi.fn(() => Promise.resolve({} as never)),
  setSatTransponder: vi.fn(() => Promise.resolve()),
  getSatTransponder: vi.fn((): Promise<import('../types').SatTransponderHeld | null> => Promise.resolve(null)),
  startSatTrack: vi.fn((): Promise<SatTrackStatus | null> => Promise.resolve(null)),
  stopSatTrack: vi.fn(() => Promise.resolve()),
  getSatTrackStatus: vi.fn((): Promise<SatTrackStatus | null> => Promise.resolve(null)),
}))
vi.mock('../api', () => api)
vi.mock('./MapView', () => ({ MapView: () => null }))
vi.mock('../toast', () => ({ pushToast: vi.fn() }))

const NOW = Math.floor(Date.now() / 1000)
const AOS = NOW + 720
const LOS = NOW + 1500

const earn: SatPassEarn = {
  newGrids: 5,
  gridSample: ['EN12', 'EN13'],
  newEntities: 1,
  entitySample: ['Palau'],
  score: 1005,
}

const rsPass = (over: Partial<SatPass> = {}): SatPass => ({
  name: 'RS-44',
  aosUnix: AOS,
  losUnix: LOS,
  maxElDeg: 62,
  aosAzDeg: 100,
  losAzDeg: 260,
  status: 'alive',
  earn,
  ...over,
})
/** Earlier AOS, nothing to earn — the default-order and no-chips control row. */
const aoPass = (): SatPass => ({
  name: 'AO-91',
  aosUnix: NOW + 600,
  losUnix: NOW + 1200,
  maxElDeg: 30,
  aosAzDeg: 200,
  losAzDeg: 320,
  status: 'alive',
  earn: null,
})

const detail = (over: Partial<SatDetail> = {}): SatDetail => ({
  name: 'RS-44',
  norad: 44909,
  status: 'alive',
  transmitters: [
    {
      description: 'CW beacon',
      alive: true,
      mode: 'CW',
      uplinkLowHz: null,
      downlinkLowHz: 435_605_000,
      invert: false,
      uplinkHighHz: null,
      downlinkHighHz: null,
      uplinkMode: null,
      downlinkMode: 'CW',
      kind: 'Transmitter',
    },
    {
      description: 'Retired FM repeater',
      alive: false,
      mode: 'FM',
      uplinkLowHz: 145_900_000,
      downlinkLowHz: 435_000_000,
      invert: false,
      uplinkHighHz: null,
      downlinkHighHz: null,
      uplinkMode: null,
      downlinkMode: null,
      kind: 'Transceiver',
    },
    {
      description: 'SSB/CW linear transponder',
      alive: true,
      mode: 'LSB',
      uplinkLowHz: 145_965_000,
      downlinkLowHz: 435_640_000,
      invert: true,
      uplinkHighHz: 145_995_000,
      downlinkHighHz: 435_670_000,
      uplinkMode: 'LSB',
      downlinkMode: 'USB',
      kind: 'Transponder',
    },
  ],
  dataFetchedAt: 1_760_000_000,
  pass: rsPass(),
  passTrack: [
    [AOS, 100, 0],
    [(AOS + LOS) / 2, 180, 62],
    [LOS, 260, 0],
  ],
  ...over,
})

/** Doppler live on the inverting linear (engine holds index 2). */
const liveStatus = (over: Partial<SatTrackStatus> = {}): SatTrackStatus => ({
  name: 'RS-44',
  state: 'tracking',
  mode: 'doppler-only',
  // The ENGINE's declared TX-leg sideband (what the real wire carries for an
  // inverting bird under an uplink-driving mapping) — the display reads THIS,
  // never a re-derivation from the SatNOGS record.
  txMode: 'LSB',
  azDeg: null,
  elDeg: null,
  aosAzDeg: 100,
  satAzDeg: 143,
  satElDeg: 47,
  rangeKm: 812,
  rangeRateKmS: -5.42,
  downlinkHz: 435_643_320,
  uplinkHz: 145_962_680,
  downlinkShiftHz: -2310,
  uplinkShiftHz: 770,
  transponder: 'SSB/CW linear transponder',
  transponderIndex: 2,
  inverting: true,
  offsetHz: 3200,
  halfWidthHz: 12_500,
  elementAgeDays: 1.2,
  elementEpochUnix: 1_785_442_400,
  aosUnix: AOS,
  losUnix: LOS,
  ...over,
})

const settings = (over: Record<string, unknown> = {}) => ({
  mygrid: 'EN52',
  rotatorModel: 0,
  rotatorHost: '',
  satDoppler: true,
  satVfoMap: 'main-down-sub-up',
  ...over,
})

beforeEach(() => {
  localStorage.clear()
  localStorage.setItem('nexus.sats.chasing', JSON.stringify(['RS-44', 'AO-91']))
  api.getSatellites.mockClear()
  api.getSatSchedule.mockReset()
  api.getSatSchedule.mockImplementation(() => Promise.resolve([aoPass(), rsPass()]))
  api.getSatPassNeeds.mockReset()
  api.getSatPassNeeds.mockImplementation(() => Promise.resolve([aoPass(), rsPass()]))
  api.getSatDetail.mockReset()
  api.getSatDetail.mockImplementation(() => Promise.resolve(detail()))
  api.getSettings.mockReset()
  api.getSettings.mockImplementation(() => Promise.resolve(settings()))
  api.setSatTransponder.mockReset()
  api.setSatTransponder.mockImplementation(() => Promise.resolve())
  api.getSatTrackStatus.mockReset()
  api.getSatTrackStatus.mockImplementation(() => Promise.resolve(null))
})
afterEach(cleanup)

const scheduleRows = (container: HTMLElement) =>
  Array.from(container.querySelectorAll('.sats-sched tbody tr'))

describe('the needs-aware schedule', () => {
  it('fetches the schedule through get_sat_pass_needs (earn arrives stamped)', async () => {
    render(<SatellitesView />)
    await waitFor(() =>
      expect(api.getSatPassNeeds).toHaveBeenCalledWith(['AO-91', 'RS-44'], 48),
    )
  })

  it('renders earn as the need-chip vocabulary on the earning row only', async () => {
    const { container } = render(<SatellitesView />)
    await waitFor(() => expect(scheduleRows(container).length).toBe(2))
    const rs = scheduleRows(container).find((r) => /RS-44/.test(r.textContent ?? ''))!
    const ao = scheduleRows(container).find((r) => /AO-91/.test(r.textContent ?? ''))!
    // The one vocabulary: NEW ONE (entity) + GRID, with the --need-* classes.
    const entity = rs.querySelector('.need-chip.need-entity')
    const grid = rs.querySelector('.need-chip.need-grid')
    expect(entity?.textContent).toMatch(/NEW ONE/)
    expect(grid?.textContent).toMatch(/GRID ×5/)
    // The sample names live in the tooltip — the counts stay complete.
    expect(entity?.getAttribute('title')).toMatch(/Palau/)
    expect(grid?.getAttribute('title')).toMatch(/EN12/)
    expect(grid?.getAttribute('title')).toMatch(/Satellite VUCC/i)
    // Nothing to earn = NO chips. Absent, not zero.
    expect(ao.querySelector('.need-chip')).toBeNull()
  })

  it('Needed is a sort key the operator clicks — never a silent reorder', async () => {
    const { container } = render(<SatellitesView />)
    await waitFor(() => expect(scheduleRows(container).length).toBe(2))
    // Default: soonest AOS first (AO-91), even though RS-44 out-earns it.
    expect(scheduleRows(container)[0].textContent).toMatch(/AO-91/)
    fireEvent.click(screen.getByTitle('Sort by Needed'))
    await waitFor(() =>
      expect(scheduleRows(container)[0].textContent).toMatch(/RS-44/),
    )
  })

  it('puts the earn chips on the Next-up strip rows too', async () => {
    const { container } = render(<SatellitesView />)
    await waitFor(() => expect(container.querySelector('.sats-best')).toBeTruthy())
    const best = container.querySelector('.sats-best')!
    const rsRow = Array.from(best.querySelectorAll('.sats-best-row')).find((r) =>
      /RS-44/.test(r.textContent ?? ''),
    )
    expect(rsRow?.querySelector('.need-chip.need-grid')).toBeTruthy()
  })
})

describe('the pass timeline earn lane', () => {
  it('shows the sample grids and the honest remainder for the selected pass', async () => {
    render(<SatellitesView focusSat="RS-44" />)
    const lane = await screen.findByTestId('sat-tl-earn')
    expect(lane.textContent).toMatch(/EN12 EN13/)
    expect(lane.textContent).toMatch(/\+3 more/)
    expect(lane.querySelector('.need-chip.need-entity')).toBeTruthy()
  })

  it('renders no lane when the pass earns nothing', async () => {
    api.getSatPassNeeds.mockImplementation(() =>
      Promise.resolve([aoPass(), rsPass({ earn: null })]),
    )
    render(<SatellitesView focusSat="RS-44" />)
    await screen.findByRole('img') // dome settled
    expect(screen.queryByTestId('sat-tl-earn')).toBeNull()
  })
})

describe('the uplink sideband display', () => {
  it('shows the TX-leg mode in the Doppler readout when the sidebands swap', async () => {
    api.getSatTrackStatus.mockImplementation(() => Promise.resolve(liveStatus()))
    const { container } = render(<SatellitesView focusSat="RS-44" />)
    await waitFor(() => expect(container.querySelector('.sat-doppler')).toBeTruthy())
    const tx = container.querySelector('.sat-dop-txmode')
    expect(tx?.textContent).toMatch(/LSB/)
    // Display only — the wording claims the TX (split) leg, not this VFO.
    expect(tx?.getAttribute('title')).toMatch(/TX/i)
  })

  it('claims nothing when the legs share a mode — nothing is commanded', async () => {
    // The engine says nothing (txMode null on the wire — same-mode legs), and
    // the record agrees: no claim anywhere.
    api.getSatTrackStatus.mockImplementation(() => Promise.resolve(liveStatus({ txMode: null })))
    const d = detail()
    d.transmitters[2] = { ...d.transmitters[2], uplinkMode: 'USB', downlinkMode: 'USB' }
    api.getSatDetail.mockImplementation(() => Promise.resolve(d))
    const { container } = render(<SatellitesView focusSat="RS-44" />)
    await waitFor(() => expect(container.querySelector('.sat-doppler')).toBeTruthy())
    expect(container.querySelector('.sat-dop-txmode')).toBeNull()
  })

  it('never claims a command the engine is not sending — record swap, engine silent', async () => {
    // The record's legs DIFFER (LSB up / USB down) but the engine's answer is
    // null — a downlink-only mapping, a CW downlink, or the operator took the
    // mode back. The readout must not show a TX mode, and the chooser note
    // must not read as a command ("is set to") — this display re-deriving the
    // command from the record is exactly how the UI claimed a write the radio
    // never got.
    api.getSatTrackStatus.mockImplementation(() => Promise.resolve(liveStatus({ txMode: null })))
    const { container } = render(<SatellitesView focusSat="RS-44" />)
    await waitFor(() => expect(container.querySelector('.sat-doppler')).toBeTruthy())
    expect(container.querySelector('.sat-dop-txmode')).toBeNull()
    fireEvent.click(screen.getByLabelText('Work SSB/CW linear transponder'))
    const line = await screen.findByTestId('sat-tp-txmode')
    expect(line.textContent).not.toMatch(/is set to/)
    expect(line.textContent).toMatch(/not being commanded/)
  })

  it('the transponder chooser says what the TX sideband will be set to', async () => {
    render(<SatellitesView focusSat="RS-44" />)
    fireEvent.click(await screen.findByLabelText('Work SSB/CW linear transponder'))
    const line = await screen.findByTestId('sat-tp-txmode')
    expect(line.textContent).toMatch(/LSB/)
    expect(line.textContent).toMatch(/USB/)
  })
})

describe('the working-state order', () => {
  it('keeps the passband strip and the transponder chooser together, above the globe', async () => {
    api.getSatTrackStatus.mockImplementation(() => Promise.resolve(liveStatus()))
    render(<SatellitesView focusSat="RS-44" />)
    const strip = await screen.findByTestId('sat-passband')
    const chooser = await screen.findByTestId('sat-tp-list')
    const globe = await screen.findByTestId('sat-globe-box')
    const follows = (a: Element, b: Element) =>
      !!(a.compareDocumentPosition(b) & Node.DOCUMENT_POSITION_FOLLOWING)
    expect(follows(strip, chooser)).toBe(true) // strip → chooser…
    expect(follows(chooser, globe)).toBe(true) // …then the globe, demoted below
  })
})

describe('the favorites-list search still narrows (guard for the aside rework)', () => {
  it('filters the Birds list by the search box', async () => {
    api.getSatellites.mockImplementation(() =>
      Promise.resolve({
        tleAgeDays: 1,
        usableCount: 97,
        agingCount: 0,
        heldBackCount: 0,
        tleFetchedAt: 1_785_542_400,
        tleSource: 'mirror',
        birds: [
          { name: 'RS-44', lat: 0, lon: 0, altKm: 500, footprintKm: 2000, track: [] },
          { name: 'AO-91', lat: 0, lon: 0, altKm: 500, footprintKm: 2000, track: [] },
        ],
        passes: [],
        excluded: [],
      }),
    )
    const { container } = render(<SatellitesView />)
    await waitFor(() =>
      expect(container.querySelectorAll('.sats-favmgr li').length).toBe(2),
    )
    fireEvent.change(screen.getByPlaceholderText('search…'), { target: { value: 'RS' } })
    await waitFor(() =>
      expect(container.querySelectorAll('.sats-favmgr li').length).toBe(1),
    )
    expect(within(container.querySelector('.sats-favmgr') as HTMLElement).getByText('RS-44')).toBeTruthy()
  })
})
