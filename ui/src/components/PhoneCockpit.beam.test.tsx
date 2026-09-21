// @vitest-environment jsdom
//
// "IS THERE A WAY TO TURN THE ROTATOR TO THAT STATION FROM THIS TAB?" (tester, 2026-09-20,
// naming the Phone tab; a second tester asked for the same button across the cockpits.)
//
// The answer was yes everywhere except here. `RotorStrip` has rendered a one-click "→ CALL"
// slew since it was written, and CwCockpit and OperateCockpit both pass `targetCall`/`onPointAt`
// to light it. PhoneCockpit passed neither, and the strip's own guard is
// `{targetCall && onPointAt && …}` — so the control rendered as NOTHING, which from the
// operator's chair is indistinguishable from a feature that was never built. Both testers
// reported it as missing.
//
// Phone was the only cockpit with nothing to pass: CW has `guide.workedCall` and Operate has the
// app-wide `selectedCall`, and Phone's call lives inside the log strip. `LogEntry`'s
// `onCallChange` is the link that gets it out.
//
// ⚠️ THE REPORT DIRECTION ONLY, AND THAT IS THE DELICATE PART. `onCallChange` is one half of a
// two-way link; RttyCockpit passes `cwLive` as well because a decoder fills its box. Nothing
// fills Phone's box but the operator, and passing `cwLive` here would arm LogEntry's
// clear-on-empty branch against a host with no box to be empty. The second test below is that
// failure mode: it fails if a typed callsign is ever wiped out from under the operator.
//
// The strip is stubbed to RENDER ITS PROPS rather than to render nothing, because the defect
// was precisely that the props were absent — a stub that swallowed them could not see it. The
// log strip is REAL, so the chain under test is the whole one: keystroke → onCallChange →
// workedCall → targetCall → onPointAt → pointRotatorAtCall.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, cleanup, waitFor } from '@testing-library/react'
import { PhoneCockpit } from './PhoneCockpit'
import { pointRotatorAtCall } from '../api'
import type { AppSnapshot } from '../types'

vi.mock('../api', () => ({
  getSettings: vi.fn(async () => ({})),
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
  pointRotatorAtCall: vi.fn(async () => 47),
  // LogEntry's, rendered for real.
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
vi.mock('./CockpitHeader', () => ({
  CockpitHeader: (p: { children?: React.ReactNode; modeIndicator?: React.ReactNode }) => (
    <header className="cockpit-header">
      {p.modeIndicator}
      {p.children}
    </header>
  ),
}))
// Renders its props — see the header note. `data-target` is empty, not absent, when no call is
// in play, so "no target" and "the strip never rendered" stay distinguishable.
vi.mock('./RotorStrip', () => ({
  RotorStrip: (p: { targetCall?: string | null; onPointAt?: (c: string) => void }) => (
    <div data-testid="rotor" data-target={p.targetCall ?? ''}>
      {p.targetCall && p.onPointAt ? (
        <button type="button" onClick={() => p.onPointAt?.(p.targetCall as string)}>
          beam
        </button>
      ) : null}
    </div>
  ),
}))
vi.mock('./PhoneScope', () => ({ PhoneScope: () => <div data-testid="scope-stub" /> }))
vi.mock('./BandStrip', () => ({ BandStrip: () => <div data-testid="bandstrip-stub" /> }))
vi.mock('./VoiceKeyer', () => ({ VoiceKeyer: () => <div data-testid="vk-stub" /> }))

const mockedPoint = vi.mocked(pointRotatorAtCall)
beforeEach(() => { mockedPoint.mockClear() })
afterEach(cleanup)

function makeSnap(over: Record<string, unknown> = {}): AppSnapshot {
  return {
    mycall: 'KD9TAW',
    hunt: null,
    radio: {
      dialMhz: 14.25, band: '20m', catOk: true, sideband: 'USB', sidebandOverride: null,
      rigMode: 'USB', transmitting: false, txEnabled: true, txAllowed: true,
      qsoRecording: false, rfPower: null, micGain: null, nrLevel: 0.3, agc: 'fast',
      nb: true, nr: true, notch: null, comp: null, vox: null, filterWidthHz: null,
      splitTxMhz: null, smeterDb: null, rxLevel: 0, phoneSegLo: null, phoneSegHi: null,
      ...over,
    },
  } as unknown as AppSnapshot
}

const renderPhone = () =>
  render(
    <PhoneCockpit
      snap={makeSnap()} theme="dark" onSnap={() => {}} onConsumeWork={() => {}}
      pendingWork={null} fieldDay={undefined} wheelSensitivity={1} spots={[]}
      onWorkSpot={() => {}}
    />,
  )

const callBox = () => document.querySelector('input.le-call') as HTMLInputElement
const rotor = () => screen.getByTestId('rotor')

describe('the Phone cockpit can beam at the station it is working', () => {
  it('hands the strip the call the operator typed, so the slew appears', async () => {
    renderPhone()
    // CONTROL: nothing typed, so there is nothing to point at and no button. If this ever
    // reports a target, every assertion below is passing on a value that was always there.
    expect(rotor().getAttribute('data-target')).toBe('')
    expect(screen.queryByRole('button', { name: /beam/i })).toBeNull()

    fireEvent.change(callBox(), { target: { value: 'ja1abc' } })

    await waitFor(() => expect(rotor().getAttribute('data-target')).toBe('JA1ABC'))
    fireEvent.click(screen.getByRole('button', { name: /beam/i }))
    await waitFor(() => expect(mockedPoint).toHaveBeenCalledWith('JA1ABC'))
  })

  it('never wipes the callsign the operator typed', async () => {
    // The failure mode the one-way link exists to avoid: LogEntry clears `logCall` when a
    // LINKED host's own box goes empty, and Phone has no box. If the fill half is ever wired
    // here, this is what the operator sees — a callsign vanishing mid-QSO.
    renderPhone()
    fireEvent.change(callBox(), { target: { value: 'ja1abc' } })
    await waitFor(() => expect(rotor().getAttribute('data-target')).toBe('JA1ABC'))
    // Let every queued effect in the link settle, then look again.
    await waitFor(() => expect(callBox().value.toUpperCase()).toBe('JA1ABC'))
    expect(rotor().getAttribute('data-target')).toBe('JA1ABC')
  })

  it('drops the target when the operator clears the box', async () => {
    renderPhone()
    fireEvent.change(callBox(), { target: { value: 'ja1abc' } })
    await waitFor(() => expect(rotor().getAttribute('data-target')).toBe('JA1ABC'))
    fireEvent.change(callBox(), { target: { value: '' } })
    // An empty target must remove the button rather than leave it pointing at a stale call —
    // the whole reason `spotCall` was the wrong source for this.
    await waitFor(() => expect(rotor().getAttribute('data-target')).toBe(''))
    expect(screen.queryByRole('button', { name: /beam/i })).toBeNull()
  })
})
