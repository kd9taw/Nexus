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
import { render, screen, fireEvent, cleanup, waitFor } from '@testing-library/react'
import { PhoneCockpit } from './PhoneCockpit'
import { logQso } from '../api'
import type { AppSnapshot } from '../types'

vi.mock('../api', () => ({
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

// The header renders its children (the mode picker and the `rig: …` mismatch chip live there),
// so the stub must pass them through or the picker assertions would pass on nothing.
vi.mock('./CockpitHeader', () => ({
  CockpitHeader: (p: { children?: React.ReactNode }) => (
    <header className="cockpit-header">{p.children}</header>
  ),
}))
// Not this suite's subject, and RotorStrip polls rotctld on mount.
vi.mock('./RotorStrip', () => ({ RotorStrip: () => null }))
vi.mock('./PhoneScope', () => ({ PhoneScope: () => <div data-testid="scope-stub" /> }))
vi.mock('./BandStrip', () => ({ BandStrip: () => <div data-testid="bandstrip-stub" /> }))
vi.mock('./VoiceKeyer', () => ({ VoiceKeyer: () => <div data-testid="vk-stub" /> }))

const mockedLogQso = vi.mocked(logQso)

beforeEach(() => mockedLogQso.mockClear())
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
    // The reported case exactly: the rig is in AM (the operator set it at the radio — the
    // cockpit does not even offer AM on 20 m), Nexus has read that back over CAT, and the top
    // bar is already flagging the disagreement.
    renderPhone({ rigMode: 'AM' })
    await logContact('w1aw')
    expect(loggedMode(), 'an AM QSO was written to the logbook as SSB').toBe('AM')
  })

  it('says AM on the line that states what will be written', async () => {
    // The screenshot's other half: "Logs to the shared logbook as SSB · 20m · 14.286 MHz".
    renderPhone({ rigMode: 'AM' })
    await waitFor(() =>
      expect(summaryText()).toMatch(/Logs to the shared logbook as AM/),
    )
  })

  it('still logs an ordinary SSB contact as SSB — the control', async () => {
    // The fix must not have swapped one default for another.
    renderPhone({ rigMode: 'USB' })
    await logContact('w1aw')
    expect(loggedMode()).toBe('SSB')
  })

  it('logs LSB as SSB too — USB/LSB are ADIF SUBMODEs, never Modes', async () => {
    renderPhone({ dialMhz: 3.885, band: '80m', rigMode: 'LSB', sideband: 'LSB' })
    await logContact('w1aw')
    expect(loggedMode()).toBe('SSB')
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
    // was an SSB emission and that is what the log must say — mislabelling SSB as AM is the
    // same defect pointing the other way.
    renderPhone({ dialMhz: 7.29, band: '40m', sidebandOverride: 'AM', rigMode: 'LSB' })
    await logContact('w1aw')
    expect(loggedMode()).toBe('SSB')
  })

  it('ignores the read-back when there is no CAT to have read it', async () => {
    // Without a control channel the mode read-back is not evidence of anything: the engine
    // clears `rig_mode` when the link drops, and a value that outlived that must not relabel
    // a QSO. Fall back to what Nexus commanded.
    renderPhone({ catOk: false, rigMode: 'AM' })
    await logContact('w1aw')
    expect(loggedMode()).toBe('SSB')
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
