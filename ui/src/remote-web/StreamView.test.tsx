// @vitest-environment jsdom
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react'
import { StreamView } from './StreamView'
import type { HostedConnection } from './client'
import type { OperationState } from './operation-protocol'
import type { OperationView } from './operation-client'
import { ANSWER, CHANNEL, LEASE, SIGNAL, byName, harness, last } from './stream-link.testkit'

const BOOT = '0f7d1c2e-5b3a-4c1d-9e8f-7a6b5c4d3e2f'
const EPOCH = '000000000000002b'
function state(phase: OperationState['phase'], extra: Partial<OperationState> = {}): OperationState {
  return { stationBootId: BOOT, allowed: true, phase, leaseId: phase === 'controlling' ? LEASE : null, revision: 1,
    commandWindowId: null, nextSequence: null, leaseRemainingMs: phase === 'controlling' ? 5000 : null, actions: [],
    txArmed: false, transmitEpoch: phase === 'controlling' ? EPOCH : null, ...extra } as OperationState
}

// jsdom has no WebRTC and never presents a video frame, so the element gets the one method the link
// reads, and each test presents frames by hand.
const frames = new Map<HTMLVideoElement, (now: number, metadata: { rtpTimestamp?: number }) => void>()
beforeEach(() => {
  Object.defineProperty(HTMLVideoElement.prototype, 'requestVideoFrameCallback', { configurable: true,
    value(this: HTMLVideoElement, callback: (now: number, metadata: { rtpTimestamp?: number }) => void) { frames.set(this, callback); return 1 } })
  Object.defineProperty(HTMLVideoElement.prototype, 'cancelVideoFrameCallback', { configurable: true, value(this: HTMLVideoElement) { frames.delete(this) } })
})
afterEach(() => {
  cleanup(); frames.clear()
  delete (HTMLVideoElement.prototype as { requestVideoFrameCallback?: unknown }).requestVideoFrameCallback
  delete (HTMLVideoElement.prototype as { cancelVideoFrameCallback?: unknown }).cancelVideoFrameCallback
})

function view(initial: Partial<OperationView>, options: Parameters<typeof harness>[0] = {}) {
  const h = harness(options)
  let snapshot = { supported: true, state: null, fresh: false, connected: true, busy: false, stopAvailable: false,
    stopSending: false, stopAccepted: false, ...initial } as OperationView
  const listeners = new Set<() => void>()
  const operations = {
    subscribe: (f: () => void) => { listeners.add(f); return () => { listeners.delete(f) } },
    getSnapshot: () => snapshot,
    acquire: vi.fn(async () => {}),
    release: vi.fn(async () => {}),
    stopTransmit: vi.fn(async () => ({ stop: 'accepted' })),
  }
  const connection = { operations, stream: h.link, source: { id: 'fake', kind: 'native', read: () => Promise.reject(Error('none')) } } as unknown as HostedConnection
  const utils = render(<StreamView connection={connection} station="Home" disconnect={() => {}} signOut={() => {}} />)
  const video = utils.container.querySelector('video')!
  // A 16:9 picture laid out at 1600×900, so every point on it maps to the frame one to one.
  Object.defineProperty(video, 'videoWidth', { configurable: true, value: 1920 })
  Object.defineProperty(video, 'videoHeight', { configurable: true, value: 1080 })
  video.getBoundingClientRect = () => ({ left: 0, top: 100, width: 1600, height: 900, right: 1600, bottom: 1000, x: 0, y: 100, toJSON: () => ({}) })
  return {
    // `h.peer` is a getter over the peers made so far; spreading would freeze it at none.
    link: h.link, peers: h.peers, signals: h.signals, get peer() { return h.peer }, tick: h.tick, operations, connection, video,
    micAsks: h.micAsks, micTracks: h.micTracks,
    /** The station's word on its microphone over, as the contract carries it. */
    station: (name: string) => act(() => { h.peer.channel('control').deliver(byName(CHANNEL.controlStationToBrowser, name)) }),
    set: (next: Partial<OperationView>) => act(() => { snapshot = { ...snapshot, ...next }; for (const f of listeners) f() }),
    /** Answer, open the channels and present a frame: the stream is live. */
    async live() {
      await act(async () => { await Promise.resolve() })
      await act(async () => { h.link.receive({ type: 'streamSignal', payload: { kind: 'answer', sdp: ANSWER } }); await Promise.resolve() })
      act(() => {
        h.peer.ontrack?.({ track: 'track', streams: ['media'] })
        for (const label of ['control', 'ptt', 'audio']) h.peer.channel(label).open()
        frames.get(video)?.(0, { rtpTimestamp: 90000 })
      })
    },
  }
}
const controlling = { state: state('controlling'), fresh: true }

it('A3: offers nothing until the operator asks, and then only under a fresh controlling lease', async () => {
  const v = view({ state: state('available'), fresh: true })
  expect(v.peers).toHaveLength(0)
  fireEvent.click(screen.getByRole('button', { name: 'Start the stream' }))
  expect(v.operations.acquire).toHaveBeenCalledTimes(1)
  await act(async () => { await Promise.resolve() })
  expect(v.peers, 'asked for, but no lease yet: still nothing').toHaveLength(0)
  // The lease arrives, but its state is stale: still nothing.
  v.set({ state: state('controlling'), fresh: false })
  await act(async () => { await Promise.resolve() })
  expect(v.peers).toHaveLength(0)
  // Positive control: fresh, and the offer goes, under that lease.
  v.set({ fresh: true })
  await act(async () => { await Promise.resolve() })
  expect(v.peers).toHaveLength(1)
  expect(v.signals).toEqual([expect.objectContaining({ leaseId: LEASE, payload: expect.objectContaining({ kind: 'offer' }) })])
})

it('A3: holding control is not asking for a stream - no offer without the operator\'s start', async () => {
  const v = view(controlling)
  await act(async () => { await Promise.resolve() })
  expect(v.peers).toHaveLength(0)
  fireEvent.click(screen.getByRole('button', { name: 'Start the stream' }))
  await act(async () => { await Promise.resolve() })
  expect(v.operations.acquire, 'already controlling: nothing to acquire').not.toHaveBeenCalled()
  expect(v.peers).toHaveLength(1)
})

it('ends the stream at once when station control goes, and says why', async () => {
  const v = view(controlling)
  fireEvent.click(screen.getByRole('button', { name: 'Start the stream' }))
  await v.live()
  expect(v.link.getSnapshot().phase).toBe('live')
  v.set({ state: state('available') })
  expect(v.link.getSnapshot()).toMatchObject({ phase: 'ended', reason: 'notController' })
  expect(screen.getAllByText('The stream ended because station control was lost.').length).toBeGreaterThan(0)
  expect(v.peer.closed).toBe(true)
})

it('THE STOP LINE: Stop TX is on screen in every state, enabled whenever a path can carry it, and uses both', async () => {
  const stopButton = () => screen.getByRole('button', { name: 'Stop TX' }) as HTMLButtonElement
  for (const [what, initial] of [
    ['disconnected', { connected: false }], ['waiting for the station', {}], ['control available', { state: state('available'), fresh: true }],
    ['control elsewhere', { state: state('occupied'), fresh: true }], ['permission required', { state: state('localPermissionRequired'), fresh: true }],
  ] as const) {
    view(initial as Partial<OperationView>)
    expect(stopButton(), what).toBeTruthy()
    cleanup()
  }
  // A station-issued stop token alone enables it, stream or no stream.
  const token = view({ state: state('available'), fresh: true, stopAvailable: true })
  expect(stopButton().disabled).toBe(false)
  fireEvent.click(stopButton())
  expect(token.operations.stopTransmit).toHaveBeenCalledTimes(1)
  cleanup()
  // Live: the stream's control channel carries it too, at the same moment.
  const live = view({ ...controlling, stopAvailable: true })
  fireEvent.click(screen.getByRole('button', { name: 'Start the stream' }))
  await live.live()
  fireEvent.click(stopButton())
  expect(last(live.peer.channel('control').sent)).toMatchObject({ type: 'stopTransmit', stationBootId: BOOT, leaseId: LEASE, transmitEpoch: EPOCH })
  expect(live.operations.stopTransmit).toHaveBeenCalledTimes(1)
  // Stalled - the picture frozen for longer than the blind limit - and it is still there, still live.
  live.tick(2500)
  await act(async () => { await new Promise(resolve => setTimeout(resolve, 300)) })
  expect(live.link.getSnapshot().phase).toBe('stalled')
  expect(stopButton().disabled).toBe(false)
  // And after the stream has died, the socket's stop token still carries it.
  act(() => { live.link.close('streamFailed') })
  expect(live.link.getSnapshot().phase).toBe('ended')
  expect(stopButton().disabled).toBe(false)
  cleanup()
  // Control: no token and no stream means nothing could carry a Stop, and the button says so.
  view({ state: state('available'), fresh: true })
  expect(stopButton().disabled).toBe(true)
})

it('Stop, outside the picture, still reaches the station when the picture is frozen and after the lease has lapsed', async () => {
  const v = view({ ...controlling, stopAvailable: true })
  const stop = () => screen.getByRole('button', { name: 'Stop TX' }) as HTMLButtonElement
  expect(stop().closest('.remote-stream-stage'), 'Stop is not part of the picture').toBeNull()
  fireEvent.click(screen.getByRole('button', { name: 'Start the stream' }))
  await v.live()
  const control = v.peer.channel('control')
  const stops = () => control.sent.filter(m => m.type === 'stopTransmit')
  // Blind: no new frame for longer than the limit. The station no longer lets this browser key
  // anything, and Stop still goes both ways.
  v.tick(2500)
  await act(async () => { await new Promise(resolve => setTimeout(resolve, 300)) })
  expect(v.link.getSnapshot().phase).toBe('stalled')
  fireEvent.click(stop())
  expect(stops()).toEqual([expect.objectContaining({ stationBootId: BOOT, leaseId: LEASE, transmitEpoch: EPOCH })])
  expect(v.operations.stopTransmit).toHaveBeenCalledTimes(1)
  // The lease lapses. The stream goes with it, but the station still issues this browser its stop
  // token, and the socket carries the Stop on it.
  v.set({ state: state('available', { transmitEpoch: EPOCH }) })
  expect(v.link.getSnapshot()).toMatchObject({ phase: 'ended', reason: 'notController' })
  expect(stop().disabled).toBe(false)
  fireEvent.click(stop())
  expect(v.operations.stopTransmit).toHaveBeenCalledTimes(2)
  expect(stops(), 'the dead stream carried nothing more').toHaveLength(1)
})

it('A2 (the page\'s half): only input aimed at the focused picture is sent; the rest of the page sends nothing', async () => {
  const v = view(controlling)
  fireEvent.click(screen.getByRole('button', { name: 'Start the stream' }))
  await v.live()
  const control = v.peer.channel('control')
  const input = { get sent() { return control.sent.filter(m => m.type !== 'heartbeat') } }
  // Keys on the page's own controls, and pointers on its header, stay on the page.
  const stop = screen.getByRole('button', { name: 'Stop TX' })
  stop.focus()
  fireEvent.keyDown(stop, { key: 'a', code: 'KeyA' })
  fireEvent.keyDown(document.body, { key: 'b', code: 'KeyB' })
  document.querySelector('header')!.dispatchEvent(new MouseEvent('pointerdown', { bubbles: true, clientX: 10, clientY: 10, button: 0, buttons: 1 }))
  // A press on the bars around the picture is not a press on Nexus.
  v.video.dispatchEvent(new MouseEvent('pointerdown', { bubbles: true, clientX: 10, clientY: 50, button: 0, buttons: 1 }))
  await new Promise(resolve => setTimeout(resolve, 20))
  expect(input.sent).toEqual([])
  // Positive control: the same gestures on the picture itself are sent, normalised to the frame.
  v.video.dispatchEvent(new MouseEvent('pointerdown', { bubbles: true, clientX: 800, clientY: 550, button: 0, buttons: 1, detail: 1 }))
  expect(document.activeElement).toBe(v.video)
  fireEvent.keyDown(v.video, { key: 'a', code: 'KeyA' })
  expect(input.sent).toEqual([
    { type: 'pointer', action: 'down', x: 0.5, y: 0.5, button: 0, buttons: 1, modifiers: 0, pointerType: 'mouse', clicks: 1 },
    { type: 'key', action: 'down', key: 'a', code: 'KeyA', modifiers: 0, repeat: false },
  ])
  // Tab is the keyboard's way out of the picture, and is never sent.
  fireEvent.keyDown(v.video, { key: 'Tab', code: 'Tab' })
  expect(input.sent).toHaveLength(2)
})

it('a click on the picture is a click at the shack, counted here: a pointer event carries no click count', async () => {
  const v = view(controlling)
  fireEvent.click(screen.getByRole('button', { name: 'Start the stream' }))
  await v.live()
  const control = v.peer.channel('control')
  const presses = () => control.sent.filter(m => m.type === 'pointer').map(m => `${m.action} ${m.clicks}`)
  // As Chrome sends them: pointerdown and pointerup with `detail` 0, whatever the click count.
  const click = (x: number) => {
    v.video.dispatchEvent(new MouseEvent('pointerdown', { bubbles: true, clientX: x, clientY: 550, button: 0, buttons: 1, detail: 0 }))
    v.video.dispatchEvent(new MouseEvent('pointerup', { bubbles: true, clientX: x, clientY: 550, button: 0, buttons: 0, detail: 0 }))
  }
  click(800)
  click(801)
  expect(presses(), 'a click, then the second click of a double click').toEqual(['down 1', 'up 1', 'down 2', 'up 2'])
  // Control: a press somewhere else is a first click again.
  click(1200)
  expect(presses().slice(4)).toEqual(['down 1', 'up 1'])
})

it('releases at the shack whatever it pressed there when the picture loses focus', async () => {
  const v = view(controlling)
  fireEvent.click(screen.getByRole('button', { name: 'Start the stream' }))
  await v.live()
  const control = v.peer.channel('control')
  const input = { get sent() { return control.sent.filter(m => m.type !== 'heartbeat') } }
  v.video.focus()
  // Space is the Phone cockpit's push-to-talk key: a key-up this page never sees must not leave it down.
  fireEvent.keyDown(v.video, { key: ' ', code: 'Space' })
  v.video.dispatchEvent(new MouseEvent('pointerdown', { bubbles: true, clientX: 800, clientY: 550, button: 0, buttons: 1 }))
  fireEvent.blur(v.video)
  expect(input.sent.slice(2)).toEqual([
    { type: 'key', action: 'up', key: ' ', code: 'Space', modifiers: 0, repeat: false },
    { type: 'pointer', action: 'up', x: 0.5, y: 0.5, button: 0, buttons: 0, modifiers: 0, pointerType: 'mouse', clicks: 0 },
  ])
  // Control: released once, not again on the next blur.
  fireEvent.blur(v.video)
  expect(input.sent).toHaveLength(4)
})

it('blind means no authority: a frozen picture sends no input, and lets go of what it held when it froze', async () => {
  const v = view(controlling)
  fireEvent.click(screen.getByRole('button', { name: 'Start the stream' }))
  await v.live()
  const control = v.peer.channel('control')
  const input = () => control.sent.filter(m => m.type !== 'heartbeat')
  const press = () => v.video.dispatchEvent(new MouseEvent('pointerdown', { bubbles: true, clientX: 800, clientY: 550, button: 0, buttons: 1, detail: 1 }))
  press()
  expect(input()).toHaveLength(1)
  v.tick(2500)
  await act(async () => { await new Promise(resolve => setTimeout(resolve, 300)) })
  expect(v.link.getSnapshot().phase).toBe('stalled')
  // What was held comes up the moment the picture freezes, not when the operator next moves.
  expect(input().slice(1)).toEqual([{ type: 'pointer', action: 'up', x: 0.5, y: 0.5, button: 0, buttons: 0, modifiers: 0, pointerType: 'mouse', clicks: 0 }])
  // Nothing new goes while it stays frozen.
  press()
  fireEvent.keyDown(v.video, { key: 'a', code: 'KeyA' })
  await new Promise(resolve => setTimeout(resolve, 20))
  expect(input()).toHaveLength(2)
  // Positive control: a new frame, and the picture takes input again.
  act(() => frames.get(v.video)?.(0, { rtpTimestamp: 180000 }))
  expect(v.link.getSnapshot().phase).toBe('live')
  fireEvent.keyDown(v.video, { key: 'a', code: 'KeyA' })
  expect(last(input())).toMatchObject({ type: 'key', action: 'down', key: 'a' })
})

it('THE DEAD-MAN, the page\'s half: what is held on the picture is re-asserted on ptt every 100 ms, and no longer once let go', async () => {
  const v = view(controlling)
  fireEvent.click(screen.getByRole('button', { name: 'Start the stream' }))
  await v.live()
  const ptt = v.peer.channel('ptt')
  const helds = () => ptt.sent.filter(m => m.type === 'held')
  const wait = (ms: number) => act(async () => { await new Promise(resolve => setTimeout(resolve, ms)) })
  v.video.focus()
  // A Space held on the picture: sent as the key it is, and re-asserted as held, at once and every 100 ms.
  fireEvent.keyDown(v.video, { key: ' ', code: 'Space' })
  expect(helds()).toEqual([{ type: 'held', keys: ['Space'], buttons: 0, seq: 0 }])
  await wait(250)
  expect(helds().length, 'at once, then every 100 ms').toBeGreaterThanOrEqual(3)
  expect(helds().map(m => m.seq), 'counting up').toEqual(helds().map((_, i) => i))
  // A button pressed too: the set grows, at once.
  v.video.dispatchEvent(new MouseEvent('pointerdown', { bubbles: true, clientX: 800, clientY: 550, button: 0, buttons: 1 }))
  expect(last(helds())).toMatchObject({ keys: ['Space'], buttons: 1 })
  fireEvent.keyUp(v.video, { key: ' ', code: 'Space' })
  expect(last(helds())).toMatchObject({ keys: [], buttons: 1 })
  // Nothing held, nothing re-asserted: the shack's window has had every release.
  v.video.dispatchEvent(new MouseEvent('pointerup', { bubbles: true, clientX: 800, clientY: 550, button: 0, buttons: 0 }))
  const sent = helds().length
  await wait(250)
  expect(helds()).toHaveLength(sent)
})

it('lets go of what the picture holds when the browser window loses focus: its key-ups will go elsewhere', async () => {
  const v = view(controlling)
  fireEvent.click(screen.getByRole('button', { name: 'Start the stream' }))
  await v.live()
  const control = v.peer.channel('control'), ptt = v.peer.channel('ptt')
  v.video.focus()
  fireEvent.keyDown(v.video, { key: 'Shift', code: 'ShiftLeft', shiftKey: true })
  act(() => { window.dispatchEvent(new Event('blur')) })
  expect(last(control.sent)).toMatchObject({ type: 'key', action: 'up', code: 'ShiftLeft' })
  const sent = ptt.sent.filter(m => m.type === 'held').length
  await act(async () => { await new Promise(resolve => setTimeout(resolve, 250)) })
  expect(ptt.sent.filter(m => m.type === 'held'), 'no longer re-asserted').toHaveLength(sent)
})

it('holds PTT while the button is held, and lets go when it is released', async () => {
  const v = view(controlling)
  fireEvent.click(screen.getByRole('button', { name: 'Start the stream' }))
  await v.live()
  const ptt = screen.getByRole('button', { name: 'Hold PTT' })
  const channel = v.peer.channel('ptt')
  fireEvent.pointerDown(ptt, { button: 0 })
  expect(ptt.getAttribute('aria-pressed')).toBe('true')
  const holds = channel.sent.filter(m => m.type === 'pttHold')
  expect(holds).toHaveLength(1)
  fireEvent.pointerUp(ptt)
  expect(ptt.getAttribute('aria-pressed')).toBe('false')
  expect(last(channel.sent)).toEqual({ type: 'pttRelease', holdId: holds[0].holdId, seq: 1 })
})

it('a stream the station ended stays ended: nothing is re-offered until the operator asks again', async () => {
  const v = view(controlling)
  fireEvent.click(screen.getByRole('button', { name: 'Start the stream' }))
  await v.live()
  expect(v.peers).toHaveLength(1)
  act(() => { v.link.receive(byName(SIGNAL.roomToBrowser, 'refused: streamClosed')) })
  await act(async () => { await Promise.resolve() })
  expect(v.link.getSnapshot()).toMatchObject({ phase: 'ended', reason: 'streamClosed' })
  expect(v.peers, 'no second offer on its own').toHaveLength(1)
  // Positive control: asking again does start a new one.
  fireEvent.click(screen.getByRole('button', { name: 'Start the stream' }))
  await act(async () => { await Promise.resolve() })
  expect(v.peers).toHaveLength(2)
})

// A5: the station holds the offer to the key it pinned for this browser. Both refusals are ones the
// operator fixes at the radio, once, and the page says so in plain words (ruling D5 for the changed key).
it('A5: says in plain words why the station refused this browser\'s key, and what to do at the radio', async () => {
  for (const [reason, words] of [
    ['deviceNotPinned', 'Approve this browser again in Nexus at the shack, once, before it can stream.'],
    ['deviceKeyMismatch', 'This browser’s key has changed, so approve it again in Nexus at the shack before streaming.'],
  ] as const) {
    const v = view(controlling)
    fireEvent.click(screen.getByRole('button', { name: 'Start the stream' }))
    await act(async () => { await Promise.resolve() })
    act(() => { v.link.receive(byName(SIGNAL.roomToBrowser, `refused: ${reason}`)) })
    expect(v.link.getSnapshot()).toMatchObject({ phase: 'ended', reason })
    expect(screen.getAllByText(new RegExp(words.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'))).length, reason).toBeGreaterThan(0)
    cleanup()
  }
})

// ── The microphone (S6; the operator's rulings R1 "Arms your mic" and R2 "State it + warn") ─────────

const USB_NOTE = 'The rig must take its SSB audio from USB (its menu for the transmit audio source). Most radios come set to the front microphone, and then an over sends the shack\'s microphone instead of you.'
const NO_POWER = 'The rig shows no power out while your voice is arriving. Set its SSB audio source to USB (its menu for the transmit audio source).'
const NEEDED = 'PTT is held but your microphone is off, so nothing is transmitted. Turn the microphone on to talk.'
const ARMED = "mic armed, no audio yet (a held PTT keys nothing until the operator's voice arrives)"
const KEYED = "mic keyed by the operator's voice"
const KEYED_NO_POWER = 'mic keyed, and the rig reports no power out (display only)'

it('the microphone is off until the operator turns it on; only then does the browser ask, and that is where the page says the rig must take USB audio', async () => {
  const v = view(controlling)
  fireEvent.click(screen.getByRole('button', { name: 'Start the stream' }))
  await v.live()
  const mic = screen.getByRole('button', { name: 'Mic off' })
  expect(mic.getAttribute('aria-pressed')).toBe('false')
  expect(v.micAsks, 'the browser was asked before the operator turned the microphone on').toEqual([])
  expect(screen.queryByText(USB_NOTE)).toBeNull()
  await act(async () => { fireEvent.click(mic); await Promise.resolve() })
  expect(v.micAsks).toHaveLength(1)
  expect(screen.getByRole('button', { name: 'Mic on' }).getAttribute('aria-pressed')).toBe('true')
  expect(screen.getByText(USB_NOTE)).toBeTruthy()
  // Off again: the track stops, and the note goes with it.
  await act(async () => { fireEvent.click(screen.getByRole('button', { name: 'Mic on' })); await Promise.resolve() })
  expect(v.micTracks[0].stopped).toBe(true)
  expect(screen.queryByText(USB_NOTE)).toBeNull()
})

it('R1: a press with the microphone off keys nothing, and the page says why; CONTROL: with it on, or nothing armed, nothing is said', async () => {
  const v = view(controlling)
  fireEvent.click(screen.getByRole('button', { name: 'Start the stream' }))
  await v.live()
  // The station armed an over (this page's Hold PTT, or Space or the cockpit's PTT through the picture).
  v.station(ARMED)
  expect(screen.getByText(NEEDED)).toBeTruthy()
  await act(async () => { fireEvent.click(screen.getByRole('button', { name: 'Mic off' })); await Promise.resolve() })
  expect(screen.queryByText(NEEDED), 'the microphone is on: nothing is missing').toBeNull()
  // And with the microphone off but nothing armed, nothing is missing either.
  await act(async () => { fireEvent.click(screen.getByRole('button', { name: 'Mic on' })); await Promise.resolve() })
  v.station('mic over ended: released')
  expect(screen.queryByText(NEEDED)).toBeNull()
})

it('R2: the station seeing no power out while the voice arrives is said, DISPLAY ONLY; CONTROL: an over with power out says nothing', async () => {
  const v = view(controlling)
  fireEvent.click(screen.getByRole('button', { name: 'Start the stream' }))
  await v.live()
  v.station(KEYED)
  expect(screen.queryByText(NO_POWER)).toBeNull()
  const sent = v.peer.channel('ptt').sent.length + v.peer.channel('control').sent.length
  v.station(KEYED_NO_POWER)
  expect(screen.getByText(NO_POWER)).toBeTruthy()
  expect(v.peer.channel('ptt').sent.length + v.peer.channel('control').sent.length, 'the warning sent something').toBe(sent)
  // The over ends; the warning goes with it.
  v.station('mic over ended: released')
  expect(screen.queryByText(NO_POWER)).toBeNull()
})

it('§7: an over the station ended is explained in words; one the operator let go of or stopped is not', async () => {
  const v = view(controlling)
  fireEvent.click(screen.getByRole('button', { name: 'Start the stream' }))
  await v.live()
  for (const [why, text] of [
    ['audioGap', 'Your audio stopped arriving, so the transmission stopped.'],
    ['presence', 'Control was lost, so the transmission stopped.'],
    ['ceiling', 'The transmission ended at its 10-minute limit. Press PTT again to carry on.'],
    ['watchdog', 'The TX watchdog stopped the transmission.'],
    ['routeChanged', "The station's audio changed, so the transmission stopped. Press PTT again."],
  ]) {
    v.station(`mic over ended: ${why}`)
    expect(screen.getByText(text).getAttribute('role'), why).toBe('alert')
  }
  for (const why of ['released', 'stopped']) {
    v.station(`mic over ended: ${why}`)
    expect(screen.queryByRole('list', { name: 'Microphone' }), why).toBeNull()
  }
  // A new over clears the last one's caption.
  v.station('mic over ended: audioGap')
  v.station(ARMED)
  expect(screen.queryByText('Your audio stopped arriving, so the transmission stopped.')).toBeNull()
})

it('M9: the station\'s audio is muted, not ducked, while the operator\'s own over is on the air, and comes back when it ends', async () => {
  const v = view(controlling)
  fireEvent.click(screen.getByRole('button', { name: 'Start the stream' }))
  await v.live()
  const mute = vi.spyOn(v.link.audio, 'setMuted')
  const ptt = screen.getByRole('button', { name: 'Hold PTT' })
  fireEvent.pointerDown(ptt, { button: 0 })
  expect(last(mute.mock.calls)).toEqual([true])
  fireEvent.pointerUp(ptt)
  expect(last(mute.mock.calls)).toEqual([false])
  // An over keyed from the picture (the cockpit's own PTT) mutes too, by the station's word.
  v.station(KEYED)
  expect(last(mute.mock.calls)).toEqual([true])
  v.station('mic over ended: audioGap')
  expect(last(mute.mock.calls)).toEqual([false])
})

it('M11: nine minutes into a run of overs the page says to identify, and once more when the stream ends; CONTROL: a short over is not prompted', async () => {
  vi.useFakeTimers({ toFake: ['Date', 'setInterval', 'clearInterval', 'setTimeout', 'clearTimeout'] })
  try {
    const DUE = 'Time to give your call sign.', END = 'Remember to give your call sign at the end of the contact.'
    const v = view(controlling)
    fireEvent.click(screen.getByRole('button', { name: 'Start the stream' }))
    await v.live()
    // CONTROL: a 30-second over and then listening: never prompted.
    v.station(KEYED)
    act(() => { vi.advanceTimersByTime(30_000) })
    v.station('mic over ended: released')
    act(() => { vi.advanceTimersByTime(9 * 60_000) })
    expect(screen.queryByText(DUE)).toBeNull()
    // A run: on the air again within the run, and past nine minutes from its start the prompt shows.
    v.station(KEYED)
    act(() => { vi.advanceTimersByTime(2000) })
    expect(screen.getByText(DUE)).toBeTruthy()
    // It shows for thirty seconds, and keys nothing.
    act(() => { vi.advanceTimersByTime(31_000) })
    expect(screen.queryByText(DUE)).toBeNull()
    v.station('mic over ended: released')
    // The stream ends with that run in it: the last prompt.
    fireEvent.click(screen.getByRole('button', { name: 'End the stream' }))
    expect(screen.getByText(END)).toBeTruthy()
  } finally { vi.useRealTimers() }
})

it('M11 control: a stream with no over in it ends with no prompt', async () => {
  const v = view(controlling)
  fireEvent.click(screen.getByRole('button', { name: 'Start the stream' }))
  await v.live()
  fireEvent.click(screen.getByRole('button', { name: 'End the stream' }))
  expect(screen.queryByText('Remember to give your call sign at the end of the contact.')).toBeNull()
})

// REMOVAL STAGE 3 (the change plan's §4.4): with the old workspace gone, the stream is where a browser
// holding control listens and stops the transmitter. Both are on it while it runs.
it('Listen and Stop TX are on the stream while it runs under control', async () => {
  const offered = state('controlling', { controls: { context: {}, capabilities: ['audioListen'] } } as unknown as Partial<OperationState>)
  const v = view({ state: offered, fresh: true })
  expect(screen.getByRole('button', { name: 'Stop TX' })).toBeTruthy()
  fireEvent.click(screen.getByRole('button', { name: 'Start the stream' }))
  await v.live()
  expect(v.link.getSnapshot().phase).toBe('live')
  expect(screen.getByRole('button', { name: 'Listen' })).toBeTruthy()
  expect(screen.getByRole('button', { name: 'Stop TX' })).toBeTruthy()
})
