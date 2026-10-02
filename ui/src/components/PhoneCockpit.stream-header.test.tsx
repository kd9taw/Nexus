// @vitest-environment jsdom
//
// THE PHONE SCREEN'S ON AIR SIGN WAITS FOR THE VOICE, LIKE THE PTT BUTTON (the operator's pick
// "Header ON AIR waits too", 2026-09-28, completed by the lead's Q1 ruling of 2026-09-29). DISPLAY ONLY.
// The sign left the header for the TX strip under the scope with the rest of the transmit cluster
// (2026-10-01, `CockpitTxStrip`), and the rule went with it: the sign under test is the strip's.
//
// Every PTT a streamed operator holds (the page's own Hold PTT, Space over the picture, this
// cockpit's PTT clicked through it) ARMS their microphone over, and their voice keys the rig
// (PhoneCockpit.stream-ptt.test.tsx). The arbiter counts an armed over as the transmitter's owner,
// so the ON AIR sign lit from the arm. It now waits: while the station reports the over
// armed (`radio.streamMic`, which says `armed` only while that over owns the transmitter), the sign
// reads as it does before a key, and it lights once the voice keys the rig. A press at the shack
// reads exactly as before, and anything else on the air lights it. What does not change: the
// arbiter's sentence on the strip's latch, the rest of the strip and the header, and the
// amplifier strip's lock, which still reads the arbiter (CockpitHeader.ampgate.test.tsx).
//
// The header and the strip are REAL here (the stream-ptt file stubs the header), and the presses
// come from the REAL stream dispatcher, so the mark under test is the one the stream makes.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react'
import * as api from '../api'
import { PhoneCockpit } from './PhoneCockpit'
import { StreamInputDispatcher } from '../remote-native/stream-input'
import type { AppSnapshot } from '../types'
import { t } from '../i18n'

const { setPtt, armStreamMic, releaseStreamMic } = vi.hoisted(() => ({
  setPtt: vi.fn(async (_on: boolean) => ({})),
  armStreamMic: vi.fn(async () => true),
  releaseStreamMic: vi.fn(async () => {}),
}))

vi.mock('../api', async (importOriginal) => {
  // Derived from the real module (stop-line.test.tsx's rule): the real header mounts more of it
  // than this file names, and a hand-kept list throws on mount the day one is added.
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) {
    auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  }
  return {
    ...auto,
    setPtt,
    armStreamMic,
    releaseStreamMic,
    getSettings: vi.fn(async () => ({})),
    readRotator: vi.fn(async () => null),
    getDeclination: vi.fn(async () => 0),
    getSatTrackStatus: vi.fn(async () => null),
    getSatTransponder: vi.fn(async () => null),
    getLicensedBandPlan: vi.fn(async () => []),
  }
})
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
// Canvas and feed children only. CockpitHeader and CockpitTxStrip are DELIBERATELY REAL.
vi.mock('./PhoneScope', () => ({ PhoneScope: () => <div data-testid="scope-stub" /> }))
vi.mock('./BandStrip', () => ({ BandStrip: () => <div data-testid="bandstrip-stub" /> }))
vi.mock('./VoiceKeyer', () => ({ VoiceKeyer: () => <div data-testid="vk-stub" /> }))
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))
vi.mock('./SpotDialog', () => ({ SpotDialog: () => null }))

// jsdom never lays out: `elementFromPoint` does not exist. Each test says what is under the pointer.
let under: Element | null = null
let stream: StreamInputDispatcher | null = null
beforeEach(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver
  // The dispatcher lets go of a held press 200 ms after the page last re-asserted it. Nothing here
  // is about that dead-man, and a slow render under load must not let the press go mid-test.
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] })
  under = null
  Object.defineProperty(document, 'elementFromPoint', { configurable: true, value: () => under })
  stream = new StreamInputDispatcher(window)
})
afterEach(async () => {
  stream?.dispose()
  stream = null
  cleanup()
  vi.useRealTimers()
  await new Promise((resolve) => setTimeout(resolve, 0))
  delete (document as { elementFromPoint?: unknown }).elementFromPoint
  vi.clearAllMocks()
  armStreamMic.mockImplementation(async () => true)
})

function makeSnap(radio: Record<string, unknown> = {}): AppSnapshot {
  return {
    mycall: 'KD9TAW',
    radio: {
      dialMhz: 14.2, band: '20m', catOk: true, sideband: 'USB', sidebandOverride: null, rigMode: 'USB',
      transmitting: false, tuning: false, txEnabled: true, txAllowed: true, txBusyReason: null, rigKeyed: false,
      streamMic: null, qsoRecording: false, rfPower: null, micGain: null, nrLevel: 0.3, agc: 'fast', nb: true,
      nr: true, notch: null, comp: null, vox: null, filterWidthHz: null, splitTxMhz: null, smeterDb: null,
      rxLevel: 0, phoneSegLo: null, phoneSegHi: null, ...radio,
    },
  } as unknown as AppSnapshot
}
const phone = (radio: Record<string, unknown> = {}) =>
  <PhoneCockpit snap={makeSnap(radio)} theme="dark" onWorkSpot={() => {}} spots={[]} />
const ptt = () => document.querySelector('.ph-ptt') as HTMLButtonElement
/** The ON AIR sign: the TX strip's state cap. */
const sign = () => document.querySelector('.cockpit-txstrip .cq-statecap') as HTMLElement
const lit = () => sign().classList.contains('tx')
const RX = t('operate.strip.state.receiving')
const TX = t('operate.strip.state.transmitting')
/** The strip's TX latch, read-only on Phone: its tooltip carries the arbiter's sentence. */
const latch = () => document.querySelector('.cockpit-txstrip .op-btn.monitor') as HTMLElement
/** The whole header and the whole strip with the sign cut out, as markup. */
const restOfHeader = () => {
  const strip = document.querySelector('.cockpit-txstrip')!.cloneNode(true) as HTMLElement
  strip.querySelector('.cq-statecap')!.replaceWith(document.createComment('the sign'))
  return document.querySelector('.cockpit-header')!.outerHTML + strip.outerHTML
}
/** Let the cockpit's chained microphone calls settle. */
const settle = () => act(async () => { for (let i = 0; i < 5; i++) await Promise.resolve() })

// What the station hands the window, as its contract spells it (webview.json).
const SPACE = (action: 'down' | 'up') => ({ type: 'key', action, key: ' ', code: 'Space', modifiers: 0, repeat: false })
const pointer = (action: 'down' | 'up') =>
  ({ type: 'pointer', action, x: 0.5, y: 0.5, button: 0, buttons: action === 'down' ? 1 : 0,
    modifiers: 0, pointerType: 'mouse', clicks: 1 })
const streamed = async (message: object) => { act(() => { stream!.handle(message) }); await settle() }

// The engine's own sentences (`TxOwner::busy_reason`), as the snapshot carries them.
const MIC = 'The Remote microphone is transmitting — stop it first'
const HELD = 'Mic PTT is held — release it first'
const TUNE = 'Tune carrier is up — stop tuning first'
const VOICE = 'A voice message is transmitting — stop it first'
const ARMED = 'Armed — talk to transmit'

// Each PTT a streamed operator can hold, and what this cockpit's button reads while it is armed.
const PRESSES: [string, () => Promise<void>, string][] = [
  ["the page's own Hold PTT, nothing pressed here", async () => {}, 'PUSH TO TALK'],
  ['the PTT button pressed through the picture', async () => { under = ptt(); await streamed(pointer('down')) }, ARMED],
  ['Space over the picture', () => streamed(SPACE('down')), ARMED],
]

describe("the Phone screen's ON AIR sign waits for the voice, like the PTT button", () => {
  it.each(PRESSES)('%s: dark while the station reports the over armed, lit once the voice keys it', async (_how, press, button) => {
    const view = render(phone())
    await press()
    view.rerender(phone({ streamMic: 'armed', txBusyReason: MIC }))
    expect(ptt().textContent, 'premise: the button').toBe(button)
    expect(lit(), 'the sign lit while the over is only armed').toBe(false)
    expect(sign().textContent, 'the sign said TX while the over is only armed').toBe(RX)
    view.rerender(phone({ streamMic: 'keyed', txBusyReason: MIC }))
    expect(lit(), 'the voice keyed the rig and the sign stayed dark').toBe(true)
    expect(sign().textContent).toBe(TX)
  })

  it.each([
    ['nothing else', {}, { txBusyReason: HELD }],
    // A key at the shack riding along with an over the page armed: the arbiter names the key
    // first, so the station no longer reports the over armed (`Engine::snapshot`).
    ['while the page holds an over armed', { streamMic: 'armed', txBusyReason: MIC }, { streamMic: null, txBusyReason: HELD }],
  ])('CONTROL: a press at the shack lights it as before, %s', async (_while, before, keyed) => {
    const view = render(phone(before))
    fireEvent.pointerDown(ptt())
    await settle()
    expect(setPtt).toHaveBeenLastCalledWith(true)
    view.rerender(phone(keyed))
    expect(lit(), 'a shack press').toBe(true)
    expect(sign().textContent).toBe(TX)
  })

  it.each([
    // What the station reports with something else holding the transmitter: the over not armed.
    ['the tune carrier', { streamMic: null, tuning: true, txBusyReason: TUNE }],
    ['a key at the shack', { streamMic: null, txBusyReason: HELD }],
    // The rig's own PTT read back is not one of the arbiter's owners, so the station can report the
    // over armed while a key at the radio is on the air: the sign's own guard lights it.
    ['a key at the radio', { streamMic: 'armed', txBusyReason: MIC, rigKeyed: true }],
    // A station built before the Q1 ruling said `armed` whatever else held the transmitter.
    ['the tune carrier, as an older station reports it', { streamMic: 'armed', tuning: true, txBusyReason: TUNE }],
    ['an FT over, as an older station reports it', { streamMic: 'armed', transmitting: true, txBusyReason: MIC }],
  ])('%s, over an armed over, lights it as before', (_what, radio) => {
    const view = render(phone({ streamMic: 'armed', txBusyReason: MIC }))
    expect(lit(), 'premise: the armed over alone leaves it dark').toBe(false)
    view.rerender(phone(radio))
    expect(lit(), 'on the air, and the sign dark').toBe(true)
    expect(sign().textContent).toBe(TX)
  })

  it('an over already on the air when the press is made (the voice keyer) keeps it lit while the station answers', async () => {
    // The button reads Armed from the press, before the station has answered the arm; the sign
    // waits only for an over the station itself reports armed.
    let answer: (armed: boolean) => void = () => {}
    armStreamMic.mockImplementation(() => new Promise<boolean>((resolve) => { answer = resolve }))
    render(phone({ txBusyReason: VOICE }))
    under = ptt()
    await streamed(pointer('down'))
    expect(ptt().textContent, 'premise: the press reads Armed until the station answers').toBe(ARMED)
    expect(lit(), 'the voice keyer on the air, and the sign dark').toBe(true)
    // Refused, the voice keyer owning the transmitter: nothing is held, and the sign reads as before.
    await act(async () => { answer(false) })
    await settle()
    expect(ptt().textContent).toBe('PUSH TO TALK')
    expect(lit()).toBe(true)
  })

  it('display only: the voice keying changes the sign and nothing else in the header, and Stop TX stops while it waits', () => {
    const view = render(phone({ streamMic: 'armed', txBusyReason: MIC }))
    expect(lit(), 'premise: the sign waits').toBe(false)
    const waiting = restOfHeader()
    const title = sign().title
    expect(latch().title, "premise: the latch's tooltip is the arbiter's sentence").toBe(MIC)
    view.rerender(phone({ streamMic: 'keyed', txBusyReason: MIC }))
    expect(lit(), 'premise: keyed').toBe(true)
    expect(sign().title, "the sign's tooltip, its accessible description, changed").toBe(title)
    expect(latch().title, "the latch's tooltip changed").toBe(MIC)
    expect(restOfHeader(), 'something in the header or the strip besides the sign changed').toBe(waiting)
    view.rerender(phone({ streamMic: 'armed', txBusyReason: MIC }))
    fireEvent.click(screen.getByRole('button', { name: /^stop tx$/i }))
    expect(api.haltTx, 'Stop TX while the sign waits').toHaveBeenCalledTimes(1)
  })
})
