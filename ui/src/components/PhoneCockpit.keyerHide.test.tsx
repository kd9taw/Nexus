// @vitest-environment jsdom
//
// HIDING THE VOICE KEYER IS A STOP, AND THE OPERATOR IS TOLD SO FIRST.
//
// The voice keyer TRANSMITS, so under the old blunt rule ("TX/safety controls have no id
// in any pane vocabulary") it could not be a panel at all. The rule was narrowed on
// 2026-08-03 to the thing it was actually protecting: a pane that can only START a
// transmission may be hidden; anything that can STOP one may never be. The keyer is
// admissible because hiding it does not strand you keyed — its unmount cleanup calls
// stopVoice, so the hide IS the abort.
//
// That argument is only worth anything if the abort really happens, so this suite renders
// the REAL VoiceKeyer (every other Phone suite stubs it) inside the REAL cockpit and hides
// it through the panel record the ⊞ menu writes. A test against a stub would prove the
// pane disappears and nothing about the transmitter.
//
// And because an abort the operator did not expect is indistinguishable from a dropout,
// the ⊞ entry carries the consequence BEFORE the tick — informed consent at the point of
// decision beats an apology afterwards, and `note` is already the menu's machinery for it
// (aria-describedby, so it reaches a screen reader too).
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, act } from '@testing-library/react'
import { PhoneCockpit, VOICE_KEYER_STOPS_ON_HIDE } from './PhoneCockpit'
import type { AppSnapshot } from '../types'
import type { PanelLayoutApi, PhonePanelId } from '../features/panelState'

// vi.hoisted, not a bare const: the vi.mock factory below is hoisted above every
// top-level binding, so a plain const would be in its temporal dead zone when it runs.
const { stopVoice } = vi.hoisted(() => ({ stopVoice: vi.fn(async () => ({})) }))

vi.mock('../api', () => ({
  setPtt: vi.fn(async () => {}),
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
  // The voice keyer's own wire. `stopVoice` is the one under test.
  getVoiceMessages: vi.fn(async () => [{ slot: 1, label: 'CQ', file: '/tmp/cq.wav' }]),
  playVoiceMessage: vi.fn(async () => ({})),
  stopVoice,
  startVoiceRecording: vi.fn(async () => ({})),
  stopVoiceRecording: vi.fn(async () => []),
  cancelVoiceRecording: vi.fn(async () => ({})),
  clearVoiceMessage: vi.fn(async () => []),
  importVoiceMessage: vi.fn(async () => []),
}))

// The header is stubbed down to the one thing under test — it hosts the ⊞ menu, which the
// cockpit hands it as `actions`.
vi.mock('./CockpitHeader', () => ({
  CockpitHeader: ({ actions }: { actions?: unknown }) => (
    <header className="cockpit-header">{actions as never}</header>
  ),
}))
vi.mock('./PhoneScope', () => ({ PhoneScope: () => <div data-testid="scope-stub" /> }))
vi.mock('./BandStrip', () => ({ BandStrip: () => <div data-testid="bandstrip-stub" /> }))
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))
vi.mock('./SpotDialog', () => ({ SpotDialog: () => null }))
// VoiceKeyer is deliberately NOT mocked — its unmount cleanup is the behaviour under test.

afterEach(() => {
  stopVoice.mockClear()
  cleanup()
})

function makeSnap(): AppSnapshot {
  return {
    mycall: 'KD9TAW',
    radio: {
      dialMhz: 14.2,
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
    },
  } as unknown as AppSnapshot
}

function fakePanels(removed: PhonePanelId[] = []): PanelLayoutApi<PhonePanelId> {
  return {
    layout: { v: 1, state: {}, share: {} },
    stateOf: (id) => (removed.includes(id) ? 'removed' : 'docked'),
    setPanelState: () => {},
    shareOf: () => 1,
    setShare: () => {},
    setShares: () => {},
    undo: () => {},
    canUndo: false,
    reset: () => {},
  }
}

const view = (removed: PhonePanelId[] = []) => (
  <PhoneCockpit
    snap={makeSnap()}
    theme="dark"
    onWorkSpot={() => {}}
    spots={[]}
    panels={fakePanels(removed)}
  />
)

describe('hiding the Phone voice keyer', () => {
  it('aborts the message in flight — the hide calls stopVoice, it does not leave you keyed', async () => {
    const r = render(view())
    await act(async () => {})
    // The real keyer is mounted (its slot grid rendered from the stubbed engine list).
    expect(document.querySelector('[data-pane="voiceKeyer"] .vk')).not.toBeNull()
    expect(stopVoice).not.toHaveBeenCalled()

    // The operator unticks Voice Keyer. This is the whole safety argument: the pane goes,
    // and the transmission goes with it.
    r.rerender(view(['voiceKeyer']))
    await act(async () => {})
    expect(document.querySelector('[data-pane="voiceKeyer"]')).toBeNull()
    expect(stopVoice, 'hiding the keyer did not abort playback — it stranded it').toHaveBeenCalled()

    // …and the way to stop a transmission the keyer did not start is exactly where it was.
    expect(document.querySelector('.cockpit-txdock .ph-ptt')).not.toBeNull()
  })

  it('hiding any OTHER panel leaves the keyer transmitting undisturbed', async () => {
    // The converse, and the reason the fiber-stability tests exist: a menu interaction
    // that merely reflows the region must not fire the keyer's cleanup. Only its own
    // entry may stop it.
    const r = render(view())
    await act(async () => {})
    r.rerender(view(['dsp']))
    await act(async () => {})
    expect(document.querySelector('[data-pane="voiceKeyer"] .vk')).not.toBeNull()
    expect(stopVoice, 'an unrelated ⊞ toggle aborted the voice keyer').not.toHaveBeenCalled()
  })

  it('the ⊞ entry says what unticking it will do, before the operator unticks it', async () => {
    render(view())
    await act(async () => {})
    fireEvent.click(screen.getByRole('button', { name: /Panels/ }))
    const box = screen.getByRole('checkbox', { name: /Voice Keyer/ }) as HTMLInputElement
    expect(box.checked).toBe(true)
    // Checkable (it is not an unavailable entry) and described by the consequence.
    expect(box.getAttribute('aria-disabled')).toBeNull()
    const whyId = box.getAttribute('aria-describedby')
    expect(whyId, 'the Voice Keyer entry carries no note').not.toBeNull()
    expect(document.getElementById(whyId!)?.textContent).toBe(VOICE_KEYER_STOPS_ON_HIDE)
  })
})
