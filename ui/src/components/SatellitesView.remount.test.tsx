// @vitest-environment jsdom
//
// THE LOG FORM SURVIVES EVERYTHING THE SECTION DOES AROUND IT.
//
// A SHIPPED DATA-LOSS BUG, found while rebuilding the pass console 2026-08-03.
// The log strip was rendered inside `{selected && detail && …}` — a descendant
// of the bird's detail card. `LogEntry` keeps every field (call, RST, name,
// comment, the park row, the other-radio override) in its OWN local state and
// is not keyed or lifted, so unmounting it is not a re-render: it is a delete.
//
// Which meant that mid-pass, between overs, with a callsign half typed:
//   · pressing ✕, or
//   · pressing Escape (the section's own close), or
//   · clicking a different bird, or
//   · the 60 s detail poll answering with `null` because SatNOGS went away
// silently destroyed what the operator had typed. That is the worst possible
// moment for it — he has one pass, a few minutes long, and he is typing between
// overs while watching the dome.
//
// THE FIX IS STRUCTURAL, not a guard: the strip is now a SIBLING of
// `.sats-detail` inside `.sats-side`, gated on the engine snapshot alone. No
// bird state, no track state, no pass state can reach it. This file is the
// proof, in the shape of `Conversation.remount.test.tsx` — assert on the value
// the operator can still see, never on a render count.
//
// It is also why the strip's gate must never be tightened back for a layout
// reason. If a later change wants the log inside a conditional, it has to lift
// LogEntry's state first, and this file is what will say so.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, cleanup, act, waitFor } from '@testing-library/react'
import { SatellitesView } from './SatellitesView'
import type { AppSnapshot, SatDetail, SatTrackStatus, SatTransponderHeld } from '../types'

const api = vi.hoisted(() => ({
  getSatellites: vi.fn(() => Promise.resolve(null)),
  getSatPassNeeds: vi.fn(() => Promise.resolve([])),
  getSatDetail: vi.fn(),
  getSettings: vi.fn(),
  setSettings: vi.fn(() => Promise.resolve({} as never)),
  setPegLock: vi.fn(() => Promise.resolve()),
  confirmSatUplink: vi.fn(() => Promise.resolve()),
  setSatTransponder: vi.fn(() => Promise.resolve()),
  getSatTransponder: vi.fn((): Promise<SatTransponderHeld | null> => Promise.resolve(null)),
  startSatTrack: vi.fn(() => Promise.resolve(null)),
  stopSatTrack: vi.fn(() => Promise.resolve()),
  getSatTrackStatus: vi.fn((): Promise<SatTrackStatus | null> => Promise.resolve(null)),
  fetchTlesNow: vi.fn(() => Promise.resolve(null)),
  fdLogManual: vi.fn(async () => ({})),
  logQso: vi.fn(async () => ({})),
  getLog: vi.fn(async () => []),
  lookupPark: vi.fn(async () => null),
  lookupParkLive: vi.fn(async () => null),
  qrzLookup: vi.fn(async () => null),
  resolveEntity: vi.fn(async () => null),
  searchParks: vi.fn(async () => []),
  setCwPeerInfo: vi.fn(async () => {}),
}))
vi.mock('../api', () => api)
vi.mock('./MapView', () => ({ MapView: () => null }))
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: async <T,>(action: () => Promise<T>) => action(),
}))

const NOW = Math.floor(Date.now() / 1000)
const AOS = NOW - 300
const LOS = NOW + 300

const detail = (name = 'RS-44', norad = 44909): SatDetail =>
  ({
    name,
    norad,
    status: 'alive',
    transmitters: [
      { description: 'SSB/CW linear transponder', alive: true, kind: 'Transponder', invert: true },
    ],
    dataFetchedAt: 1_760_000_000,
    pass: {
      name,
      aosUnix: AOS,
      losUnix: LOS,
      maxElDeg: 62,
      aosAzDeg: 100,
      losAzDeg: 260,
      status: 'alive',
    },
    passTrack: [
      [AOS, 100, 0],
      [NOW, 180, 62],
      [LOS, 260, 0],
    ],
  }) as unknown as SatDetail

const status = (over: Partial<SatTrackStatus> = {}): SatTrackStatus =>
  ({
    name: 'RS-44',
    state: 'tracking',
    mode: 'rotor+doppler',
    dopplerDownlink: true,
    dopplerUplink: true,
    uplinkOffer: 'none',
    uplinkOfferMap: null,
    uplinkRadio: 'IC-9700',
    uplinkRadioId: 1,
    azDeg: 141,
    elDeg: 46,
    aosAzDeg: 100,
    maxElDeg: 45,
    satAzDeg: 143,
    satElDeg: 47,
    downlinkHz: 435_643_320,
    uplinkHz: 145_962_680,
    transponder: 'SSB/CW linear transponder',
    transponderIndex: 0,
    inverting: true,
    offsetHz: 3200,
    halfWidthHz: 12_500,
    elementAgeDays: 1.2,
    elementEpochUnix: 1_785_442_400,
    aosUnix: AOS,
    losUnix: LOS,
    ...over,
  }) as SatTrackStatus

const snap = (): AppSnapshot =>
  ({
    mycall: 'KD9TAW',
    mygrid: 'EN52',
    hunt: null,
    fieldDay: null,
    link: { tier: 'TempoFast' },
    radio: {
      dialMhz: 435.64332,
      band: '70cm',
      rigMode: 'USB',
      sideband: 'USB',
      catOk: true,
      transmitting: false,
      txEnabled: true,
      txAllowed: true,
    },
  }) as unknown as AppSnapshot

const settings = () => ({
  mygrid: 'EN52',
  rotatorModel: 2,
  rotatorHost: '',
  satDopplerOff: false,
  satVfoMap: 'main-down-sub-up',
})

beforeEach(() => {
  localStorage.clear()
  api.getSatDetail.mockReset()
  api.getSatDetail.mockImplementation(() => Promise.resolve(detail()))
  api.getSettings.mockReset()
  api.getSettings.mockImplementation(() => Promise.resolve(settings()))
  api.getSatTrackStatus.mockReset()
  api.getSatTrackStatus.mockImplementation(() => Promise.resolve(null))
  api.getSatTransponder.mockReset()
  api.getSatTransponder.mockImplementation(() => Promise.resolve(null))
})
afterEach(cleanup)

/** Type a partial contact into the section's log strip, the way an operator
 *  does between overs: a callsign and a report, not yet committed. */
async function halfTypeAContact() {
  const call = await screen.findByPlaceholderText('Call')
  await act(async () => {
    fireEvent.change(call, { target: { value: 'W1AW' } })
  })
  return call as HTMLInputElement
}

/** The live value of the call field — the thing the operator can still see.
 *  Re-queried every time, so a REMOUNT (a fresh input at the same place) is
 *  caught rather than papered over by a stale node reference. */
const callValue = () =>
  (screen.getByPlaceholderText('Call') as HTMLInputElement).value

describe('a half-typed contact survives the section moving around it', () => {
  it('survives ✕ — closing the detail is navigation, not a reset', async () => {
    render(<SatellitesView focusSat="RS-44" snap={snap()} />)
    await halfTypeAContact()
    expect(callValue()).toBe('W1AW')

    await act(async () => {
      fireEvent.click(document.querySelector<HTMLButtonElement>('.sats-detail-close')!)
    })
    await waitFor(() => expect(document.querySelector('.sats-detail')).toBeNull())

    expect(
      callValue(),
      'closing the bird destroyed a half-typed contact — the log strip is nested in the detail card again',
    ).toBe('W1AW')
  })

  it('survives Escape (the section’s own close path)', async () => {
    render(<SatellitesView focusSat="RS-44" snap={snap()} />)
    await halfTypeAContact()
    await act(async () => {
      // Escape inside a text field belongs to that field, so the section's
      // handler deliberately ignores it there — fire it where it closes.
      fireEvent.keyDown(window, { key: 'Escape' })
    })
    await waitFor(() => expect(document.querySelector('.sats-detail')).toBeNull())
    expect(callValue(), 'Escape destroyed a half-typed contact').toBe('W1AW')
  })

  it('survives switching birds mid-entry', async () => {
    const { rerender } = render(<SatellitesView focusSat="RS-44" snap={snap()} />)
    await halfTypeAContact()

    api.getSatDetail.mockImplementation(() => Promise.resolve(detail('AO-91', 43017)))
    rerender(<SatellitesView focusSat="AO-91" snap={snap()} />)
    await waitFor(() =>
      expect(document.querySelector('.sats-arm-id')!.textContent).toMatch(/AO-91/),
    )

    expect(callValue(), 'switching birds destroyed a half-typed contact').toBe('W1AW')
  })

  it('survives the detail fetch losing SatNOGS', async () => {
    // `getSatDetail` rejecting sets `detail` to null with `selected` still set —
    // the backend cannot reach SatNOGS, which is routine in a field shack. The
    // detail card disappears on its own, with NO operator action at all. Nested,
    // that alone wiped the form: the failure mode nobody would ever reproduce
    // on purpose. Driven here by switching birds into a rejecting fetch, which
    // is the same state transition the 60 s poll makes without needing to mock
    // the clocks this component genuinely runs on.
    const { rerender } = render(<SatellitesView focusSat="RS-44" snap={snap()} />)
    await halfTypeAContact()
    expect(document.querySelector('.sats-detail')).not.toBeNull()

    api.getSatDetail.mockImplementation(() => Promise.reject(new Error('offline')))
    rerender(<SatellitesView focusSat="AO-91" snap={snap()} />)
    await waitFor(() => expect(document.querySelector('.sats-detail')).toBeNull())

    expect(callValue(), 'losing the detail destroyed a half-typed contact').toBe('W1AW')
  })

  it('survives a pass ARMING and reaching AOS under it', async () => {
    // The arm bar, the readiness rail and the Doppler readout all appear and
    // then change state while the operator types. None of them is an ancestor
    // of the strip any more, and this is what says so.
    render(<SatellitesView focusSat="RS-44" snap={snap()} />)
    await halfTypeAContact()

    api.getSatTrackStatus.mockImplementation(() =>
      Promise.resolve(status({ state: 'armed', azDeg: null, elDeg: null })),
    )
    await waitFor(() => expect(screen.queryByTestId('sat-rail')).not.toBeNull(), { timeout: 4000 })
    expect(callValue(), 'arming a pass destroyed a half-typed contact').toBe('W1AW')

    api.getSatTrackStatus.mockImplementation(() => Promise.resolve(status()))
    await waitFor(
      () =>
        expect(document.querySelector('.sat-doppler')!.textContent).toMatch(/435\.64332/),
      { timeout: 4000 },
    )
    expect(callValue(), 'AOS destroyed a half-typed contact').toBe('W1AW')
  })

  it('the strip renders from the snapshot alone — no bird open, no track', async () => {
    // The gate that makes all of the above true, asserted directly: with
    // nothing selected and nothing armed, the log is still there. If a later
    // change re-nests it, this is the line that fails first.
    render(<SatellitesView snap={snap()} />)
    const call = await screen.findByPlaceholderText('Call')
    expect(document.querySelector('.sats-detail'), 'no bird should be open').toBeNull()
    expect(screen.queryByTestId('sat-rail'), 'no track should be armed').toBeNull()
    expect(call.closest('.sats-log'), 'the strip lost its wrapper').not.toBeNull()
    expect(call.closest('.sats-detail'), 'the strip is inside the detail card again').toBeNull()
  })
})
