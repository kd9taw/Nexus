// @vitest-environment jsdom
//
// LOGGING A CONTACT FROM THE SATELLITES SECTION.
//
// Operator, 0.27.3, after a clean pass with the manual rotor: "the problem came
// when I tried to log someone, as I dont have a spot to log within the
// satellites section to log my sat qso's. Lets not reinvent anything, logging
// sections exist in the phone area, can we drop that in?"
//
// Three claims are pinned here:
//
//  1. The section renders the SHARED `LogEntry` — the same component the Phone
//     and CW cockpits use — not a second log panel. A copy is exactly the thing
//     the operator asked us not to build.
//  2. It sits in the one `.sats-side` scroller, hard against the Doppler
//     readout and AHEAD of the transponder cards and the globe, because an
//     operator turning a rotator by hand has seconds between overs and will not
//     scroll a column past two full-height graphics to reach a form. Asserted
//     as DOCUMENT ORDER, which is his scroll order.
//  3. What reaches `logQso` is an ORDINARY CONTACT. No `propMode`, no
//     `satName`, nothing derived from the bird — satellite tagging is not done
//     yet, and a QSO record is permanent. Asserting on the RECORD, never on
//     what the panel displays: the display is not what LoTW reads.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, cleanup, act, waitFor } from '@testing-library/react'
import { SatellitesView } from './SatellitesView'
import type { AppSnapshot, LoggedQso, SatDetail, SatTrackStatus, SatTransponderHeld } from '../types'

const api = vi.hoisted(() => ({
  // The section's own surface.
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
  // LogEntry's surface (it renders REAL here — it is half the thing under test).
  fdLogManual: vi.fn(async () => ({})),
  logQso: vi.fn(async () => ({})),
  getLog: vi.fn(async () => [] as LoggedQso[]),
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

/** RS-44's card, mid-pass: a linear bird with a beacon and a transponder. */
const detail = (over: Partial<SatDetail> = {}): SatDetail =>
  ({
    name: 'RS-44',
    norad: 44909,
    status: 'alive',
    transmitters: [
      { description: 'CW beacon', alive: true, kind: 'Transmitter', invert: false },
      {
        description: 'SSB/CW linear transponder',
        alive: true,
        kind: 'Transponder',
        invert: true,
        downlinkMode: 'USB',
        uplinkMode: 'LSB',
      },
    ],
    dataFetchedAt: 1_760_000_000,
    pass: {
      name: 'RS-44',
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
    ...over,
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
    rangeKm: 812,
    rangeRateKmS: -5.42,
    downlinkHz: 435_643_320,
    uplinkHz: 145_962_680,
    downlinkShiftHz: -2310,
    uplinkShiftHz: 770,
    transponder: 'SSB/CW linear transponder',
    transponderIndex: 1,
    inverting: true,
    offsetHz: 3200,
    halfWidthHz: 12_500,
    elementAgeDays: 1.2,
    elementEpochUnix: 1_785_442_400,
    aosUnix: AOS,
    losUnix: LOS,
    ...over,
  }) as SatTrackStatus

const held = (): SatTransponderHeld =>
  ({
    name: 'RS-44',
    index: 1,
    description: 'SSB/CW linear transponder',
    binding: null,
  }) as SatTransponderHeld

/** The rig on the DOWNLINK — 70 cm USB, which is what the operator's dial reads
 *  while the engine corrects RS-44's downlink. */
const snap = (over: Record<string, unknown> = {}): AppSnapshot =>
  ({
    mycall: 'KD9TAW',
    mygrid: 'EN52',
    hunt: null,
    radio: {
      dialMhz: 435.64332,
      band: '70cm',
      rigMode: 'USB',
      sideband: 'USB',
      catOk: true,
      transmitting: false,
      txEnabled: true,
      txAllowed: true,
      ...over,
    },
  }) as unknown as AppSnapshot

const settings = (over: Record<string, unknown> = {}) => ({
  mygrid: 'EN52',
  rotatorModel: 2,
  rotatorHost: '',
  satDopplerOff: false,
  satVfoMap: 'main-down-sub-up',
  ...over,
})

beforeEach(() => {
  localStorage.clear()
  api.getSatDetail.mockReset()
  api.getSatDetail.mockImplementation(() => Promise.resolve(detail()))
  api.getSettings.mockReset()
  api.getSettings.mockImplementation(() => Promise.resolve(settings()))
  api.getSatTrackStatus.mockReset()
  api.getSatTrackStatus.mockImplementation(() => Promise.resolve(status()))
  api.getSatTransponder.mockReset()
  api.getSatTransponder.mockImplementation(() => Promise.resolve(held()))
  api.logQso.mockClear()
})
afterEach(cleanup)

/** Type a call into the section's log strip and commit it. */
async function logCall(call: string) {
  const strip = await screen.findByPlaceholderText('Call')
  await act(async () => {
    fireEvent.change(strip, { target: { value: call } })
  })
  await act(async () => {
    fireEvent.click(screen.getByRole('button', { name: 'Log' }))
  })
  await waitFor(() => expect(api.logQso).toHaveBeenCalled())
  return lastLoggedRecord()
}

/** The record handed to `logQso` — what the backend (and then ADIF, and then
 *  LoTW) actually receives. Every assertion below is on THIS, never on what the
 *  panel renders. */
function lastLoggedRecord(): LoggedQso {
  const calls = api.logQso.mock.calls as unknown as LoggedQso[][]
  return calls[calls.length - 1][0]
}

describe('Satellites — logging the contact you just made', () => {
  it('renders the SHARED log strip in the section, not a second panel', async () => {
    render(<SatellitesView focusSat="RS-44" snap={snap()} />)
    const strip = await screen.findByPlaceholderText('Call')

    // The shared component's own root, and its own commit row. A section-local
    // copy would not carry these — and a copy is the thing the operator asked
    // us NOT to build ("logging sections exist in the phone area, can we drop
    // that in?").
    const panel = strip.closest('.log-entry')
    expect(panel, 'the log strip is not the shared LogEntry').not.toBeNull()
    expect(panel!.querySelector('.le-log-btn'), 'no commit row').not.toBeNull()
    // Two features nothing but LogEntry has: the POTA/SOTA park row and the
    // other-radio override. Their presence is the proof this is the cockpits'
    // strip rather than a lookalike built for this section.
    expect(panel!.querySelector('.le-park-prog'), 'no park row').not.toBeNull()
    expect(panel!.querySelector('.le-override-toggle'), 'no other-radio override').not.toBeNull()

    // Exactly one of them. Two log panels in one column is the bug this test exists to catch.
    expect(document.querySelectorAll('.log-entry').length).toBe(1)
  })

  it('sits with the Doppler readout, ahead of the transponder cards and the globe', async () => {
    render(<SatellitesView focusSat="RS-44" snap={snap()} />)
    await screen.findByPlaceholderText('Call')

    const side = document.querySelector('.sats-side')!
    const panel = document.querySelector('.log-entry')!
    const readout = document.querySelector('.sat-doppler')!
    const cards = document.querySelector('[data-testid="sat-tp-list"]')!
    const globe = document.querySelector('[data-testid="sat-globe-box"]')!

    // ONE scroll owner: the strip lives in the column that already owns the
    // overflow and adds no scroller of its own.
    expect(side.contains(panel)).toBe(true)
    expect(panel.closest('.sats-detail')).not.toBeNull()

    // DOCUMENT ORDER is the operator's scroll order. Readout → log → cards →
    // globe: he types with the readout still on screen, and never travels past
    // the two square graphics to reach the form.
    expect(
      readout.compareDocumentPosition(panel) & Node.DOCUMENT_POSITION_FOLLOWING,
      'log strip is above the Doppler readout',
    ).toBeTruthy()
    expect(
      panel.compareDocumentPosition(cards) & Node.DOCUMENT_POSITION_FOLLOWING,
      'transponder cards are above the log strip',
    ).toBeTruthy()
    expect(
      panel.compareDocumentPosition(globe) & Node.DOCUMENT_POSITION_FOLLOWING,
      'the globe is above the log strip',
    ).toBeTruthy()
  })

  it('logs an ORDINARY contact — no satellite fields reach the record', async () => {
    // THE DESCOPE, pinned. The panel answers "somewhere to log without leaving
    // the section" and nothing more: satellite tagging (ADIF PROP_MODE +
    // SAT_NAME, which LoTW needs for satellite credit) is not done yet, and a
    // guessed SAT_NAME is worse than none — LoTW rejects a name it does not
    // recognise. ABSENT, not null: `undefined`/null would still serialize a
    // decision the operator never made.
    render(<SatellitesView focusSat="RS-44" snap={snap()} />)
    const rec = await logCall('W1AW')

    expect(Object.prototype.hasOwnProperty.call(rec, 'propMode')).toBe(false)
    expect(Object.prototype.hasOwnProperty.call(rec, 'satName')).toBe(false)
    // Nothing else derived from the bird either — no name in any field that
    // reaches the log. (`comment`/`notes` are the plausible smuggling routes.)
    expect(JSON.stringify(rec)).not.toContain('RS-44')

    // What it DOES carry is the station state, exactly as the Phone strip
    // carries it: the operator's own dial and band.
    expect(rec.call).toBe('W1AW')
    expect(rec.band).toBe('70cm')
    expect(rec.freqMhz).toBeCloseTo(435.64332, 5)
  })

  it('logs no satellite fields even with the transponder still HELD', async () => {
    // The mirror of the engine-side regression fix
    // (a_qso_logged_while_a_transponder_is_held_carries_no_satellite_fields).
    // A held bird used to tag the record at the backend funnel; the section
    // must not put one back on the front end either.
    render(<SatellitesView focusSat="RS-44" snap={snap()} />)
    const rec = await logCall('W1AW')
    expect(rec.propMode ?? null).toBeNull()
    expect(rec.satName ?? null).toBeNull()
  })

  it('says plainly, in the section, that the contact is not tagged as a satellite QSO', async () => {
    // Operator-facing honesty: nobody should wait months for LoTW satellite
    // credit that was never requested. If this line goes, the panel starts
    // lying by omission.
    render(<SatellitesView focusSat="RS-44" snap={snap()} />)
    await screen.findByPlaceholderText('Call')
    const note = document.querySelector('.sats-log-note')!
    expect(note, 'the honesty line is gone').not.toBeNull()
    expect(note.textContent).toMatch(/not.*tagged as a satellite QSO/i)
    expect(note.textContent).toMatch(/PROP_MODE/)
    expect(note.textContent).toMatch(/SAT_NAME/)
  })

  it('logs the mode the station is on, folded to the closed ADIF enumeration', async () => {
    // The same rule and the same field the Phone strip logs on. USB is an ADIF
    // SUBMODE — `<MODE>USB` gets the whole record rejected on LoTW upload — so
    // it folds to SSB. Nothing about the bird feeds this.
    render(<SatellitesView focusSat="RS-44" snap={snap()} />)
    expect((await logCall('W1AW')).mode).toBe('SSB')
  })

  it('logs FM when the station is on FM, and CW with the right default report', async () => {
    render(<SatellitesView focusSat="RS-44" snap={snap({ sideband: 'FM' })} />)
    expect((await logCall('W1AW')).mode).toBe('FM')
    cleanup()

    api.logQso.mockClear()
    render(<SatellitesView focusSat="RS-44" snap={snap({ sideband: 'CW' })} />)
    const rec = await logCall('W1AW')
    expect(rec.mode).toBe('CW')
    expect(rec.rstSent, 'a CW contact defaults to a three-digit report').toBe('599')
  })
})
