// @vitest-environment jsdom
//
// "Work this pass" + the readiness rail + rotor-less tracking (litigation top-5 ①/③).
//
// What is pinned here is the CHAIN and its honesty:
//
//  - The arm affordance renders WITHOUT a rotator. The backend runs a rotor-less track
//    (pass clock + Doppler); hiding the column made the largest satellite population —
//    the Arrow-antenna FM operator — believe the feature did not exist.
//  - The tracking badge reads the DTO's `mode` and says exactly which surfaces are
//    driven: 'Doppler only' (no rotor), 'pass timing only' (rotor-less AND Doppler
//    prereqs off). It never claims a rotor it does not have.
//  - "Work this pass" is ONE control that runs the chain: select the bird, auto-pick a
//    workable transponder (never a beacon; never overriding the operator's explicit
//    "None"), arm the pass. The click is the consent — nothing arms without it.
//  - The readiness rail renders the chain AS a chain: five rows (Elements joined in
//    the currency overhaul), each gate's absence visible and fixable in place. The
//    Doppler row reports the two SEPARATELY consented legs from the track itself
//    (downlink automatic, uplink confirmed per radio) and carries the one-time
//    uplink confirmation as its fix. The off switch is a live mirror (writes go
//    read-modify-write through getSettings → setSettings so no other setting is
//    clobbered); the MAPPING writes through the backend confirmSatUplink verb —
//    the consent pair is engine-owned live state a settings payload cannot
//    carry (round 3). The rail never flips a fail-safe default by itself.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
import { SatellitesView } from './SatellitesView'
import type { SatDetail, SatPass, SatTrackStatus, SatView } from '../types'

const api = vi.hoisted(() => ({
  getSatellites: vi.fn((): Promise<SatView | null> => Promise.resolve(null)),
  getSatSchedule: vi.fn((): Promise<SatPass[]> => Promise.resolve([])),
  getSatPassNeeds: vi.fn((): Promise<SatPass[]> => Promise.resolve([])),
  getSatDetail: vi.fn(),
  getSettings: vi.fn(),
  setSettings: vi.fn((_s: unknown) => Promise.resolve({} as never)),
  confirmSatUplink: vi.fn((_m: unknown, _r?: number) => Promise.resolve({} as never)),
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

const passRow = (over: Partial<SatPass> = {}): SatPass => ({
  name: 'RS-44',
  aosUnix: NOW + 720,
  losUnix: NOW + 1500,
  maxElDeg: 62,
  aosAzDeg: 100,
  losAzDeg: 260,
  status: 'alive',
  ...over,
})

/** RS-44 with a dead entry first (the index trap), a beacon, then the linear. */
const linearDetail = (): SatDetail => ({
  name: 'RS-44',
  norad: 44909,
  status: 'alive',
  transmitters: [
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
  pass: passRow(),
  passTrack: [
    [NOW + 720, 100, 0],
    [NOW + 1100, 180, 62],
    [NOW + 1500, 260, 0],
  ],
})

/** A bird whose only alive transmitter is a beacon — never auto-picked. */
const beaconOnlyDetail = (): SatDetail => ({
  ...linearDetail(),
  transmitters: [linearDetail().transmitters[1]],
})

const trackStatus = (over: Partial<SatTrackStatus> = {}): SatTrackStatus => ({
  name: 'RS-44',
  state: 'armed',
  mode: 'doppler-only',
  dopplerDownlink: true,
  dopplerUplink: true,
  uplinkOffer: 'none',
  uplinkOfferMap: null,
  uplinkRadio: 'IC-9700',
  // Matches mkSettings' activeRadio — the DTO names the rig any confirmation
  // lands on, and the click confirms for THIS id, never the active one.
  uplinkRadioId: 3,
  azDeg: null,
  elDeg: null,
  aosAzDeg: 100,
  maxElDeg: 45,
  satAzDeg: null,
  satElDeg: null,
  rangeKm: null,
  rangeRateKmS: null,
  downlinkHz: null,
  uplinkHz: null,
  downlinkShiftHz: null,
  uplinkShiftHz: null,
  transponder: null,
  transponderIndex: null,
  inverting: false,
  offsetHz: null,
  halfWidthHz: null,
  elementAgeDays: 1.2,
  elementEpochUnix: 1_785_442_400,
  aosUnix: NOW + 720,
  losUnix: NOW + 1500,
  ...over,
})

/** NO rotator configured — the population the rotor refusal shut out. */
const mkSettings = (over: Record<string, unknown> = {}) => ({
  mygrid: 'EN52',
  rotatorModel: 0,
  rotatorHost: '',
  satDopplerOff: false,
  satVfoMap: 'off',
  activeRadio: 3,
  ...over,
})

/** The element snapshot the Next/Best strip reads (2026-08 ruling: the strip
 * is ALL-bird from `view.passes`, no longer an echo of the ★ schedule) — the
 * strip rows are the tests' always-on work affordances, so the snapshot must
 * exist like it does in the app. */
const mkView = (): SatView => ({
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
  passes: [
    passRow({ norad: 44909 }),
    passRow({ name: 'AO-91', norad: 43017, aosUnix: NOW + 900, losUnix: NOW + 1700 }),
  ],
  excluded: [],
})

beforeEach(() => {
  localStorage.clear()
  localStorage.setItem('nexus.sats.chasing', JSON.stringify(['RS-44']))
  api.getSatellites.mockReset()
  api.getSatellites.mockImplementation(() => Promise.resolve(mkView()))
  api.getSatSchedule.mockReset()
  api.getSatSchedule.mockImplementation(() => Promise.resolve([passRow()]))
  api.getSatPassNeeds.mockReset()
  api.getSatPassNeeds.mockImplementation(() => Promise.resolve([passRow()]))
  api.getSatDetail.mockReset()
  api.getSatDetail.mockImplementation(() => Promise.resolve(linearDetail()))
  api.getSettings.mockReset()
  api.getSettings.mockImplementation(() => Promise.resolve(mkSettings()))
  api.setSettings.mockReset()
  api.setSettings.mockImplementation(() => Promise.resolve({} as never))
  api.confirmSatUplink.mockReset()
  api.confirmSatUplink.mockImplementation(() => Promise.resolve({} as never))
  api.setSatTransponder.mockReset()
  api.setSatTransponder.mockImplementation(() => Promise.resolve())
  api.getSatTransponder.mockReset()
  api.getSatTransponder.mockImplementation(() => Promise.resolve(null))
  api.startSatTrack.mockReset()
  api.startSatTrack.mockImplementation(() => Promise.resolve(trackStatus()))
  api.getSatTrackStatus.mockReset()
  api.getSatTrackStatus.mockImplementation(() => Promise.resolve(null))
})
afterEach(cleanup)

/** All "Work this pass" affordances (Next-up rows + schedule rows). */
const workButtons = () => screen.findAllByTitle(/^Work this pass/)

describe('rotor-less tracking UI', () => {
  it('renders the work/arm affordance with NO rotator configured', async () => {
    render(<SatellitesView />)
    // At least one on the Next-up strip and one in the schedule column.
    expect((await workButtons()).length).toBeGreaterThanOrEqual(2)
    // …and its title says honestly that nothing will move.
    const rowBtn = (await workButtons()).find((b) => /no rotator/.test(b.title ?? ''))
    expect(rowBtn).toBeTruthy()
  })

  it('polls sat_track_status without a rotor — the section can see its own track', async () => {
    render(<SatellitesView />)
    await waitFor(() => expect(api.getSatTrackStatus).toHaveBeenCalled())
  })

  it('clicking work arms the pass without a rotor', async () => {
    render(<SatellitesView />)
    fireEvent.click((await workButtons())[0])
    await waitFor(() =>
      expect(api.startSatTrack).toHaveBeenCalledWith('RS-44', NOW + 720),
    )
  })
})

describe('the honest mode badge', () => {
  it("says 'Doppler only' for a rotor-less track and never claims a rotor", async () => {
    api.getSatTrackStatus.mockImplementation(() =>
      Promise.resolve(trackStatus({ mode: 'doppler-only' })),
    )
    const { container } = render(<SatellitesView />)
    await waitFor(() => expect(container.querySelector('.sats-tracking-badge')).toBeTruthy())
    const badge = container.querySelector('.sats-tracking-badge')!
    expect(badge.textContent).toMatch(/Doppler only/)
    expect(badge.textContent).toMatch(/rises az 100°/)
    expect(badge.textContent).not.toMatch(/cmd az/)
    expect(badge.getAttribute('title')).not.toMatch(/driving the rotor/)
    expect(badge.getAttribute('title')).not.toMatch(/takes it 5 min before AOS/)
  })

  it("says 'pass timing only' when neither the rotor nor Doppler is driven", async () => {
    api.getSatTrackStatus.mockImplementation(() =>
      Promise.resolve(trackStatus({ mode: 'pass-only' })),
    )
    const { container } = render(<SatellitesView />)
    await waitFor(() => expect(container.querySelector('.sats-tracking-badge')).toBeTruthy())
    const badge = container.querySelector('.sats-tracking-badge')!
    expect(badge.textContent).toMatch(/pass timing only/)
    expect(badge.getAttribute('title')).toMatch(/nothing is driven/i)
  })

  it('adds no mode word on the full rotor+doppler track (the unmarked case)', async () => {
    api.getSatTrackStatus.mockImplementation(() =>
      Promise.resolve(
        trackStatus({ mode: 'rotor+doppler', state: 'tracking', azDeg: 141, elDeg: 46 }),
      ),
    )
    const { container } = render(<SatellitesView />)
    await waitFor(() => expect(container.querySelector('.sats-tracking-badge')).toBeTruthy())
    const badge = container.querySelector('.sats-tracking-badge')!
    expect(badge.textContent).not.toMatch(/Doppler only|pass timing only|rotor only/)
    expect(badge.textContent).toMatch(/cmd az 141°/)
  })
})

describe('"Work this pass" runs the chain', () => {
  it('selects the bird, auto-picks the first workable transponder, and arms', async () => {
    render(<SatellitesView />)
    fireEvent.click((await workButtons())[0])
    // Wire index 2: the RAW index of the linear (dead row 0 counts — the index trap).
    await waitFor(() => expect(api.setSatTransponder).toHaveBeenCalledWith('RS-44', 2))
    await waitFor(() => expect(api.startSatTrack).toHaveBeenCalledWith('RS-44', NOW + 720))
    // The auto-pick is disclosed, not silent (on the rail and on the card).
    expect((await screen.findAllByText(/picked for you/)).length).toBeGreaterThanOrEqual(1)
  })

  it('NEVER auto-picks a beacon — a downlink-only entry cannot be worked', async () => {
    api.getSatDetail.mockImplementation(() => Promise.resolve(beaconOnlyDetail()))
    render(<SatellitesView />)
    fireEvent.click((await workButtons())[0])
    await waitFor(() => expect(api.startSatTrack).toHaveBeenCalled())
    expect(api.setSatTransponder).not.toHaveBeenCalled()
  })

  it('re-picks on the next pass after the backend handed the dial back at LOS', async () => {
    // The stale-hold defect: pick → the pass runs to LOS → the backend
    // releases the hold (set_sat_transponder(None)) — but the local mirror
    // still said "held", so the next ▶ Work on the SAME bird (LEO birds
    // repeat every ~100 min; this is the normal flow) skipped its re-pick
    // and armed a pass whose Doppler had nothing to tune, with the rail all
    // green. The engine truth (get_sat_transponder, mocked null throughout
    // = nothing held) must win over the last local click.
    render(<SatellitesView focusSat="RS-44" />)
    fireEvent.click(await screen.findByLabelText('Work SSB/CW linear transponder'))
    await waitFor(() => expect(api.setSatTransponder).toHaveBeenCalledWith('RS-44', 2))
    api.setSatTransponder.mockClear()
    // (LOS passed: the engine reports no hold. Working the bird again must
    // run the auto-pick again.)
    fireEvent.click((await workButtons())[0])
    await waitFor(() => expect(api.setSatTransponder).toHaveBeenCalledWith('RS-44', 2))
    await waitFor(() => expect(api.startSatTrack).toHaveBeenCalled())
  })

  it('skips the auto-pick when the ENGINE already holds this bird', async () => {
    api.getSatTransponder.mockImplementation(() =>
      Promise.resolve({
        name: 'RS-44',
        index: 2,
        description: 'SSB/CW linear transponder',
        binding: null,
      }),
    )
    render(<SatellitesView />)
    fireEvent.click((await workButtons())[0])
    await waitFor(() => expect(api.startSatTrack).toHaveBeenCalled())
    expect(api.setSatTransponder).not.toHaveBeenCalled()
  })

  it('a None on one bird survives working ANOTHER bird — consent is per bird', async () => {
    // The single-slot opt-out was erased by any later pick on a different
    // bird, breaking the stated invariant ("Work this pass must never
    // re-take a dial that was deliberately handed back").
    localStorage.setItem('nexus.sats.chasing', JSON.stringify(['RS-44', 'AO-91']))
    const ao = { ...linearDetail(), name: 'AO-91' }
    api.getSatDetail.mockImplementation((name: string) =>
      Promise.resolve(name === 'AO-91' ? ao : linearDetail()),
    )
    api.getSatPassNeeds.mockImplementation(() =>
      Promise.resolve([
        passRow(),
        passRow({ name: 'AO-91', aosUnix: NOW + 900, losUnix: NOW + 1700 }),
      ]),
    )
    render(<SatellitesView focusSat="RS-44" />)
    // Say None for RS-44 (pick first — the None radio starts checked, and a
    // click on a checked radio fires nothing).
    fireEvent.click(await screen.findByLabelText('Work SSB/CW linear transponder'))
    await waitFor(() => expect(api.setSatTransponder).toHaveBeenCalledWith('RS-44', 2))
    fireEvent.click(
      await screen.findByLabelText('Work no transponder — leave the dial to me'),
    )
    await waitFor(() => expect(api.setSatTransponder).toHaveBeenCalledWith('RS-44', null))
    api.setSatTransponder.mockClear()
    // Work AO-91: ITS auto-pick fires — that consent belongs to AO-91 alone.
    const rowFor = (name: string) => (b: HTMLElement) =>
      new RegExp(name).test(b.closest('tr, .sats-best-row')?.textContent ?? '')
    fireEvent.click((await workButtons()).find(rowFor('AO-91'))!)
    await waitFor(() => expect(api.setSatTransponder).toHaveBeenCalledWith('AO-91', 2))
    api.setSatTransponder.mockClear()
    api.startSatTrack.mockClear()
    // Work RS-44 again: the None must still stand.
    fireEvent.click((await workButtons()).find(rowFor('RS-44'))!)
    await waitFor(() => expect(api.startSatTrack).toHaveBeenCalledWith('RS-44', NOW + 720))
    expect(api.setSatTransponder).not.toHaveBeenCalled()
  })

  it("respects the operator's explicit None — work never re-takes the dial", async () => {
    render(<SatellitesView focusSat="RS-44" />)
    // Pick, then explicitly hand the dial back (a fresh radio starts on None,
    // and clicking an already-checked radio fires nothing).
    fireEvent.click(await screen.findByLabelText('Work SSB/CW linear transponder'))
    await waitFor(() => expect(api.setSatTransponder).toHaveBeenCalledWith('RS-44', 2))
    fireEvent.click(
      await screen.findByLabelText('Work no transponder — leave the dial to me'),
    )
    await waitFor(() => expect(api.setSatTransponder).toHaveBeenCalledWith('RS-44', null))
    api.setSatTransponder.mockClear()
    fireEvent.click((await workButtons())[0])
    await waitFor(() => expect(api.startSatTrack).toHaveBeenCalled())
    expect(api.setSatTransponder).not.toHaveBeenCalled()
  })
})

describe('the readiness rail', () => {
  it('renders the five gates with honest not-ready states and shape-carried bullets', async () => {
    api.getSatTrackStatus.mockImplementation(() =>
      Promise.resolve(
        trackStatus({ mode: 'pass-only', dopplerDownlink: false, dopplerUplink: false }),
      ),
    )
    render(<SatellitesView focusSat="RS-44" />)
    const rail = await screen.findByTestId('sat-rail')
    const text = rail.textContent ?? ''
    expect(text).toMatch(/Pass/)
    expect(text).toMatch(/armed — AOS in \d+ min/)
    expect(text).toMatch(/Rotor/)
    expect(text).toMatch(/no rotator configured/)
    expect(text).toMatch(/Transponder/)
    expect(text).toMatch(/none — the dial stays yours/)
    expect(text).toMatch(/Doppler/)
    // No transponder held, so nothing is being tuned YET — and the row says
    // what it is waiting for rather than "off", which is a different fact.
    expect(text).toMatch(/waiting for a transponder/)
    expect(text).toMatch(/Elements/)
    expect(text).toMatch(/1\.2 d old — current/)
    // Shape, not colour: filled ● for ready, hollow ○ for not — 5 rows,
    // 2 ready (Pass + the fresh frozen Elements set).
    const dots = rail.querySelectorAll('.sat-rail-dot')
    expect(dots.length).toBe(5)
    expect(rail.querySelectorAll('.sat-rail-dot.ok').length).toBe(2)
  })

  it('the Doppler row says the downlink is corrected and offers the uplink once', async () => {
    // THE operator's report, as the rail sees it: nothing configured, a
    // transponder held, and the receive dial already following the bird. The
    // transmit VFO is the only thing left to ask about — once, for this radio,
    // in the words of the radio itself.
    api.getSatTrackStatus.mockImplementation(() =>
      Promise.resolve(
        trackStatus({
          state: 'tracking',
          mode: 'doppler-only',
          transponder: 'RS-44|SSB/CW linear transponder',
          dopplerDownlink: true,
          dopplerUplink: false,
          uplinkOffer: 'confirm',
          uplinkOfferMap: 'main-down-sub-up',
          uplinkRadio: 'IC-9700',
        }),
      ),
    )
    render(<SatellitesView focusSat="RS-44" />)
    const rail = await screen.findByTestId('sat-rail')
    const row = Array.from(rail.querySelectorAll('.sat-rail-row')).find((r) =>
      /Doppler/.test(r.textContent ?? ''),
    )!
    const state = row.querySelector('.sat-rail-state')!.textContent ?? ''
    expect(state).toMatch(/correcting the downlink/)
    expect(state).toMatch(/Doppler drives IC-9700 as Main = downlink, Sub = uplink/)
    // The example parenthetical belongs to the select, not to the sentence.
    expect(state).not.toMatch(/full duplex\)/)
    // One click hands the transmit VFO over — mapping AND the radio it is
    // confirmed for (the DTO's uplinkRadioId), through the ONE backend verb:
    // the consent pair is engine-owned live state, never a settings payload.
    fireEvent.click(screen.getByRole('button', { name: /confirm uplink/i }))
    await waitFor(() => expect(api.confirmSatUplink).toHaveBeenCalledWith('main-down-sub-up', 3))
  })

  it('the confirmation lands on the radio the copy names, not whichever is active at click time', async () => {
    // Round 2, defect 6a. The DTO names the rig the offer is about
    // (uplinkRadio / uplinkRadioId — the rig that would receive the split).
    // If the active radio moves between the DTO poll and the click, the grant
    // must still land on the NAMED rig: consent for any other radio was never
    // shown to the operator.
    api.getSettings.mockImplementation(() =>
      Promise.resolve(mkSettings({ activeRadio: 7 })),
    )
    api.getSatTrackStatus.mockImplementation(() =>
      Promise.resolve(
        trackStatus({
          state: 'tracking',
          mode: 'doppler-only',
          transponder: 'RS-44|SSB/CW linear transponder',
          dopplerDownlink: true,
          dopplerUplink: false,
          uplinkOffer: 'confirm',
          uplinkOfferMap: 'main-down-sub-up',
          uplinkRadio: 'IC-9700',
          uplinkRadioId: 3,
        }),
      ),
    )
    render(<SatellitesView focusSat="RS-44" />)
    await screen.findByTestId('sat-rail')
    fireEvent.click(screen.getByRole('button', { name: /confirm uplink/i }))
    // The verb is invoked with the NAMED rig's id — the backend records it
    // for that id, not for whichever radio is active when the click lands.
    await waitFor(() => expect(api.confirmSatUplink).toHaveBeenCalledWith('main-down-sub-up', 3))
    expect(api.confirmSatUplink).not.toHaveBeenCalledWith('main-down-sub-up', 7)
  })

  it('the chooser does not claim the transmit VFO is tuned while the mapping is unconfirmed', async () => {
    // Round 2, defect 5. The old line keyed on the settings mirror alone
    // ("uplink-only ⇒ only the transmit VFO is tuned") and contradicted the
    // engine's own binding note in the same column when the mapping was not
    // confirmed for the radio in play. Truth comes from the track's per-leg
    // booleans, which ARE the engine's legs.
    api.getSettings.mockImplementation(() =>
      Promise.resolve(mkSettings({ satVfoMap: 'uplink-only' })),
    )
    api.getSatTransponder.mockImplementation(() =>
      Promise.resolve({
        name: 'RS-44',
        index: 2,
        description: 'SSB/CW linear transponder',
        binding: null,
      }),
    )
    api.getSatTrackStatus.mockImplementation(() =>
      Promise.resolve(
        trackStatus({
          state: 'tracking',
          mode: 'pass-only',
          transponder: 'RS-44|SSB/CW linear transponder',
          transponderIndex: 2,
          dopplerDownlink: false,
          dopplerUplink: false,
        }),
      ),
    )
    render(<SatellitesView focusSat="RS-44" />)
    await screen.findByTestId('sat-rail')
    const state = document.querySelector('.sat-tp-state')!
    expect(state.textContent).not.toMatch(/only the transmit VFO is tuned/)
    expect(state.textContent).toMatch(/not confirmed for this radio/)
  })

  it("the pass-only badge title enumerates causes that can actually apply now", async () => {
    // Round 2, defect 7. In the D2 half-state (correction on, transponder
    // held, uplink-only mapping unconfirmed) the old title blamed
    // "correction switched off, or no transponder held" — both false. The
    // actionable cause has to be in the list.
    api.getSatTrackStatus.mockImplementation(() =>
      Promise.resolve(
        trackStatus({ mode: 'pass-only', dopplerDownlink: false, dopplerUplink: false }),
      ),
    )
    const { container } = render(<SatellitesView focusSat="RS-44" />)
    await waitFor(() => expect(container.querySelector('.sats-tracking-badge')).toBeTruthy())
    const title = container.querySelector('.sats-tracking-badge')!.getAttribute('title')!
    expect(title).toMatch(/uplink-only mapping/)
  })

  it('the TX-sideband note names the live cause, never "Doppler off" in the default state', async () => {
    // Round 2, defect 7. With correction ON and the downlink being corrected
    // one row up, the note still listed "Doppler off" first among its causes.
    // The component knows the actual cause — name it alone.
    api.getSatTransponder.mockImplementation(() =>
      Promise.resolve({
        name: 'RS-44',
        index: 2,
        description: 'SSB/CW linear transponder',
        binding: null,
      }),
    )
    api.getSatTrackStatus.mockImplementation(() =>
      Promise.resolve(
        trackStatus({
          state: 'tracking',
          mode: 'doppler-only',
          transponder: 'RS-44|SSB/CW linear transponder',
          transponderIndex: 2,
          dopplerDownlink: true,
          dopplerUplink: false,
        }),
      ),
    )
    render(<SatellitesView focusSat="RS-44" />)
    await screen.findByTestId('sat-rail')
    const note = await screen.findByTestId('sat-tp-txmode')
    expect(note.textContent).not.toMatch(/Doppler off/)
    expect(note.textContent).toMatch(/not driving the uplink on this radio/)
  })

  it('never offers a mapping it cannot derive — it asks', async () => {
    // A confident default for a known full-duplex rig is not a licence to
    // guess for an unknown one. No confirm button, no pre-filled mapping.
    api.getSatTrackStatus.mockImplementation(() =>
      Promise.resolve(
        trackStatus({
          state: 'tracking',
          mode: 'doppler-only',
          transponder: 'RS-44|SSB/CW linear transponder',
          dopplerDownlink: true,
          dopplerUplink: false,
          uplinkOffer: 'ask',
          uplinkOfferMap: null,
          uplinkRadio: 'FT-847',
        }),
      ),
    )
    render(<SatellitesView focusSat="RS-44" />)
    const rail = await screen.findByTestId('sat-rail')
    expect(rail.textContent).toMatch(/Pick which VFO carries your uplink/)
    expect(screen.queryByRole('button', { name: /confirm uplink/i })).toBeNull()
  })

  it('is absent when no track is armed for this bird', async () => {
    render(<SatellitesView focusSat="RS-44" />)
    await screen.findByRole('img') // detail (sky dome) settled
    expect(screen.queryByTestId('sat-rail')).toBeNull()
  })

  it("the Doppler row's fix IS the row: 'turn on' clears satDopplerOff read-modify-write", async () => {
    api.getSettings.mockImplementation(() =>
      Promise.resolve(mkSettings({ satDopplerOff: true })),
    )
    api.getSatTrackStatus.mockImplementation(() =>
      Promise.resolve(
        trackStatus({ mode: 'pass-only', dopplerDownlink: false, dopplerUplink: false }),
      ),
    )
    render(<SatellitesView focusSat="RS-44" />)
    const rail = await screen.findByTestId('sat-rail')
    fireEvent.click(await screen.findByRole('button', { name: /turn on/i }))
    await waitFor(() => expect(api.setSettings).toHaveBeenCalled())
    const written = api.setSettings.mock.calls[0][0] as Record<string, unknown>
    expect(written.satDopplerOff).toBe(false)
    // Read-modify-write: the rest of the settings object rode along untouched.
    expect(written.mygrid).toBe('EN52')
    expect(rail).toBeTruthy()
  })

  it('the VFO mirror writes the mapping and carries the full downlink warning', async () => {
    api.getSatTrackStatus.mockImplementation(() =>
      Promise.resolve(trackStatus({ mode: 'pass-only' })),
    )
    render(<SatellitesView focusSat="RS-44" />)
    await screen.findByTestId('sat-rail')
    const sel = screen.getByLabelText('Satellite VFO mapping') as HTMLSelectElement
    expect(sel.title).toMatch(/transmits on your own downlink/)
    fireEvent.change(sel, { target: { value: 'main-down-sub-up' } })
    // The mapping and the radio it applies to are ONE write, through the
    // backend verb (the engine drives the uplink only on a confirmed radio,
    // so a mapping written alone would leave the rail asking forever) — and
    // the rail passes the DTO's uplinkRadioId, the rig its copy names.
    await waitFor(() => expect(api.confirmSatUplink).toHaveBeenCalledWith('main-down-sub-up', 3))
    expect(api.setSettings).not.toHaveBeenCalled()
  })

  it('disables the pick button WITH its reason when the bird lists no transmitters', async () => {
    // The chooser only renders when the bird HAS transmitters, so the rail's
    // pick button pointed at nothing — enabled, and silently doing nothing on
    // click. Disabled-with-reason, never a dead control.
    api.getSatTrackStatus.mockImplementation(() =>
      Promise.resolve(trackStatus({ mode: 'pass-only' })),
    )
    api.getSatDetail.mockImplementation(() =>
      Promise.resolve({ ...linearDetail(), transmitters: [] }),
    )
    render(<SatellitesView focusSat="RS-44" />)
    const rail = await screen.findByTestId('sat-rail')
    const pick = Array.from(rail.querySelectorAll('button')).find((b) =>
      /pick/.test(b.textContent ?? ''),
    ) as HTMLButtonElement
    expect(pick).toBeTruthy()
    expect(pick.disabled).toBe(true)
    expect(pick.title).toMatch(/No transmitters listed/i)
  })

  it('the Pass row carries the stop control', async () => {
    api.getSatTrackStatus.mockImplementation(() =>
      Promise.resolve(trackStatus({ mode: 'pass-only' })),
    )
    render(<SatellitesView focusSat="RS-44" />)
    const rail = await screen.findByTestId('sat-rail')
    const stop = Array.from(rail.querySelectorAll('button')).find((b) =>
      /stop/.test(b.textContent ?? ''),
    )
    expect(stop).toBeTruthy()
    fireEvent.click(stop!)
    await waitFor(() => expect(api.stopSatTrack).toHaveBeenCalled())
  })
})

describe('the Rotor gate for a rotor-less station', () => {
  // ○ is "configured but not in this track — re-arm can fix it". A station
  // with NO rotator can never pass that gate and never needs to: rendering ○
  // there made the chain read permanently broken for the largest satellite
  // population. Absence gets an absent mark, not a failed gate (the sentinel
  // lesson, applied to a glyph).
  it('renders an absent mark, not a failed gate, when no rotator is configured', async () => {
    api.getSatTrackStatus.mockImplementation(() =>
      Promise.resolve(trackStatus({ mode: 'pass-only' })),
    )
    render(<SatellitesView focusSat="RS-44" />)
    const rail = await screen.findByTestId('sat-rail')
    const rotorRow = Array.from(rail.querySelectorAll('.sat-rail-row')).find((r) =>
      /no rotator configured/.test(r.textContent ?? ''),
    )
    expect(rotorRow).toBeTruthy()
    expect(rotorRow?.querySelector('.sat-rail-dot')?.textContent).toBe('—')
  })

  it('keeps the hollow not-ready circle when a rotator exists but is not in this track', async () => {
    api.getSettings.mockImplementation(() => Promise.resolve(mkSettings({ rotatorModel: 2 })))
    api.getSatTrackStatus.mockImplementation(() =>
      Promise.resolve(trackStatus({ mode: 'doppler-only' })),
    )
    render(<SatellitesView focusSat="RS-44" />)
    const rail = await screen.findByTestId('sat-rail')
    const rotorRow = Array.from(rail.querySelectorAll('.sat-rail-row')).find((r) =>
      /re-arm to take the rotor/.test(r.textContent ?? ''),
    )
    expect(rotorRow).toBeTruthy()
    expect(rotorRow?.querySelector('.sat-rail-dot')?.textContent).toBe('○')
  })
})

// ROUND 3. The consent write goes through the ONE backend verb
// (confirmSatUplink — the pair is engine-owned live state, so a stale
// whole-settings Save can never resurrect a pruned consent), the rail's
// confirm affordance extends to a mapping already in force but unconfirmed
// for the radio in play, and every dial-ownership claim keys on the DOWNLINK
// leg — the leg that actually holds the dial.
describe('round 3: the consent verb and the mapping-in-force offer', () => {
  it('confirming the derived offer goes through the backend verb, for the radio the copy names', async () => {
    api.getSatTrackStatus.mockImplementation(() =>
      Promise.resolve(
        trackStatus({
          state: 'tracking',
          mode: 'doppler-only',
          transponder: 'RS-44|SSB/CW linear transponder',
          dopplerDownlink: true,
          dopplerUplink: false,
          uplinkOffer: 'confirm',
          uplinkOfferMap: 'main-down-sub-up',
          uplinkRadio: 'IC-9700',
          uplinkRadioId: 3,
        }),
      ),
    )
    render(<SatellitesView focusSat="RS-44" />)
    await screen.findByTestId('sat-rail')
    fireEvent.click(screen.getByRole('button', { name: /confirm uplink/i }))
    await waitFor(() => expect(api.confirmSatUplink).toHaveBeenCalledWith('main-down-sub-up', 3))
    // Never a whole-settings write: a form-shaped payload is what let a stale
    // snapshot resurrect a pruned consent.
    expect(api.setSettings).not.toHaveBeenCalled()
  })

  it('offers to confirm the mapping IN FORCE — never replacing it with a derived one', async () => {
    // Defect 4: uplink-only, unconfirmed for this radio — the migration's own
    // destination for a legacy uplink-only file. The select already shows
    // uplink-only (a same-value re-pick fires no change event), so the rail
    // must carry a real confirm affordance for THAT mapping.
    api.getSettings.mockImplementation(() =>
      Promise.resolve(mkSettings({ satVfoMap: 'uplink-only' })),
    )
    api.getSatTrackStatus.mockImplementation(() =>
      Promise.resolve(
        trackStatus({
          state: 'tracking',
          mode: 'pass-only',
          transponder: 'RS-44|SSB/CW linear transponder',
          dopplerDownlink: false,
          dopplerUplink: false,
          uplinkOffer: 'confirm-mapping',
          uplinkOfferMap: 'uplink-only',
          uplinkRadio: 'IC-9700',
          uplinkRadioId: 3,
        }),
      ),
    )
    render(<SatellitesView focusSat="RS-44" />)
    const rail = await screen.findByTestId('sat-rail')
    // The copy names the operator's own mapping, not a derived replacement.
    expect(rail.textContent).toMatch(/Confirm Uplink only \(transmit\) for IC-9700/)
    const btn = screen.getByRole('button', { name: /confirm uplink/i })
    // The button confirms the mapping in force for the DTO's radio — with NO
    // map argument (round 4, residual 3): the DTO's map is poll-time state,
    // so the backend resolves "the mapping in force" at write time, the same
    // way it already resolves the radio.
    fireEvent.click(btn)
    await waitFor(() => expect(api.confirmSatUplink).toHaveBeenCalledWith(undefined, 3))
    // …and its title does not claim the mapping was read from the radio.
    expect(btn.title).not.toMatch(/read this from your radio model/i)
  })

  it('keys dial ownership on the downlink leg: uplink-only driving never claims the dial', async () => {
    // Defect 5: uplink-only confirmed and driving — mode doppler-only,
    // uplinkHz set, downlinkHz null. The engine only writes the split TX
    // VFO; the badge title must not say Doppler is steering the dial.
    api.getSatTrackStatus.mockImplementation(() =>
      Promise.resolve(
        trackStatus({
          state: 'tracking',
          mode: 'doppler-only',
          transponder: 'RS-44|SSB/CW linear transponder',
          dopplerDownlink: false,
          dopplerUplink: true,
          downlinkHz: null,
          uplinkHz: 145_962_680,
          uplinkShiftHz: 770,
        }),
      ),
    )
    const { container } = render(<SatellitesView focusSat="RS-44" />)
    await waitFor(() => expect(container.querySelector('.sats-tracking-badge')).toBeTruthy())
    const title = container.querySelector('.sats-tracking-badge')!.getAttribute('title')!
    expect(title).not.toMatch(/steering the radio dial/)
    expect(title).not.toMatch(/takes the radio dial/)
    expect(title).toMatch(/dial stays yours/i)
  })

  it('pre-hold copy follows the mapping in force: uplink-only promises the uplink, not the downlink', async () => {
    // Defect 6: with nothing held, the rail promised "then the downlink
    // follows the bird" — deterministically false under uplink-only.
    api.getSettings.mockImplementation(() =>
      Promise.resolve(mkSettings({ satVfoMap: 'uplink-only' })),
    )
    api.getSatTrackStatus.mockImplementation(() =>
      Promise.resolve(
        trackStatus({ mode: 'pass-only', dopplerDownlink: false, dopplerUplink: false }),
      ),
    )
    render(<SatellitesView focusSat="RS-44" />)
    const rail = await screen.findByTestId('sat-rail')
    const row = Array.from(rail.querySelectorAll('.sat-rail-row')).find((r) =>
      /Doppler/.test(r.textContent ?? ''),
    )!
    const state = row.querySelector('.sat-rail-state')!.textContent ?? ''
    expect(state).toMatch(/waiting for a transponder/)
    expect(state).not.toMatch(/the downlink follows the bird/)
    expect(state).toMatch(/uplink/)
    // The Doppler readout's no-hold line makes the same promise — it must not
    // offer to "put the dial under Doppler" under a mapping that never will.
    const readout = document.querySelector('.sat-doppler.none')
    expect(readout?.textContent ?? '').not.toMatch(/put the dial under Doppler/)
  })

  it('the waiting copy is confirmation-aware: no promised drive while the mapping is unconfirmed', async () => {
    // ROUND 4, residual 5. Uplink-only IN FORCE but unconfirmed for the radio
    // in play (the DTO's offer says so: confirm-mapping). "then Doppler tunes
    // your uplink" promised a drive nobody had consented to — the waiting
    // copy must name the confirmation gate instead.
    api.getSettings.mockImplementation(() =>
      Promise.resolve(mkSettings({ satVfoMap: 'uplink-only' })),
    )
    api.getSatTrackStatus.mockImplementation(() =>
      Promise.resolve(
        trackStatus({
          mode: 'pass-only',
          dopplerDownlink: false,
          dopplerUplink: false,
          uplinkOffer: 'confirm-mapping',
          uplinkOfferMap: 'uplink-only',
        }),
      ),
    )
    render(<SatellitesView focusSat="RS-44" />)
    const rail = await screen.findByTestId('sat-rail')
    const row = Array.from(rail.querySelectorAll('.sat-rail-row')).find((r) =>
      /Doppler/.test(r.textContent ?? ''),
    )!
    const state = row.querySelector('.sat-rail-state')!.textContent ?? ''
    expect(state).toMatch(/waiting for a transponder/)
    expect(state).not.toMatch(/then Doppler tunes your uplink/)
    expect(state).toMatch(/once your mapping is confirmed/i)
    // The Doppler readout's no-hold line must not make the promise either.
    const readout = document.querySelector('.sat-doppler.none')?.textContent ?? ''
    expect(readout).not.toMatch(/pick one below and Doppler tunes your uplink/)
    expect(readout).toMatch(/once your mapping is confirmed/i)
  })
})

// THE FIELD REPORT (0.27.0, IC-9700, native CI-V serving): the operator held a
// cross-band bird under a VFO A/B mapping he had chosen and confirmed, the
// split was refused every tick, and the rail proposed nothing — because every
// offer is suppressed once a mapping is consented to. Offering a correction to
// a choice that CANNOT WORK is not overwriting the operator's choice: the
// backend re-opens the question in that one state ("switch-mapping"), and this
// is the affordance it lands on — the SAME rail button, never a second one.
describe('a mapping that cannot carry the pass', () => {
  it('offers the mapping that can, on the existing rail button, and applies nothing until clicked', async () => {
    api.getSettings.mockImplementation(() =>
      Promise.resolve(mkSettings({ satVfoMap: 'a-down-b-up' })),
    )
    api.getSatTrackStatus.mockImplementation(() =>
      Promise.resolve(
        trackStatus({
          state: 'tracking',
          mode: 'doppler-only',
          transponder: 'RS-44|SSB/CW linear transponder',
          // The uplink is CONSENTED to and computed every tick — and the split
          // apply refuses to write it, which is why the correction is offered
          // over a live uplink leg rather than in place of one.
          dopplerDownlink: true,
          dopplerUplink: true,
          uplinkOffer: 'switch-mapping',
          uplinkOfferMap: 'main-down-sub-up',
          uplinkRadio: 'IC-9700',
          uplinkRadioId: 3,
        }),
      ),
    )
    render(<SatellitesView focusSat="RS-44" />)
    const rail = await screen.findByTestId('sat-rail')
    const row = Array.from(rail.querySelectorAll('.sat-rail-row')).find((r) =>
      /Doppler/.test(r.textContent ?? ''),
    )!
    const state = row.querySelector('.sat-rail-state')!.textContent ?? ''
    // The row may NOT claim the uplink as driven — it is computed and thrown
    // away — and it names the mapping that would carry it, on this radio.
    expect(state).not.toMatch(/correcting the downlink and the uplink/)
    expect(state).toMatch(/Main = downlink, Sub = uplink/)
    expect(state).toMatch(/IC-9700/)
    // One click, the same verb, the radio the copy named — and nothing is
    // written until that click happens.
    expect(api.confirmSatUplink).not.toHaveBeenCalled()
    fireEvent.click(screen.getByRole('button', { name: /switch mapping/i }))
    await waitFor(() => expect(api.confirmSatUplink).toHaveBeenCalledWith('main-down-sub-up', 3))
    expect(api.setSettings).not.toHaveBeenCalled()
  })

  it('never second-guesses a mapping that works: no offer while the split lands', async () => {
    api.getSettings.mockImplementation(() =>
      Promise.resolve(mkSettings({ satVfoMap: 'a-down-b-up' })),
    )
    api.getSatTrackStatus.mockImplementation(() =>
      Promise.resolve(
        trackStatus({
          state: 'tracking',
          mode: 'doppler-only',
          transponder: 'SO-50|V/V FM repeater',
          dopplerDownlink: true,
          dopplerUplink: true,
          uplinkOffer: 'none',
          uplinkOfferMap: null,
        }),
      ),
    )
    render(<SatellitesView focusSat="RS-44" />)
    const rail = await screen.findByTestId('sat-rail')
    const row = Array.from(rail.querySelectorAll('.sat-rail-row')).find((r) =>
      /Doppler/.test(r.textContent ?? ''),
    )!
    expect(row.querySelector('.sat-rail-state')!.textContent).toMatch(
      /correcting the downlink and the uplink/,
    )
    expect(screen.queryByRole('button', { name: /switch mapping/i })).toBeNull()
  })
})

// The LOS handback notice moved to the app-wide armed-track watcher
// (features/satPassAlert.ts, tested there): fired from a view-scoped poll it
// only ever landed with this section open, and the operator may be anywhere
// in the app at LOS.
