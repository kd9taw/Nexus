// @vitest-environment jsdom
//
// SIMPLEX BIRDS — the 145.825 APRS digipeater class (ISS, NO-84), where the
// uplink and the downlink are the SAME frequency.
//
// Operator field report (0.24.4, dual-radio): picking one produced a rail
// reading "Radio: Yeasu · 2m · SSB — 145.825 ↓ · 145.825 ↑" — the wrong rig,
// the wrong class, and the same frequency printed twice as if it were a split.
// The backend now declines the split outright (one dial carries both legs, and
// asking for a split there engages the IC-9700's CROSS-BAND satellite mode on a
// same-band 2 m channel); this pins the two surfaces that report it.
//
// What is pinned here:
//  - the Radio line shows ONE frequency and says why, instead of the same
//    number twice behind a ↓ and a ↑;
//  - the Doppler row states that both legs ride one dial;
//  - that row no longer prints the VFO mapping TWICE — the select beside it is
//    the live value and the control for it, and the state text repeating the
//    identical sentence is the doubled line in the report;
//  - a cross-band bird is untouched: two frequencies, and no simplex wording.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup } from '@testing-library/react'
import { SatellitesView } from './SatellitesView'
import type { SatBinding, SatDetail, SatTrackStatus, SatTransponderHeld } from '../types'

const api = vi.hoisted(() => ({
  getSatellites: vi.fn(() => Promise.resolve(null)),
  getSatSchedule: vi.fn(() => Promise.resolve([])),
  getSatPassNeeds: vi.fn(() => Promise.resolve([])),
  getSatDetail: vi.fn(),
  getSettings: vi.fn(),
  setSettings: vi.fn((_s: unknown) => Promise.resolve({} as never)),
  setSatTransponder: vi.fn(() => Promise.resolve()),
  getSatTransponder: vi.fn((): Promise<SatTransponderHeld | null> => Promise.resolve(null)),
  startSatTrack: vi.fn((): Promise<SatTrackStatus | null> => Promise.resolve(null)),
  stopSatTrack: vi.fn(() => Promise.resolve()),
  getSatTrackStatus: vi.fn((): Promise<SatTrackStatus | null> => Promise.resolve(null)),
  fetchTlesNow: vi.fn(() => Promise.resolve(null)),
}))
vi.mock('../api', () => api)
vi.mock('./MapView', () => ({ MapView: () => null }))
vi.mock('../toast', () => ({ pushToast: vi.fn() }))

const NOW = Math.floor(Date.now() / 1000)

/** The ISS as SatNOGS publishes it: Mode V APRS, AFSK, 145.825 both ways. */
const detail = (): SatDetail => ({
  name: 'ISS',
  norad: 25544,
  status: 'alive',
  transmitters: [
    {
      description: 'Mode V APRS',
      alive: true,
      mode: 'AFSK',
      uplinkLowHz: 145_825_000,
      downlinkLowHz: 145_825_000,
      invert: false,
      uplinkHighHz: null,
      downlinkHighHz: null,
      uplinkMode: null,
      downlinkMode: null,
      kind: 'Transceiver',
    },
  ],
  dataFetchedAt: 1_760_000_000,
  elementAgeDays: 1.2,
  pass: null,
  passTrack: [],
})

/** What the engine binds after picking it: the FM rig, FM class, ONE leg. */
const binding = (over: Partial<SatBinding> = {}): SatBinding => ({
  radioId: 1,
  radioName: 'IC-9700',
  band: '2m',
  fm: true,
  simplex: true,
  downlinkMhz: 145.825,
  uplinkMhz: null,
  pendingDownlinkMhz: null,
  pendingUplinkMhz: null,
  note: 'Simplex channel — you transmit on this same dial, so no split was written.',
  ...over,
})

const held = (over: Partial<SatBinding> = {}): SatTransponderHeld => ({
  name: 'ISS',
  index: 0,
  description: 'Mode V APRS',
  binding: binding(over),
})

const trackStatus = (over: Partial<SatTrackStatus> = {}): SatTrackStatus => ({
  name: 'ISS',
  state: 'armed',
  mode: 'doppler-only',
  dopplerDownlink: true,
  // The HONEST wire for a held simplex channel: one dial carries both legs,
  // the engine writes no split, so the uplink leg reads false even under a
  // confirmed mapping (Engine::sat_doppler_legs — the same derivation the
  // tick writes by).
  dopplerUplink: false,
  uplinkOffer: 'none',
  uplinkOfferMap: null,
  uplinkRadio: 'IC-9700',
  uplinkRadioId: 1,
  azDeg: null,
  elDeg: null,
  aosAzDeg: 100,
  satAzDeg: null,
  satElDeg: null,
  rangeKm: null,
  rangeRateKmS: null,
  downlinkHz: null,
  uplinkHz: null,
  downlinkShiftHz: null,
  uplinkShiftHz: null,
  transponder: 'Mode V APRS',
  transponderIndex: 0,
  inverting: false,
  offsetHz: null,
  halfWidthHz: null,
  elementAgeDays: 1.2,
  elementEpochUnix: NOW - 104_000,
  aosUnix: NOW + 720,
  losUnix: NOW + 1500,
  ...over,
})

const settings = (over: Record<string, unknown> = {}) => ({
  mygrid: 'EN52',
  rotatorModel: 0,
  rotatorHost: '',
  satDopplerOff: false,
  satVfoMap: 'a-up-b-down',
  radioPegged: false,
  ...over,
})

beforeEach(() => {
  localStorage.clear()
  api.getSatDetail.mockReset()
  api.getSatDetail.mockImplementation(() => Promise.resolve(detail()))
  api.getSettings.mockReset()
  api.getSettings.mockImplementation(() => Promise.resolve(settings()))
  api.setSettings.mockReset()
  api.setSettings.mockImplementation(() => Promise.resolve({} as never))
  api.getSatTransponder.mockReset()
  api.getSatTransponder.mockImplementation(() => Promise.resolve(held()))
  api.getSatTrackStatus.mockReset()
  api.getSatTrackStatus.mockImplementation(() => Promise.resolve(trackStatus()))
})
afterEach(cleanup)

describe('a simplex bird on the readiness rail', () => {
  it('shows ONE frequency on the Radio line, with the reason there is only one', async () => {
    render(<SatellitesView focusSat="ISS" />)
    const state = (await screen.findByTestId('sat-radio-binding')).querySelector(
      '.sat-rail-state',
    )!.textContent!
    // The class the operator's routing rule needs, and the rig it reached.
    expect(state).toMatch(/IC-9700/)
    expect(state).toMatch(/2m/)
    expect(state).toMatch(/FM/)
    expect(state).not.toMatch(/SSB/)
    // ONE leg — never "145.825 ↓ · 145.825 ↑", which read as a split.
    expect(state).toMatch(/145\.825 ↓/)
    expect(state).not.toMatch(/↑/)
    expect(state).toMatch(/simplex/i)
  })

  it('says on the Doppler row that both legs ride one dial', async () => {
    render(<SatellitesView focusSat="ISS" />)
    const rail = await screen.findByTestId('sat-rail')
    const row = Array.from(rail.querySelectorAll('.sat-rail-row')).find((r) =>
      /Doppler/.test(r.textContent ?? ''),
    )!
    expect(row.querySelector('.sat-rail-state')!.textContent).toMatch(/same dial/)
  })

  it('names the VFO mapping exactly once on that row — the select is the value', async () => {
    // The doubled line in the field report: the state text spelled the mapping
    // out and the select beside it printed the identical sentence as its
    // selected option. One row, one statement of the mapping.
    render(<SatellitesView focusSat="ISS" />)
    const rail = await screen.findByTestId('sat-rail')
    const row = Array.from(rail.querySelectorAll('.sat-rail-row')).find((r) =>
      /Doppler/.test(r.textContent ?? ''),
    )!
    const label = 'VFO A = uplink, VFO B = downlink'
    // The select still carries it (it is the control AND the read-back)…
    expect((row.querySelector('select') as HTMLSelectElement).selectedOptions[0].textContent).toBe(
      label,
    )
    // …and the state text beside it does not repeat it.
    expect(row.querySelector('.sat-rail-state')!.textContent).not.toMatch(/VFO A/)
  })

  it('never promises an uplink confirmation on a one-channel bird', async () => {
    // Round 2, defect 5. Confirming a Main/Sub mapping on a simplex hold
    // records a permanent per-radio consent for a split the engine refuses on
    // this bird — the promise in the offer copy cannot be kept, so the rail
    // must not render it. (The backend suppresses the offer on such a hold
    // too; the rail's simplex branch is the belt to that suspender.)
    api.getSatTrackStatus.mockImplementation(() =>
      Promise.resolve(
        trackStatus({ uplinkOffer: 'confirm', uplinkOfferMap: 'main-down-sub-up' }),
      ),
    )
    render(<SatellitesView focusSat="ISS" />)
    const rail = await screen.findByTestId('sat-rail')
    const row = Array.from(rail.querySelectorAll('.sat-rail-row')).find((r) =>
      /Doppler/.test(r.textContent ?? ''),
    )!
    expect(row.querySelector('.sat-rail-state')!.textContent).toMatch(/same dial/)
    expect(row.textContent).not.toMatch(/Confirm the uplink/)
    expect(screen.queryByRole('button', { name: /confirm uplink/i })).toBeNull()
  })

  it('leaves a CROSS-BAND bird alone — two frequencies, no simplex wording', async () => {
    // SO-50 is an FM bird too, but its legs are 2 m up / 70 cm down: it keeps
    // its split, and nothing here may tell the operator otherwise. The wire
    // reports its uplink genuinely driven.
    api.getSatTrackStatus.mockImplementation(() =>
      Promise.resolve(trackStatus({ dopplerUplink: true })),
    )
    api.getSatTransponder.mockImplementation(() =>
      Promise.resolve(
        held({ simplex: false, uplinkMhz: 145.85, downlinkMhz: 436.795, note: null }),
      ),
    )
    render(<SatellitesView focusSat="ISS" />)
    const rail = await screen.findByTestId('sat-rail')
    const state = (await screen.findByTestId('sat-radio-binding')).querySelector(
      '.sat-rail-state',
    )!.textContent!
    expect(state).toMatch(/436\.795 ↓/)
    expect(state).toMatch(/145\.850 ↑/)
    expect(state).not.toMatch(/simplex/i)
    const row = Array.from(rail.querySelectorAll('.sat-rail-row')).find((r) =>
      /Doppler/.test(r.textContent ?? ''),
    )!
    expect(row.querySelector('.sat-rail-state')!.textContent).toBe(
      'correcting the downlink and the uplink',
    )
  })
})
