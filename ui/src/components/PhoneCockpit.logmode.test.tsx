// @vitest-environment jsdom
//
// AN AM QSO MUST BE LOGGED AS AM (operator report, 2026-09-10, v1.11.1-beta.1).
//
// The operator held an AM contact on 14.286 — the 20 m AM calling frequency — and Nexus wrote
// it to the shared logbook as SSB. The screenshot showed BOTH halves of the defect at once: the
// top bar read `rig: AM`, because Nexus had read the rig's mode over CAT and noticed it
// disagreed with what it believed, and the log pane under it read "Logs to the shared logbook
// as SSB". Nexus knew the answer and logged something else.
//
// The cause was one expression in the Phone cockpit: the mode handed to LogEntry was
// `commandedMode === 'FM' ? 'FM' : 'SSB'`, a binary choice in which everything that is not FM
// is SSB. It could not say AM even when the operator had PICKED AM in the cockpit, and it never
// consulted the rig's own read-back at all.
//
// So this suite renders the REAL LogEntry inside the REAL PhoneCockpit with the props App gives
// it and asserts what reaches `logQso` — the wire value that becomes `<MODE:2>AM` in the ADIF
// and goes to QRZ/LoTW/ClubLog/eQSL. A stub of LogEntry would only prove a prop was passed; the
// question here is what gets WRITTEN.
//
// Both directions matter. Labelling an SSB contact AM is exactly as wrong as the reported bug,
// so the ordinary-SSB control and the cases where the read-back must NOT be believed (no CAT, a
// mode Nexus cannot map to a phone Mode) are asserted alongside it.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, within, fireEvent, cleanup, waitFor } from '@testing-library/react'
import { PhoneCockpit } from './PhoneCockpit'
import { logQso } from '../api'
import type { AppSnapshot } from '../types'

vi.mock('../api', () => ({
  // The Phone cockpit reads the FM repeater shift from Settings — it is the only surface
  // that carries it, and the transmit contract will not state a frequency without it.
  getSettings: vi.fn(async () => ({})),
  // Phone cockpit's own seams.
  setPtt: vi.fn(async () => {}),
  setTxEnabled: vi.fn(async () => ({})),
  setRfPower: vi.fn(async () => {}),
  setMicGain: vi.fn(async () => {}),
  setNrLevel: vi.fn(async () => {}),
  setAgc: vi.fn(async () => ({})),
  setScopeSpan: vi.fn(async () => ({})),
  setScopeRef: vi.fn(async () => {}),
  setFlexPanSpan: vi.fn(async () => ({})),
  setFlexPanRef: vi.fn(async () => ({})),
  startQsoRecording: vi.fn(async () => ({})),
  stopQsoRecording: vi.fn(async () => ({})),
  setTune: vi.fn(async () => ({})),
  haltTx: vi.fn(async () => ({})),
  setFrequency: vi.fn(async () => ({})),
  setSplit: vi.fn(async () => ({})),
  setRigFunc: vi.fn(async () => ({})),
  setSidebandOverride: vi.fn(async () => ({})),
  setFilterWidth: vi.fn(async () => ({})),
  openPanelWindow: vi.fn(async () => {}),
  // LogEntry's — the component under test here, rendered for real.
  contestIMoved: vi.fn(async () => ({})),
  contestLogManual: vi.fn(async () => ({})),
  logQso: vi.fn(async () => ({})),
  getLog: vi.fn(async () => []),
  lookupPark: vi.fn(async () => null),
  lookupParkLive: vi.fn(async () => null),
  qrzLookup: vi.fn(async () => null),
  resolveEntity: vi.fn(async () => null),
  searchParks: vi.fn(async () => []),
  setCwPeerInfo: vi.fn(async () => {}),
}))
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))

// The header renders its children AND its `modeIndicator` — the mode picker and the `rig: …`
// mismatch chip live in the latter — so the stub must pass BOTH through.
// ⚠️ It passed only `children` until 2026-09-10, so the picker was absent from every test in
// this file and any assertion about it would have been vacuous.
vi.mock('./CockpitHeader', () => ({
  CockpitHeader: (p: { children?: React.ReactNode; modeIndicator?: React.ReactNode }) => (
    <header className="cockpit-header">
      {p.modeIndicator}
      {p.children}
    </header>
  ),
}))
// Not this suite's subject, and RotorStrip polls rotctld on mount.
vi.mock('./RotorStrip', () => ({ RotorStrip: () => null }))
vi.mock('./PhoneScope', () => ({ PhoneScope: () => <div data-testid="scope-stub" /> }))
vi.mock('./BandStrip', () => ({ BandStrip: () => <div data-testid="bandstrip-stub" /> }))
vi.mock('./VoiceKeyer', () => ({ VoiceKeyer: () => <div data-testid="vk-stub" /> }))

const mockedLogQso = vi.mocked(logQso)

// Braces are load-bearing: a concise arrow RETURNS the mock, and vitest calls a hook's
// return value as its teardown — an unhandled rejection the moment it rejects.
beforeEach(() => { mockedLogQso.mockClear() })
afterEach(cleanup)

/** A Phone snapshot on the 20 m AM calling frequency — the operator's own case. */
function makeSnap(over: Record<string, unknown> = {}): AppSnapshot {
  return {
    mycall: 'KD9TAW',
    hunt: null,
    radio: {
      dialMhz: 14.286,
      band: '20m',
      catOk: true,
      sideband: 'USB',
      sidebandOverride: null,
      rigMode: 'USB',
      transmitting: false,
      txEnabled: true,
      txAllowed: true,
      qsoRecording: false,
      rfPower: null,
      micGain: null,
      nrLevel: 0.3,
      agc: 'fast',
      nb: true,
      nr: true,
      notch: null,
      comp: null,
      vox: null,
      filterWidthHz: null,
      splitTxMhz: null,
      smeterDb: null,
      rxLevel: 0,
      phoneSegLo: null,
      phoneSegHi: null,
      ...over,
    },
  } as unknown as AppSnapshot
}

/** Render with the props App hands the cockpit (App.tsx `case 'phone'`). */
const renderPhone = (over: Record<string, unknown> = {}) =>
  render(
    <PhoneCockpit
      snap={makeSnap(over)}
      theme="dark"
      onSnap={() => {}}
      onConsumeWork={() => {}}
      pendingWork={null}
      fieldDay={undefined}
      wheelSensitivity={1}
      spots={[]}
      onWorkSpot={() => {}}
    />,
  )

/** Enter a call in the log strip and commit it. */
async function logContact(call: string) {
  fireEvent.change(document.querySelector('input.le-call') as HTMLInputElement, {
    target: { value: call },
  })
  fireEvent.click(screen.getByRole('button', { name: /^log$/i }))
  await waitFor(() => expect(mockedLogQso).toHaveBeenCalled())
}

/** The MODE field of the single record that reached the logbook. */
const loggedMode = () =>
  (mockedLogQso.mock.calls[0][0] as unknown as { mode: string }).mode

/** The line under the entry strip that tells the operator what will be written. */
const summaryText = () => document.body.textContent ?? ''

describe('the Phone cockpit logs the mode the rig is actually on', () => {
  it('logs an AM contact on 14.286 as AM, not SSB', async () => {
    // The reported case exactly: the rig is in AM, Nexus has read that back over CAT, and the
    // top bar is already flagging the disagreement. (At the time of the report the cockpit did
    // not offer AM on 20 m either, which is why he set it at the radio; that filter is gone.)
    renderPhone({ rigMode: 'AM' })
    await logContact('w1aw')
    expect(loggedMode(), 'an AM QSO was written to the logbook as SSB').toBe('AM')
  })

  it('offers AM on 20 m — the band a hardcoded filter used to rule out', async () => {
    // 14.286 IS the 20 m AM calling frequency. The picker used to hide AM below 10 MHz and at
    // 28 MHz and up, so on the one band this operator needed it, the button was missing. Nexus
    // does not get to have a band-plan opinion the rig itself does not have.
    renderPhone({ rigMode: 'USB' })
    const picker = await screen.findByRole('group', { name: /mode/i })
    const offered = within(picker)
      .getAllByRole('button')
      .map((b) => (b.textContent ?? '').trim())
    expect(offered, 'AM must be offered on 20 m').toContain('AM')
    // The control: the modes that were never filtered are still all there, so a green here
    // cannot come from the picker having lost its contents. AUTO's button names the sideband it
    // currently resolves to ("AUTO·USB"), hence the prefix match rather than equality.
    expect(offered).toEqual(expect.arrayContaining(['USB', 'LSB', 'FM', 'AM']))
    expect(offered.some((m) => m.startsWith('AUTO')), 'AUTO is still offered').toBe(true)
  })

  it('says AM on the line that states what will be written', async () => {
    // The screenshot's other half: "Logs to the shared logbook as SSB · 20m · 14.286 MHz".
    renderPhone({ rigMode: 'AM' })
    await waitFor(() =>
      expect(summaryText()).toMatch(/Logs to the shared logbook as AM/),
    )
  })

  // ⭐ THE SIDEBAND IS PART OF THE CONTACT (operator report, 2026-09-15). Everything above
  // fixed WHICH phone mode gets logged; it still could not say which SIDEBAND, because the
  // log mode was the ADIF *Mode* and USB and LSB are ADIF SUBMODEs of SSB. So every phone
  // QSO went to the logbook — and to QRZ, which shows a submode-less SSB record as "USB" —
  // as plain SSB, and nothing recorded whether it was upper or lower. HRD tracks U/L.
  //
  // The wire value here is the record's mode LABEL, not its ADIF MODE field: the exporter
  // (logbook.rs `adif_submode`) turns "LSB" into `<MODE:3>SSB<SUBMODE:3>LSB`, so MODE stays
  // SSB and the closed-enumeration trap that keeps USB/LSB out of `LOG_MODES` is untouched.
  it('logs a 20 m USB contact as USB, not bare SSB', async () => {
    renderPhone({ rigMode: 'USB' })
    await logContact('w1aw')
    expect(loggedMode(), 'the sideband was thrown away').toBe('USB')
  })

  it('logs an 80 m LSB contact as LSB', async () => {
    renderPhone({ dialMhz: 3.885, band: '80m', rigMode: 'LSB', sideband: 'LSB' })
    await logContact('w1aw')
    expect(loggedMode(), 'the sideband was thrown away').toBe('LSB')
  })

  it('the hand-entry override logs the mode its own box is showing', async () => {
    // ⚠️ THE OVERRIDE'S MODE BOX MUST NOT LIE. Its `<select>` is CONTROLLED and its options are
    // `LOG_MODES`, which has no USB/LSB — they are SUBMODEs, and that ruling stands. Seeded
    // with the cockpit's live "LSB" it holds a value no `<option>` matches, and the HTML
    // selectedness algorithm then selects the FIRST option for a single-select with nothing
    // selected: the box reads **SSB** while React state still says LSB, with no warning
    // anywhere. Open the override, touch nothing, log — and the strip writes a mode different
    // from the one it is showing.
    //
    // It is NOT blank, which is worth writing down: that was the guess, and a test asserting
    // "the box is not blank" passes identically before and after the fix. What discriminates
    // is whether the box and the RECORD agree.
    //
    // So the override opens on the parent MODE, which is what its vocabulary can express.
    renderPhone({ dialMhz: 3.885, band: '80m', rigMode: 'LSB', sideband: 'LSB' })
    fireEvent.click(screen.getByRole('button', { name: /another radio/i }))
    const shown = (document.querySelector('select.le-ov-mode') as HTMLSelectElement).value
    expect(shown).toBe('SSB')
    await logContact('w1aw')
    expect(loggedMode(), 'the override logged a mode its box was not showing').toBe(shown)

    // THE CONTROL, and it is the whole reason this is not merely "logs SSB": with the override
    // CLOSED the very same snapshot logs the sideband. Had the fix thrown the sideband away
    // everywhere, this half would fail.
    cleanup()
    mockedLogQso.mockClear()
    renderPhone({ dialMhz: 3.885, band: '80m', rigMode: 'LSB', sideband: 'LSB' })
    await logContact('w1aw')
    expect(loggedMode(), 'the closed-override path lost the sideband').toBe('LSB')
  })

  it('says the sideband on the line that states what will be written', async () => {
    renderPhone({ dialMhz: 3.885, band: '80m', rigMode: 'LSB', sideband: 'LSB' })
    await waitFor(() =>
      expect(summaryText()).toMatch(/Logs to the shared logbook as LSB/),
    )
  })

  it('logs an FM contact as FM', async () => {
    renderPhone({ dialMhz: 29.6, band: '10m', rigMode: 'FM', sidebandOverride: 'FM' })
    await logContact('w1aw')
    expect(loggedMode()).toBe('FM')
  })

  it("logs AM when the operator PICKS AM in the cockpit and the rig follows", async () => {
    // 7.290 is an AM window, so the picker offers AM there; `sidebandOverride` is the pick.
    renderPhone({ dialMhz: 7.29, band: '40m', sidebandOverride: 'AM', rigMode: 'AM' })
    await logContact('w1aw')
    expect(loggedMode()).toBe('AM')
  })

  it('follows the RIG, not the pick, when the rig did not take the AM command', async () => {
    // The operator picked AM but the CAT set failed / the rig is still on LSB. What went out
    // was a lower-sideband emission and that is what the log must say — mislabelling SSB as AM
    // is the same defect pointing the other way. The rig named the sideband, so the record
    // carries it.
    renderPhone({ dialMhz: 7.29, band: '40m', sidebandOverride: 'AM', rigMode: 'LSB' })
    await logContact('w1aw')
    expect(loggedMode()).toBe('LSB')
  })

  it('ignores the read-back when there is no CAT to have read it', async () => {
    // Without a control channel the mode read-back is not evidence of anything: the engine
    // clears `rig_mode` when the link drops, and a value that outlived that must not relabel
    // a QSO. Fall back to what Nexus commanded.
    renderPhone({ catOk: false, rigMode: 'AM' })
    await logContact('w1aw')
    expect(loggedMode()).toBe('SSB')
  })

  it('never invents a sideband from the band when the rig did not name one', async () => {
    // ⛔ The sideband comes from the RIG or not at all. The fallback the log mode must NOT use
    // is the cockpit's AUTO face, `dialMhz < 10 ? LSB : USB` — a BAND DEFAULT. Writing that
    // into a permanent record claims a sideband nobody measured, and the rig may have been on
    // the other one all along.
    //
    // Both bands are exercised so a green cannot come from the band default happening to agree
    // with the answer: on 80 m AUTO says LSB, on 20 m it says USB, and neither may appear.
    for (const over of [
      { catOk: false, dialMhz: 3.885, band: '80m', rigMode: '' }, // AUTO would say LSB
      { catOk: false, dialMhz: 14.2, band: '20m', rigMode: '' }, // AUTO would say USB
      { catOk: true, dialMhz: 3.885, band: '80m', rigMode: 'CW' }, // read-back names no phone mode
      { catOk: true, dialMhz: 14.2, band: '20m', rigMode: 'PKTUSB' },
    ]) {
      mockedLogQso.mockClear()
      renderPhone(over)
      await logContact('w1aw')
      expect(
        loggedMode(),
        `a sideband was invented on ${over.band} with rig "${over.rigMode}"`,
      ).toBe('SSB')
      cleanup()
    }
    // THE POSITIVE CONTROL. The same harness, the same two bands, with the rig actually
    // reporting a sideband — these MUST come back USB/LSB, or the assertions above are passing
    // because nothing can ever log a sideband rather than because the gate works.
    for (const [over, want] of [
      [{ catOk: true, dialMhz: 3.885, band: '80m', rigMode: 'LSB' }, 'LSB'],
      [{ catOk: true, dialMhz: 14.2, band: '20m', rigMode: 'USB' }, 'USB'],
    ] as const) {
      mockedLogQso.mockClear()
      renderPhone(over)
      await logContact('w1aw')
      expect(loggedMode(), 'the control did not trip').toBe(want)
      cleanup()
    }
  })

  it('ignores a read-back that does not name a phone mode', async () => {
    // A rig reporting CW / PKTUSB / anything outside the phone family while the Phone cockpit
    // is open is a real operational mismatch — the cockpit's own chip shouts about it — but it
    // is not a mode this strip can log FROM. Keep the commanded mode rather than inventing one;
    // the operator has the manual override for a genuine cross-mode contact.
    for (const rigMode of ['CW', 'PKTUSB', 'RTTY']) {
      mockedLogQso.mockClear()
      renderPhone({ rigMode })
      await logContact('w1aw')
      expect(loggedMode(), `rig: ${rigMode} relabelled the QSO`).toBe('SSB')
      cleanup()
    }
  })
})
