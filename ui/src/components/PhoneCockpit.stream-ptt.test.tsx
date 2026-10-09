// @vitest-environment jsdom
//
// A PHONE PTT PRESS THROUGH THE REMOTE STREAM ARMS THE STREAMED OPERATOR'S MICROPHONE, and never
// keys the rig on the shack's own (the operator's ruling "Arms your mic", 2026-09-27).
//
// `set_ptt` keys the rig on its own modulation source, which on SSB is the microphone at the
// shack. Pressed from Remote, that sends the voice of an empty room while the operator who pressed
// it talks into a browser nobody hears. So a press the stream dispatcher fires - Space over the
// picture, the PTT button clicked through it - arms the microphone over instead, which only their
// own audio keys; Lock refuses one; a release from either side lets go of both. At the shack
// nothing changes, and every case below is paired with that control.
//
// The presses here come from the REAL dispatcher, so the mark under test is the one the stream
// makes and not a stand-in for it.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react'
import { PhoneCockpit } from './PhoneCockpit'
import { StreamInputDispatcher } from '../remote-native/stream-input'
import type { AppSnapshot } from '../types'

// THE BUDGET (2026-10-09). The slowest case here, "Space over the picture arms the over and never calls…", takes
// 0.18 s and 1.59 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than
// one core, past vitest's 5 s default. 15 s is the house budget; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

const { setPtt, setTxEnabled, armStreamMic, releaseStreamMic, pushToast } = vi.hoisted(() => ({
  setPtt: vi.fn(async (_on: boolean) => ({})),
  setTxEnabled: vi.fn(async () => ({})),
  armStreamMic: vi.fn(async () => true),
  releaseStreamMic: vi.fn(async () => {}),
  pushToast: vi.fn(),
}))

vi.mock('../api', () => ({
  getSettings: vi.fn(async () => ({})),
  setPtt,
  setTxEnabled,
  armStreamMic,
  releaseStreamMic,
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
}))
vi.mock('../toast', () => ({
  pushToast,
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
vi.mock('./CockpitHeader', () => ({ CockpitHeader: () => <header className="cockpit-header" /> }))
vi.mock('./PhoneScope', () => ({ PhoneScope: () => <div data-testid="scope-stub" /> }))
vi.mock('./BandStrip', () => ({ BandStrip: () => <div data-testid="bandstrip-stub" /> }))
vi.mock('./VoiceKeyer', () => ({ VoiceKeyer: () => <div data-testid="vk-stub" /> }))
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))
vi.mock('./SpotDialog', () => ({ SpotDialog: () => null }))

// jsdom never lays out: `elementFromPoint` does not exist. Each test says what is under the pointer.
let under: Element | null = null
let stream: StreamInputDispatcher | null = null
beforeEach(() => {
  under = null
  Object.defineProperty(document, 'elementFromPoint', { configurable: true, value: () => under })
  stream = new StreamInputDispatcher(window)
})
afterEach(async () => {
  stream?.dispose()
  stream = null
  cleanup()
  // What the unmount let go of is sent on the cockpit's chain, a few promises later: let it land
  // here, so it is never counted in the next test.
  await new Promise((resolve) => setTimeout(resolve, 0))
  delete (document as { elementFromPoint?: unknown }).elementFromPoint
  for (const f of [setPtt, setTxEnabled, armStreamMic, releaseStreamMic, pushToast]) f.mockClear()
  armStreamMic.mockImplementation(async () => true)
})

function makeSnap(radio: Record<string, unknown> = {}): AppSnapshot {
  return {
    mycall: 'KD9TAW',
    radio: {
      dialMhz: 14.2, band: '20m', catOk: true, sideband: 'USB', sidebandOverride: null, rigMode: 'USB',
      transmitting: false, txEnabled: true, txAllowed: true, qsoRecording: false, rfPower: null, micGain: null,
      nrLevel: 0.3, agc: 'fast', nb: true, nr: true, notch: null, comp: null, vox: null, filterWidthHz: null,
      splitTxMhz: null, smeterDb: null, rxLevel: 0, phoneSegLo: null, phoneSegHi: null, ...radio,
    },
  } as unknown as AppSnapshot
}
const renderPhone = () => render(<PhoneCockpit snap={makeSnap()} theme="dark" onWorkSpot={() => {}} spots={[]} />)
const ptt = () => document.querySelector('.ph-ptt') as HTMLButtonElement
const lockBox = () => document.querySelector('.ph-lock input') as HTMLInputElement
/** Let the cockpit's chained microphone calls settle. */
const settle = () => act(async () => { for (let i = 0; i < 5; i++) await Promise.resolve() })

// What the station hands the window, as its contract spells it (webview.json).
const SPACE = (action: 'down' | 'up') => ({ type: 'key', action, key: ' ', code: 'Space', modifiers: 0, repeat: false })
const pointer = (action: 'down' | 'up' | 'move') =>
  ({ type: 'pointer', action, x: 0.5, y: 0.5, button: action === 'move' ? -1 : 0, buttons: action === 'down' ? 1 : 0,
    modifiers: 0, pointerType: 'mouse', clicks: action === 'move' ? 0 : 1 })
const streamed = async (message: object) => { act(() => { stream!.handle(message) }); await settle() }
const keyedAtTheShack = () => setPtt.mock.calls.some(([on]) => on === true)

describe('R1: a press through the stream arms the browser microphone, never the shack\'s', () => {
  it('Space over the picture arms the over and never calls set_ptt; letting go releases it', async () => {
    renderPhone()
    await streamed(SPACE('down'))
    expect(armStreamMic).toHaveBeenCalledTimes(1)
    expect(keyedAtTheShack(), 'a streamed Space keyed the shack\'s microphone').toBe(false)
    await streamed(SPACE('up'))
    expect(releaseStreamMic).toHaveBeenCalledTimes(1)
    expect(keyedAtTheShack()).toBe(false)
  })

  it('CONTROL: Space at the shack keys set_ptt exactly as before, and arms nothing', async () => {
    renderPhone()
    fireEvent.keyDown(window, { key: ' ', code: 'Space' })
    await settle()
    expect(setPtt).toHaveBeenLastCalledWith(true)
    fireEvent.keyUp(window, { key: ' ', code: 'Space' })
    await settle()
    expect(setPtt).toHaveBeenLastCalledWith(false)
    expect(armStreamMic).not.toHaveBeenCalled()
    expect(releaseStreamMic, 'nothing was armed, so nothing is released').not.toHaveBeenCalled()
  })

  it('the PTT button pressed through the picture arms the over and never calls set_ptt; letting go releases it', async () => {
    renderPhone()
    under = ptt()
    await streamed(pointer('down'))
    expect(armStreamMic).toHaveBeenCalledTimes(1)
    // Held, and armed: the voice keys it (the Armed label, below).
    expect(ptt().classList.contains('keyed'), 'the press shows as held').toBe(true)
    expect(ptt().textContent).toBe('Armed — talk to transmit')
    await streamed(pointer('up'))
    expect(releaseStreamMic).toHaveBeenCalledTimes(1)
    expect(keyedAtTheShack()).toBe(false)
  })

  it('CONTROL: the PTT button pressed at the shack keys set_ptt exactly as before', async () => {
    renderPhone()
    fireEvent.pointerDown(ptt())
    await settle()
    expect(setPtt).toHaveBeenLastCalledWith(true)
    fireEvent.pointerUp(ptt())
    await settle()
    expect(setPtt).toHaveBeenLastCalledWith(false)
    expect(armStreamMic).not.toHaveBeenCalled()
  })

  it('Lock refuses a streamed press and says why; the Lock box itself still works from the stream', async () => {
    renderPhone()
    // Ticked through the stream: the box is operable, so a Lock left on at the shack can be undone.
    under = lockBox()
    await streamed(pointer('down'))
    await streamed(pointer('up'))
    expect(lockBox().checked).toBe(true)
    under = ptt()
    await streamed(pointer('down'))
    await streamed(pointer('up'))
    expect(armStreamMic, 'Lock armed a hands-free microphone over from Remote').not.toHaveBeenCalled()
    expect(keyedAtTheShack(), 'Lock keyed the shack\'s microphone from Remote').toBe(false)
    expect(pushToast).toHaveBeenCalledWith(
      'Lock (hands-free PTT) does not work over Remote. Untick Lock, then hold PTT or the Space bar to talk.', 'info', 4000)
    expect(ptt().textContent).not.toMatch(/ON AIR/i)
  })

  it('CONTROL: with Lock at the shack, a click latches set_ptt and the next unlatches it, exactly as before', async () => {
    renderPhone()
    fireEvent.click(lockBox())
    expect(lockBox().checked).toBe(true)
    fireEvent.pointerDown(ptt())
    await settle()
    expect(setPtt).toHaveBeenLastCalledWith(true)
    fireEvent.pointerUp(ptt())
    await settle()
    expect(setPtt, 'Lock: letting go of the button does not unkey').toHaveBeenLastCalledWith(true)
    fireEvent.pointerDown(ptt())
    await settle()
    expect(setPtt).toHaveBeenLastCalledWith(false)
    expect(armStreamMic).not.toHaveBeenCalled()
    expect(pushToast).not.toHaveBeenCalled()
  })

  it('a release at the shack lets go of an over armed through the stream', async () => {
    renderPhone()
    await streamed(SPACE('down'))
    expect(armStreamMic).toHaveBeenCalledTimes(1)
    // The shack's own Space comes up (an unmarked key-up), as when the operator there lets go.
    fireEvent.keyUp(window, { key: ' ', code: 'Space' })
    await settle()
    expect(releaseStreamMic).toHaveBeenCalledTimes(1)
    expect(setPtt).toHaveBeenLastCalledWith(false)
  })

  it('a release through the stream lets go of a key made at the shack', async () => {
    renderPhone()
    fireEvent.pointerDown(ptt())
    await settle()
    expect(setPtt).toHaveBeenLastCalledWith(true)
    // The stream's pointer passes over the button and off it: its leave is a release.
    under = ptt()
    await streamed(pointer('move'))
    under = document.body
    await streamed(pointer('move'))
    expect(setPtt).toHaveBeenLastCalledWith(false)
    expect(armStreamMic).not.toHaveBeenCalled()
  })

  it('a release never overtakes the arm it lets go of: it is sent only once the arm has answered', async () => {
    let answer: (armed: boolean) => void = () => {}
    armStreamMic.mockImplementation(() => new Promise<boolean>((resolve) => { answer = resolve }))
    renderPhone()
    await streamed(SPACE('down'))
    await streamed(SPACE('up'))
    expect(armStreamMic).toHaveBeenCalledTimes(1)
    expect(releaseStreamMic, 'the release went ahead of its arm').not.toHaveBeenCalled()
    await act(async () => { answer(true) })
    await settle()
    expect(releaseStreamMic).toHaveBeenCalledTimes(1)
  })

  it('an arm the station refuses holds nothing: the button lets go, and no release is owed', async () => {
    armStreamMic.mockImplementation(async () => false)
    renderPhone()
    await streamed(SPACE('down'))
    expect(armStreamMic).toHaveBeenCalledTimes(1)
    expect(ptt().textContent).not.toMatch(/ON AIR/i)
    await streamed(SPACE('up'))
    expect(releaseStreamMic).not.toHaveBeenCalled()
    expect(keyedAtTheShack()).toBe(false)
  })

  it('leaving Phone lets go of an over armed through the stream', async () => {
    const view = renderPhone()
    await streamed(SPACE('down'))
    view.unmount()
    await settle()
    expect(releaseStreamMic).toHaveBeenCalledTimes(1)
  })
})

// ── The Armed label (the operator's pick "Show Armed until keyed"), DISPLAY ONLY ───────────────
//
// A press through the picture ARMS the over and the voice keys it, so until the voice arrives the
// button reads "Armed — talk to transmit", then "ON AIR — release to stop" as before. What does not
// change: its accessible name in each state (the stop-line sweep finds it by name), its colours,
// and a release still lets go of the over. The station says which the over is (`radio.streamMic`).

const ON_AIR = 'ON AIR — release to stop'
const ARMED = 'Armed — talk to transmit'
// The stop-line sweep's own matcher for this button (stop-line.test.tsx, Phone's PTT).
const SWEEP = /push to talk|on air — release to stop|tx locked|tx off — click to enable/i
const phone = (radio: Record<string, unknown> = {}) =>
  <PhoneCockpit snap={makeSnap(radio)} theme="dark" onWorkSpot={() => {}} spots={[]} />

describe('the Armed label: a streamed press reads Armed until the voice keys the rig', () => {
  it('the PTT button pressed through the picture reads Armed, under the name it had, until the voice keys; then ON AIR', async () => {
    const view = render(phone())
    under = ptt()
    await streamed(pointer('down'))
    expect(armStreamMic).toHaveBeenCalledTimes(1)
    // Before the station's next snapshot reports the over, the press already reads Armed: no
    // flash of ON AIR for the up to 300 ms until it does.
    expect(ptt().textContent, 'the press, before the station reports the over').toBe(ARMED)
    // The station armed the over; no voice yet.
    view.rerender(phone({ streamMic: 'armed' }))
    expect(ptt().textContent, 'armed, no voice yet').toBe(ARMED)
    expect(screen.getByRole('button', { name: ON_AIR }), 'the accessible name changed').toBe(ptt())
    expect(screen.getByRole('button', { name: SWEEP }), 'the stop-line sweep no longer finds it').toBe(ptt())
    expect(ptt().classList.contains('keyed'), 'its colours are no longer the held ones').toBe(true)
    // The voice keys the rig.
    view.rerender(phone({ streamMic: 'keyed' }))
    expect(ptt().textContent, 'keyed by the voice').toBe(ON_AIR)
    expect(screen.getByRole('button', { name: ON_AIR })).toBe(ptt())
  })

  it('it stays a stop control while it reads Armed: letting go, through the picture or at the shack, releases the over', async () => {
    const view = render(phone())
    under = ptt()
    await streamed(pointer('down'))
    view.rerender(phone({ streamMic: 'armed' }))
    expect(ptt().textContent).toBe(ARMED)
    await streamed(pointer('up'))
    expect(releaseStreamMic, 'the release through the picture').toHaveBeenCalledTimes(1)
    expect(ptt().textContent).toBe('PUSH TO TALK')
    // Armed again with Space over the picture, and let go at the shack.
    await streamed(SPACE('down'))
    expect(ptt().textContent).toBe(ARMED)
    fireEvent.keyUp(window, { key: ' ', code: 'Space' })
    await settle()
    expect(releaseStreamMic, 'the release at the shack').toHaveBeenCalledTimes(2)
    expect(keyedAtTheShack()).toBe(false)
  })

  it('CONTROL: a press at the shack reads as before, even while the station reports an over armed from the page', async () => {
    // The page's own Hold PTT armed the over; nobody pressed this button.
    render(phone({ streamMic: 'armed' }))
    expect(ptt().textContent, 'not pressed here').toBe('PUSH TO TALK')
    fireEvent.pointerDown(ptt())
    await settle()
    expect(setPtt).toHaveBeenLastCalledWith(true)
    expect(ptt().textContent, 'a shack press').toBe(ON_AIR)
    fireEvent.pointerUp(ptt())
    await settle()
    expect(ptt().textContent).toBe('PUSH TO TALK')
    expect(armStreamMic).not.toHaveBeenCalled()
  })

  it('CONTROL: an over the station no longer reports armed (it ended while still held) reads as before', async () => {
    const view = render(phone())
    await streamed(SPACE('down'))
    view.rerender(phone({ streamMic: 'armed' }))
    expect(ptt().textContent).toBe(ARMED)
    // The audio design's gaps end the over at the station while the press is still held here.
    view.rerender(phone({ streamMic: null }))
    expect(ptt().textContent, 'Armed shown over no over at all').toBe(ON_AIR)
  })
})
