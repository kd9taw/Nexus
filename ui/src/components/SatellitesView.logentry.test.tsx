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
 *  while the engine corrects RS-44's downlink. `top` sets snapshot-level fields
 *  (the Field Day status, the active tier); `over` sets radio ones. */
const snap = (over: Record<string, unknown> = {}, top: Record<string, unknown> = {}): AppSnapshot =>
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
      ...over,
    },
    ...top,
  }) as unknown as AppSnapshot

/** Field Day running, the way `snap.fieldDay` arrives from the engine. */
const fieldDay = () => ({
  myClass: '1D',
  mySection: 'IL',
  running: true,
  state: 'running',
  qsoCount: 4,
  sections: 2,
  points: 8,
  log: [],
})

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
  api.fdLogManual.mockClear()
  api.qrzLookup.mockClear()
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
    // Three features nothing but LogEntry has: the other-radio override, the
    // callbook lookup and the private notes box. Their presence is the proof this
    // is the cockpits' strip rather than a lookalike built for this section.
    // (The park row used to stand here as the fourth. It is gone from this
    // section on purpose — see 'asks for no park row' below — so it can no longer
    // be the evidence.)
    expect(panel!.querySelector('.le-override-toggle'), 'no other-radio override').not.toBeNull()
    expect(panel!.querySelector('.le-lookup'), 'no callbook lookup').not.toBeNull()
    expect(panel!.querySelector('.le-notes'), 'no notes box').not.toBeNull()

    // Exactly one of them. Two log panels in one column is the bug this test exists to catch.
    expect(document.querySelectorAll('.log-entry').length).toBe(1)
  })

  it('is a SIBLING of the detail card, in the pass column, above Birds', async () => {
    // ⚠️ THIS ASSERTION INVERTED ON 2026-08-03, AND THE ARGUMENT MATTERS MORE
    // THAN THE ASSERTION — read it before "fixing" it back.
    //
    // WHAT IT USED TO SAY. The section was one long scrolling column: Doppler
    // readout → log → transponder cards → globe. Document order WAS scroll
    // order, and the claim was that an operator with seconds between overs
    // would never travel past two full-height square graphics to reach a form,
    // so the form had to come before them.
    //
    // WHY THAT IS THE WRONG QUESTION NOW. The pass rebuild took the surfaces
    // that argument was about OUT of this column entirely: the frequencies and
    // the transponder chooser are in the planning column (`.sats-radio`,
    // permanently on screen, never scrolled), and the globe sits beside the sky
    // dome in one bounded row. Nothing the operator needs mid-pass is behind the
    // form any more, so there is no travel to protect him from. What is left in
    // the pass column is the pass, the log, and the Birds catalog — which is the
    // one surface the operator himself put below the fold ("You can always
    // scroll down").
    //
    // THE NEW INVARIANT IS STRONGER THAN AN ORDERING, AND IT IS A BUG FIX.
    // LogEntry keeps every field in local state. Nested inside
    // `{selected && detail && …}` — which is where it was — hitting ✕, pressing
    // Escape, or clicking another bird UNMOUNTED it and destroyed a half-typed
    // contact, mid-pass, between overs. It is a sibling of `.sats-detail` now,
    // gated on the engine snapshot alone. `SatellitesView.remount.test.tsx`
    // proves the consequence; this proves the structure that allows it.
    render(<SatellitesView focusSat="RS-44" snap={snap()} />)
    await screen.findByPlaceholderText('Call')

    const side = document.querySelector('.sats-side')!
    const panel = document.querySelector('.log-entry')!
    const wrap = document.querySelector('.sats-log')!
    const detailCard = document.querySelector('.sats-detail')!
    const birds = document.querySelector('.sats-favmgr')!
    const cards = document.querySelector('[data-testid="sat-tp-list"]')!

    // ONE scroll owner: the strip lives in the column that already owns the
    // overflow and adds no scroller of its own.
    expect(side.contains(panel)).toBe(true)
    expect(wrap.querySelector('.log-entry')).toBe(panel)

    // A SIBLING, NOT A DESCENDANT. This is the whole fix: nothing about which
    // bird is open can unmount the form.
    expect(panel.closest('.sats-detail'), 'the log is nested in the detail card again').toBeNull()
    expect(wrap.parentElement).toBe(side)
    expect(detailCard.parentElement).toBe(side)

    // Pass card → log → Birds. Birds is the deliberate below-the-fold block.
    expect(
      detailCard.compareDocumentPosition(wrap) & Node.DOCUMENT_POSITION_FOLLOWING,
      'the log strip is above the pass card',
    ).toBeTruthy()
    expect(
      wrap.compareDocumentPosition(birds) & Node.DOCUMENT_POSITION_FOLLOWING,
      'the Birds catalog is above the log strip',
    ).toBeTruthy()

    // The transponder chooser is in the OTHER column now — permanently on
    // screen, never behind the form. That is what retires the old ordering.
    expect(side.contains(cards), 'the chooser is back in the scrolling pass column').toBe(false)
    expect(cards.closest('.sats-plan'), 'the chooser left the planning column').not.toBeNull()
  })

  it('asks for no park row — there is no POTA/SOTA reference on a bird', async () => {
    // Operator, 0.28.1: "that section still has a pota/sota section, which is
    // shouldnt". NOT ASKED FOR rather than hidden: the section passes
    // `exchange="satellite"`, an exchange with no park in it, so the picker, the
    // reference search and the park-detail chip are never rendered — and they
    // take no vertical space in a column already shared with the sky dome (the
    // same column he asked to make smaller one message earlier).
    render(<SatellitesView focusSat="RS-44" snap={snap()} />)
    const panel = (await screen.findByPlaceholderText('Call')).closest('.log-entry')!
    expect(panel.querySelector('.le-park-row'), 'the POTA/SOTA row is still here').toBeNull()
    expect(panel.querySelector('.le-park-prog')).toBeNull()
    expect(panel.querySelector('.le-park-search')).toBeNull()
    expect(screen.queryByPlaceholderText(/^Park \(/)).toBeNull()
    expect(screen.queryByPlaceholderText(/^Summit \(/)).toBeNull()
  })

  it('takes the GRID the station passed you, and it reaches the record', async () => {
    // Operator, same message: "what about the sat, gridsquares features to log".
    // Grid-for-grid IS the satellite exchange — on a bird it is most of the
    // contact. The field was state-only before this: callbook-filled, printed in
    // the summary line, and impossible to TYPE, so a square passed on air could
    // not be logged at all. Asserted on the RECORD, never on the display.
    render(<SatellitesView focusSat="RS-44" snap={snap()} />)
    const call = await screen.findByPlaceholderText('Call')
    const grid = screen.getByPlaceholderText('Grid')
    await act(async () => {
      fireEvent.change(call, { target: { value: 'W1AW' } })
    })
    await act(async () => {
      fireEvent.change(grid, { target: { value: 'fn31pr' } })
    })
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Log' }))
    })
    await waitFor(() => expect(api.logQso).toHaveBeenCalled())
    expect(lastLoggedRecord().grid).toBe('FN31PR')
  })

  // ---- NO PARK REACHES A SATELLITE RECORD, BY ANY PATH ----
  //
  // Removing the park ROW is not the same as removing the park. A pending
  // POTA/SOTA hunt is snapshot state that arrives whatever section the operator
  // is in, and the strip used to latch its reference into `logParkRef`. The
  // commit then asked "does this differ from the pending hunt?" and sent it as
  // `ota` when it did — which is exactly what a STALE latch looks like once the
  // hunt has ended or moved to another park. The record leaves as ADIF
  // SIG/SIG_INFO: a satellite contact filed as a park contact, with nothing on
  // screen to reveal, edit or clear it. That is worse than the visible row.
  //
  // Asserting on the RECORD in both cases, from the section, with the hunt in
  // the state that produces the stale latch — a chip check would pass while the
  // wire lied.
  const huntSnap = (hunt: unknown) =>
    snap({}, { hunt: hunt as never, mycall: 'KD9TAW' })

  async function logFromSection(call: string) {
    const field = await screen.findByPlaceholderText('Call')
    await act(async () => {
      fireEvent.change(field, { target: { value: call } })
    })
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Log' }))
    })
    await waitFor(() => expect(api.logQso).toHaveBeenCalled())
    return lastLoggedRecord()
  }

  it('carries no park reference after a hunt ENDS mid-pass', async () => {
    const { rerender } = render(
      <SatellitesView focusSat="RS-44" snap={huntSnap({ program: 'POTA', reference: 'K-1234', call: 'W1AW' })} />,
    )
    await screen.findByPlaceholderText('Call')
    // The activator logs out / the engine clears the pend: hunt gone, any latched
    // reference now differs from "no hunt at all" and reads as an explicit entry.
    await act(async () => {
      rerender(<SatellitesView focusSat="RS-44" snap={huntSnap(null)} />)
    })
    const rec = await logFromSection('W1AW')
    expect(rec.ota ?? null, 'a park reference rode onto a satellite contact').toBeNull()
    expect(JSON.stringify(rec)).not.toContain('K-1234')
  })

  it('carries no park reference after a hunt SWITCHES to another park', async () => {
    const { rerender } = render(
      <SatellitesView focusSat="RS-44" snap={huntSnap({ program: 'POTA', reference: 'K-1234', call: 'W1AW' })} />,
    )
    await screen.findByPlaceholderText('Call')
    await act(async () => {
      rerender(
        <SatellitesView focusSat="RS-44" snap={huntSnap({ program: 'POTA', reference: 'K-9999', call: 'W1AW' })} />,
      )
    })
    const rec = await logFromSection('W1AW')
    expect(rec.ota ?? null, 'a park reference rode onto a satellite contact').toBeNull()
    expect(JSON.stringify(rec)).not.toContain('K-1234')
    expect(JSON.stringify(rec)).not.toContain('K-9999')
  })

  it('carries no park reference while a hunt is still LIVE and matching', async () => {
    // The third state, for completeness: the engine's callsign auto-tag owns
    // this one, and the strip must not double-report it either.
    render(
      <SatellitesView focusSat="RS-44" snap={huntSnap({ program: 'POTA', reference: 'K-1234', call: 'W1AW' })} />,
    )
    const rec = await logFromSection('W1AW')
    expect(rec.ota ?? null).toBeNull()
    expect(JSON.stringify(rec)).not.toContain('K-1234')
  })

  it('a callbook square the operator never typed cannot strand him mid-pass', async () => {
    // QRZ answers `grid` with whatever is in the profile. An 8-character
    // extended locator (probed live: FN31PR99) filled the field, failed the
    // strip's 4/6 check and DISABLED Log — on a pass, over a value nobody typed.
    // 8 is a legal ADIF GRIDSQUARE, so it is accepted and logged whole.
    api.qrzLookup.mockResolvedValueOnce({ call: 'W1AW', grid: 'FN31PR99', name: 'Hiram' } as never)
    render(<SatellitesView focusSat="RS-44" snap={snap()} />)
    const call = await screen.findByPlaceholderText('Call')
    await act(async () => {
      fireEvent.change(call, { target: { value: 'W1AW' } })
    })
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Lookup' }))
    })
    await waitFor(() =>
      expect((screen.getByPlaceholderText('Grid') as HTMLInputElement).value).toBe('FN31PR99'),
    )
    const btn = screen.getByRole('button', { name: 'Log' }) as HTMLButtonElement
    expect(btn.disabled, 'the callbook locked the operator out of his own log').toBe(false)
    await act(async () => {
      fireEvent.click(btn)
    })
    await waitFor(() => expect(api.logQso).toHaveBeenCalled())
    expect(lastLoggedRecord().grid).toBe('FN31PR99')
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
    // AND that the cost is not only an upload one. Nexus decides "satellite
    // QSO?" locally from PROP_MODE (`qso_is_sat`), so the Satellite-VUCC totals
    // on the Awards screen and the satellite needs board miss it too. A note
    // that named only LoTW would leave an operator watching an in-app counter
    // that is never going to move.
    expect(
      note.textContent,
      'the note frames this as an upload-only matter — it is not',
    ).toMatch(/Nexus.s own satellite totals/i)
    // AND the two things that make "add the fields yourself" actionable rather
    // than a shrug. Both are pinned in Rust at their real sites — TQSL's
    // pair validation (see `Engine::log_qso`) and the award fold
    // (`a_two_metre_satellite_contact_is_credited_to_terrestrial_vucc`) — and
    // this asserts the operator is actually told: adding only one field gets
    // the record refused, and leaving both off is not a neutral omission on a
    // metre band, it hands the grid to the wrong award.
    expect(
      note.textContent,
      'the note says "add the fields" without saying it must be both — one alone is refused',
    ).toMatch(/both/i)
    expect(
      note.textContent,
      'the note calls this a missing credit; on 2 m it is a WRONG credit',
    ).toMatch(/terrestrial VUCC/i)
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

  // ---- THE TWO DISCLOSED DIVERGENCES ----
  //
  // Both are assertions on behaviour that is DEFERRED, not decided: the shared
  // strip was dropped in unchanged, and these are what "unchanged" costs. They
  // are pinned so the docs cannot drift off them, and so that whoever fixes
  // either one is handed the paragraphs to delete — these tests go red on the
  // fix, which is the coupling.

  it("records the tier's mode on a digital section — the old SSB fold is dead", async () => {
    // The sat-FT batch (2026-08-10) made this strip tier-aware: on a DIGITAL
    // operating section the record carries the tier's own registered name (the
    // engine's log-mode strings, used verbatim), never the sideband the data
    // mode was generated on. This flips the old disclosed defect
    // ("records SSB on a digital tier"); the guide's "does not do yet" note
    // and the CHANGELOG disclosure went with it.
    render(
      <SatellitesView
        focusSat="RS-44"
        snap={snap({ sideband: 'USB', operatingMode: 'digital' }, { link: { tier: 'Q65' } })}
      />,
    )
    expect((await logCall('W1AW')).mode).toBe('Q65')
    cleanup()

    // The SECTION gate is load-bearing: link.tier always names a tier (the
    // decoder is always set to something), so a voice pass worked from the
    // Phone section must still fold by SIDEBAND — labelling it FT8 would be
    // the same defect pointing the other way.
    api.logQso.mockClear()
    render(
      <SatellitesView
        focusSat="RS-44"
        snap={snap({ sideband: 'USB', operatingMode: 'phone' }, { link: { tier: 'FT8' } })}
      />,
    )
    expect((await logCall('W1AW')).mode).toBe('SSB')
    cleanup()

    // And an FM bird worked from a non-digital section still records FM.
    api.logQso.mockClear()
    render(
      <SatellitesView
        focusSat="RS-44"
        snap={snap({ sideband: 'FM', operatingMode: 'phone' }, { link: { tier: 'TempoFast' } })}
      />,
    )
    expect((await logCall('W1AW')).mode).toBe('FM')
  })

  it("the override's mode picker covers FT8/FT4 and no other digital tier", async () => {
    // ⚠️ THE WORKAROUND, MEASURED. The CHANGELOG and the guide send an operator
    // on a data mode to "Log a contact from another radio and pick the mode by
    // hand". That advice is only as good as the picker's option list, which is
    // LogEntry's `LOG_MODES` and nothing more — so it works on FT8 and FT4 and
    // has no entry at all for Q65, JT65, MSK144, WSPR, FST4/FST4W or Tempo.
    // Advice that fails when followed is worse than none, so the wording names
    // the two tiers it covers and sends the rest to the Logbook's free-text
    // Mode field. This test is what keeps those sentences honest: widen
    // `LOG_MODES` and it goes red, which is the cue to widen the prose too.
    render(<SatellitesView focusSat="RS-44" snap={snap()} />)
    const strip = await screen.findByPlaceholderText('Call')
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: /another radio/i }))
    })
    const picker = strip
      .closest('.log-entry')!
      .querySelector('.le-ov-mode') as HTMLSelectElement | null
    expect(picker, 'the override has no mode picker at all').not.toBeNull()
    const offered = Array.from(picker!.options).map((o) => o.value)
    expect(offered).toEqual(['SSB', 'FM', 'AM', 'CW', 'RTTY', 'FT8', 'FT4'])
    for (const tier of ['Q65', 'JT65', 'MSK144', 'WSPR', 'FST4', 'TempoFast', 'TempoDeep']) {
      expect(
        offered,
        `${tier} is now in the picker — the guide may stop sending it to the Logbook`,
      ).not.toContain(tier)
    }
  })

  it('logs to the ORDINARY log during Field Day — the disclosed FD divergence, NOT YET fixed', async () => {
    // App.tsx passes `fieldDay` to CwCockpit and PhoneCockpit, each of which
    // adds its own literal `fdMode` ("CW" / "PH") on the way down to LogEntry,
    // so those strips route through `fdLogManual` into the contest log while FD
    // runs. This section gets neither, so a satellite contact made during Field
    // Day lands in the general log and earns the club nothing — with FD visibly
    // running everywhere else in the app.
    render(<SatellitesView focusSat="RS-44" snap={snap({}, { fieldDay: fieldDay() })} />)
    const rec = await logCall('W1AW')
    expect(rec.call).toBe('W1AW')
    expect(
      api.fdLogManual,
      'the Satellites strip reached the Field Day log — delete the "not yet" note in the guide and the CHANGELOG',
    ).not.toHaveBeenCalled()
  })

  it('takes TWO Enters on a fresh call: the first looks it up, the second logs', async () => {
    // What the guide has to say, because it is what the strip does. The first
    // Enter on an un-enriched call fires the callbook lookup and swallows the
    // commit (`onCallEnter` → `triedLookupRef`); only the next one logs. A guide
    // that says "type the call, press Enter" sends an operator away from the
    // radio believing a contact is in the log when it is not.
    render(<SatellitesView focusSat="RS-44" snap={snap()} />)
    const call = await screen.findByPlaceholderText('Call')
    await act(async () => {
      fireEvent.change(call, { target: { value: 'W1AW' } })
    })
    await act(async () => {
      fireEvent.keyDown(call, { key: 'Enter' })
    })
    await waitFor(() => expect(api.qrzLookup).toHaveBeenCalledWith('W1AW'))
    expect(api.logQso, 'the first Enter logged the contact').not.toHaveBeenCalled()

    await act(async () => {
      fireEvent.keyDown(call, { key: 'Enter' })
    })
    await waitFor(() => expect(api.logQso).toHaveBeenCalled())
    expect(lastLoggedRecord().call).toBe('W1AW')
  })
})
