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
//    the currency overhaul), each gate's absence visible and fixable in place. The two Settings switches (satDoppler, satVfoMap)
//    get live mirrors here — writes go read-modify-write through getSettings →
//    setSettings so no other setting is clobbered. The rail never flips a fail-safe
//    default by itself.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
import { SatellitesView } from './SatellitesView'
import { pushToast } from '../toast'
import type { SatDetail, SatPass, SatTrackStatus } from '../types'

const api = vi.hoisted(() => ({
  getSatellites: vi.fn(() => Promise.resolve(null)),
  getSatSchedule: vi.fn((): Promise<SatPass[]> => Promise.resolve([])),
  getSatPassNeeds: vi.fn((): Promise<SatPass[]> => Promise.resolve([])),
  getSatDetail: vi.fn(),
  getSettings: vi.fn(),
  setSettings: vi.fn((_s: unknown) => Promise.resolve({} as never)),
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
  satDoppler: false,
  satVfoMap: 'off',
  ...over,
})

beforeEach(() => {
  localStorage.clear()
  localStorage.setItem('nexus.sats.chasing', JSON.stringify(['RS-44']))
  api.getSatellites.mockClear()
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
      Promise.resolve(trackStatus({ mode: 'pass-only' })),
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
    expect(text).toMatch(/off — nothing is being tuned/)
    expect(text).toMatch(/Elements/)
    expect(text).toMatch(/1\.2 d old — current/)
    // Shape, not colour: filled ● for ready, hollow ○ for not — 5 rows,
    // 2 ready (Pass + the fresh frozen Elements set).
    const dots = rail.querySelectorAll('.sat-rail-dot')
    expect(dots.length).toBe(5)
    expect(rail.querySelectorAll('.sat-rail-dot.ok').length).toBe(2)
  })

  it('is absent when no track is armed for this bird', async () => {
    render(<SatellitesView focusSat="RS-44" />)
    await screen.findByRole('img') // detail (sky dome) settled
    expect(screen.queryByTestId('sat-rail')).toBeNull()
  })

  it("the Doppler row's fix IS the row: 'turn on' writes satDoppler read-modify-write", async () => {
    api.getSatTrackStatus.mockImplementation(() =>
      Promise.resolve(trackStatus({ mode: 'pass-only' })),
    )
    render(<SatellitesView focusSat="RS-44" />)
    const rail = await screen.findByTestId('sat-rail')
    fireEvent.click(await screen.findByRole('button', { name: /turn on/i }))
    await waitFor(() => expect(api.setSettings).toHaveBeenCalled())
    const written = api.setSettings.mock.calls[0][0] as Record<string, unknown>
    expect(written.satDoppler).toBe(true)
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
    await waitFor(() => expect(api.setSettings).toHaveBeenCalled())
    const written = api.setSettings.mock.calls[0][0] as Record<string, unknown>
    expect(written.satVfoMap).toBe('main-down-sub-up')
    expect(written.mygrid).toBe('EN52')
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

describe('the LOS handback', () => {
  // The backend releases the transponder hold at LOS; before this toast the
  // rail, binding line and header badge simply vanished on the next poll —
  // the one ownership change in the section with zero notification.
  it('says the pass ended and the dial came back when the track vanishes at LOS', async () => {
    const ended = trackStatus({
      state: 'tracking',
      mode: 'doppler-only',
      transponder: 'SSB/CW linear transponder',
      aosUnix: NOW - 700,
      losUnix: NOW - 5,
    })
    api.getSatTrackStatus.mockImplementationOnce(() => Promise.resolve(ended))
    render(<SatellitesView focusSat="RS-44" />)
    await waitFor(
      () =>
        expect(vi.mocked(pushToast)).toHaveBeenCalledWith(
          expect.stringMatching(/RS-44 pass complete — LOS\. Dial handed back\./),
          'info',
          6000,
        ),
      { timeout: 4000 },
    )
  })

  // The 2 s tick can lap a slow answer. If the lapped (stale) answer is
  // applied when it finally lands, it re-seeds the previous-track ref with the
  // dead pass as "live" — and the NEXT null poll announces the same handback a
  // second time. Stale in-flight answers must be dropped, not applied.
  it('announces the handback once when a stale in-flight answer lands after it', async () => {
    vi.mocked(pushToast).mockClear()
    const ended = trackStatus({
      state: 'tracking',
      mode: 'doppler-only',
      transponder: 'SSB/CW linear transponder',
      aosUnix: NOW - 700,
      losUnix: NOW - 5,
    })
    const pending: Array<(t: SatTrackStatus | null) => void> = []
    api.getSatTrackStatus.mockImplementation(
      () => new Promise<SatTrackStatus | null>((res) => pending.push(res)),
    )
    render(<SatellitesView focusSat="RS-44" />)
    // Poll 1 answers live promptly; poll 2 stalls in flight.
    await waitFor(() => expect(pending.length).toBeGreaterThanOrEqual(1))
    pending[0](ended)
    // Poll 3 overtakes it with the truth: the track is gone — one toast.
    await waitFor(() => expect(pending.length).toBeGreaterThanOrEqual(3), { timeout: 6000 })
    pending[2](null)
    await waitFor(() => expect(vi.mocked(pushToast)).toHaveBeenCalledTimes(1))
    // The lapped poll-2 answer finally lands, carrying the dead pass as live.
    pending[1](ended)
    // Poll 4 sees null again — a second "pass complete" would be a false claim.
    await waitFor(() => expect(pending.length).toBeGreaterThanOrEqual(4), { timeout: 6000 })
    pending[3](null)
    await new Promise((r) => setTimeout(r, 50))
    expect(vi.mocked(pushToast)).toHaveBeenCalledTimes(1)
    // Real timers ride the component's own 2 s poll cadence: four ticks.
  }, 15000)
})
