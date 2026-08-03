// @vitest-environment jsdom
//
// WHICH RIG WILL MOVE — the Satellites section's radio binding.
//
// Operator field report (IC-9700 as radio 1, FTDX10 as radio 0, dual-radio):
// "I have my 9700 selected, but in the sat area, it's not selecting the radio."
// The section showed NOTHING about radios: the readiness rail claimed Doppler
// "is steering the radio dial" without ever naming a dial, and the sat path had
// no routing call in it at all, so Doppler drove whatever rig happened to be
// active. The backend now routes a pick on band+mode class exactly as a repeater
// tune does and reports what it bound to; this pins the surface that shows it.
//
// What is pinned here:
//  - the binding line NAMES the routed radio (and the band+class it routed on),
//    so one glance answers "which rig will move";
//  - when nothing moved, the honest REASON is shown in place of frequencies —
//    never computed centres beside a radio that never budged;
//  - it reads the ENGINE's binding off the same read-back the hold uses, not
//    the last local click;
//  - the peg override is the app's existing one (Settings `radioPegged`,
//    read-modify-write like the two Doppler switches beside it).
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
import { SatellitesView } from './SatellitesView'
import type { SatDetail, SatTransponderHeld } from '../types'

const api = vi.hoisted(() => ({
  getSatellites: vi.fn(() => Promise.resolve(null)),
  getSatSchedule: vi.fn(() => Promise.resolve([])),
  getSatPassNeeds: vi.fn(() => Promise.resolve([])),
  getSatDetail: vi.fn(),
  getSettings: vi.fn(),
  setSettings: vi.fn((_s: unknown) => Promise.resolve({} as never)),
  setSatTransponder: vi.fn(() => Promise.resolve()),
  getSatTransponder: vi.fn((): Promise<SatTransponderHeld | null> => Promise.resolve(null)),
  startSatTrack: vi.fn(() => Promise.resolve(null)),
  stopSatTrack: vi.fn(() => Promise.resolve()),
  getSatTrackStatus: vi.fn(() => Promise.resolve(null)),
}))
vi.mock('../api', () => api)
vi.mock('./MapView', () => ({ MapView: () => null }))
vi.mock('../toast', () => ({ pushToast: vi.fn() }))

const detail = (): SatDetail => ({
  name: 'RS-44',
  norad: 44909,
  status: 'alive',
  transmitters: [
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
  pass: null,
  passTrack: [],
})

const settings = (over: Record<string, unknown> = {}) => ({
  mygrid: 'EN52',
  rotatorModel: 0,
  rotatorHost: '',
  satDopplerOff: false,
  satVfoMap: 'main-down-sub-up',
  radioPegged: false,
  ...over,
})

/** The engine's read-back after a pick the RIG CONFIRMED on the IC-9700. */
const heldTuned = (): SatTransponderHeld => ({
  name: 'RS-44',
  index: 0,
  description: 'SSB/CW linear transponder',
  binding: {
    radioId: 1,
    radioName: 'IC-9700',
    band: '70cm',
    fm: false,
    simplex: false,
    downlinkMhz: 435.64,
    uplinkMhz: 145.965,
    pendingDownlinkMhz: null,
    pendingUplinkMhz: null,
    note: null,
  },
})

beforeEach(() => {
  localStorage.clear()
  api.getSatDetail.mockReset()
  api.getSatDetail.mockImplementation(() => Promise.resolve(detail()))
  api.getSettings.mockReset()
  api.getSettings.mockImplementation(() => Promise.resolve(settings()))
  api.setSettings.mockReset()
  api.setSettings.mockImplementation(() => Promise.resolve({} as never))
  api.setSatTransponder.mockReset()
  api.setSatTransponder.mockImplementation(() => Promise.resolve())
  api.getSatTransponder.mockReset()
  api.getSatTransponder.mockImplementation(() => Promise.resolve(null))
})
afterEach(cleanup)

describe('the radio binding line', () => {
  it('names the radio the pick routed to, and the band+class it routed on', async () => {
    api.getSatTransponder.mockImplementation(() => Promise.resolve(heldTuned()))
    render(<SatellitesView focusSat="RS-44" />)
    const bind = await screen.findByTestId('sat-radio-binding')
    expect(bind.textContent).toMatch(/IC-9700/)
    expect(bind.textContent).toMatch(/70cm/)
    expect(bind.textContent).toMatch(/SSB/)
  })

  it('shows the frequencies it actually WROTE, both legs', async () => {
    // The operator's third complaint: "nexus should know the frequencies both
    // up and down across vhf/uhf". Down on the dial, up on the split TX leg.
    api.getSatTransponder.mockImplementation(() => Promise.resolve(heldTuned()))
    render(<SatellitesView focusSat="RS-44" />)
    const bind = await screen.findByTestId('sat-radio-binding')
    expect(bind.textContent).toMatch(/435\.640/)
    expect(bind.textContent).toMatch(/145\.965/)
  })

  it('shows a leg the rig has not acknowledged yet as still tuning — hollow dot, trailing ellipsis', async () => {
    // The sentinel rule, rendered: right after a pick the legs are REQUESTED,
    // not done — the engine confirms each one only when the radio loop reports
    // the rig's acknowledgment. 0.24.2 printed the computed centres with a
    // filled dot while the 9700 never received a byte (the field report).
    api.getSatTransponder.mockImplementation(() =>
      Promise.resolve({
        ...heldTuned(),
        binding: {
          radioId: 1,
          radioName: 'IC-9700',
          band: '70cm',
          fm: false,
          simplex: false,
          downlinkMhz: null,
          uplinkMhz: null,
          pendingDownlinkMhz: 435.64,
          pendingUplinkMhz: 145.965,
          note: null,
        },
      }),
    )
    render(<SatellitesView focusSat="RS-44" />)
    const bind = await screen.findByTestId('sat-radio-binding')
    // The frequencies show — marked in-flight, never claimed as landed.
    expect(bind.textContent).toMatch(/435\.640 ↓ …/)
    expect(bind.textContent).toMatch(/145\.965 ↑ …/)
    expect(bind.querySelector('.sat-rail-dot.ok')).toBeNull()
  })

  it('fills the dot only once every requested leg is confirmed on the wire', async () => {
    // Half-landed: the dial acked, the split still pending — still hollow.
    api.getSatTransponder.mockImplementation(() =>
      Promise.resolve({
        ...heldTuned(),
        binding: {
          radioId: 1,
          radioName: 'IC-9700',
          band: '70cm',
          fm: false,
          simplex: false,
          downlinkMhz: 435.64,
          uplinkMhz: null,
          pendingDownlinkMhz: null,
          pendingUplinkMhz: 145.965,
          note: null,
        },
      }),
    )
    render(<SatellitesView focusSat="RS-44" />)
    const bind = await screen.findByTestId('sat-radio-binding')
    expect(bind.textContent).toMatch(/435\.640 ↓/)
    expect(bind.textContent).toMatch(/145\.965 ↑ …/)
    expect(bind.querySelector('.sat-rail-dot.ok')).toBeNull()
  })

  it('prints the REASON instead of frequencies when nothing moved', async () => {
    // "None — leave the dial to me" keeps meaning exactly that. The hold still
    // stands; the radio does not move; the line says so rather than showing
    // computed centres beside a rig that never budged.
    api.getSatTransponder.mockImplementation(() =>
      Promise.resolve({
        ...heldTuned(),
        binding: {
          radioId: 1,
          radioName: 'IC-9700',
          band: '70cm',
          fm: false,
          simplex: false,
          downlinkMhz: null,
          uplinkMhz: null,
          pendingDownlinkMhz: null,
          pendingUplinkMhz: null,
          note: 'VFO mapping is None — the dial stays yours; nothing was tuned.',
        },
      }),
    )
    render(<SatellitesView focusSat="RS-44" />)
    const bind = await screen.findByTestId('sat-radio-binding')
    expect(bind.textContent).toMatch(/nothing was tuned/)
    expect(bind.textContent).not.toMatch(/435\.640/)
    // Not-ready is carried by SHAPE, the codebase idiom (survives greyscale).
    expect(bind.querySelector('.sat-rail-dot.ok')).toBeNull()
  })

  it("stays under the bird it belongs to — another bird's detail never wears it", async () => {
    // The engine's binding is GLOBAL (one hold at a time); the detail pane is
    // whichever bird was clicked. RS-44's rig, band and frequencies rendering
    // under AO-91's heading — with no bird named on the line — would
    // misattribute them; the chooser already calls out the cross-bird hold in
    // its own words ("Doppler holds a transponder on …"), the same idiom that
    // gates every other per-bird surface here (heldIndex, detailTrack).
    api.getSatTransponder.mockImplementation(() => Promise.resolve(heldTuned())) // RS-44 held
    api.getSatDetail.mockImplementation(() => Promise.resolve({ ...detail(), name: 'AO-91' }))
    render(<SatellitesView focusSat="AO-91" />)
    await screen.findByTestId('sat-tp-list')
    expect(screen.queryByTestId('sat-radio-binding')).toBeNull()
  })

  it('shows a refusal that never resolved a rig as the reason alone — no phantom rig, band or class', async () => {
    // A refusal that returns BEFORE routing carries no radioId, no radioName
    // and no band, and no class was ever chosen. Rendering the fallback rig
    // name and the separators anyway produced "this radio · · SSB — …" — a rig
    // never resolved and a routing class that never routed.
    api.getSatTransponder.mockImplementation(() =>
      Promise.resolve({
        ...heldTuned(),
        binding: {
          radioId: null,
          radioName: '',
          band: '',
          fm: false,
          simplex: false,
          downlinkMhz: null,
          uplinkMhz: null,
          pendingDownlinkMhz: null,
          pendingUplinkMhz: null,
          note: 'No transponder is held — nothing to tune to.',
        },
      }),
    )
    render(<SatellitesView focusSat="RS-44" />)
    const bind = await screen.findByTestId('sat-radio-binding')
    // The STATE span only — the peg button's own label legitimately says
    // "pin this radio" and is not the surface under test.
    const state = bind.querySelector('.sat-rail-state')!.textContent
    expect(state).toMatch(/nothing to tune to/)
    expect(state).not.toMatch(/this radio/)
    expect(state).not.toMatch(/SSB/)
    expect(state).not.toMatch(/· ·/)
  })

  it('names the rig for a downlink the band table cannot label — the chip drops, the radio does not', async () => {
    // QO-100 and the microwave birds have no band label (bandplan::band_for_dial
    // stops at 23 cm), and that used to be a REFUSAL. Now they tune, so an empty
    // band arrives alongside a resolved rig and a real leg. Keying the rig line
    // on the band — as this rail once did — would print "10489.550 ↓ …" beside
    // no radio at all. Absent is absent: the band chip goes, nothing else does.
    api.getSatTransponder.mockImplementation(() =>
      Promise.resolve({
        ...heldTuned(),
        binding: {
          radioId: 1,
          radioName: 'IC-9700',
          band: '',
          fm: false,
          simplex: false,
          downlinkMhz: null,
          uplinkMhz: null,
          pendingDownlinkMhz: 10489.55,
          pendingUplinkMhz: null,
          note: null,
        },
      }),
    )
    render(<SatellitesView focusSat="RS-44" />)
    const bind = await screen.findByTestId('sat-radio-binding')
    const state = bind.querySelector('.sat-rail-state')!.textContent
    expect(state).toMatch(/IC-9700/)
    expect(state).toMatch(/10489\.550 ↓ …/)
    expect(state).toMatch(/SSB/)
    // …and no empty band slot left behind where the chip used to be.
    expect(state).not.toMatch(/· ·/)
  })

  it('is absent until a transponder is held — nothing is about to move', async () => {
    render(<SatellitesView focusSat="RS-44" />)
    await screen.findByTestId('sat-tp-list')
    expect(screen.queryByTestId('sat-radio-binding')).toBeNull()
  })

  it('appears after a pick, from the ENGINE read-back rather than the click', async () => {
    // The hold is released backend-side (LOS, live-track stop). A binding drawn
    // from the last local click would name a rig the engine no longer drives.
    render(<SatellitesView focusSat="RS-44" />)
    expect(screen.queryByTestId('sat-radio-binding')).toBeNull()
    api.getSatTransponder.mockImplementation(() => Promise.resolve(heldTuned()))
    fireEvent.click(await screen.findByLabelText('Work SSB/CW linear transponder'))
    await waitFor(() => expect(api.setSatTransponder).toHaveBeenCalledWith('RS-44', 0))
    const bind = await screen.findByTestId('sat-radio-binding')
    expect(bind.textContent).toMatch(/IC-9700/)
  })

  it('overrides routing through the app’s own peg-lock, read-modify-write', async () => {
    // The existing override idiom (the TopBar RadioSwitcher's 🔒), reachable
    // where the operator is. Read-modify-write so the one field changes and
    // nothing else in the settings object rides along wrong.
    api.getSatTransponder.mockImplementation(() => Promise.resolve(heldTuned()))
    render(<SatellitesView focusSat="RS-44" />)
    await screen.findByTestId('sat-radio-binding')
    fireEvent.click(screen.getByRole('button', { name: /pin this radio/i }))
    await waitFor(() => expect(api.setSettings).toHaveBeenCalled())
    const written = api.setSettings.mock.calls[0][0] as Record<string, unknown>
    expect(written.radioPegged).toBe(true)
    expect(written.mygrid).toBe('EN52') // the rest of the object survived
  })
})
