// @vitest-environment jsdom
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react'
import { StreamView } from './StreamView'
import type { HostedConnection } from './client'
import type { OperationState } from './operation-protocol'
import type { OperationView } from './operation-client'
import { ANSWER, LEASE, SIGNAL, byName, harness, last } from './stream-link.testkit'
import { armStreamPttKey, StreamInputDispatcher } from '../remote-native/stream-input'

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

function view(initial: Partial<OperationView>) {
  const h = harness()
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
  // A key held down at the shack (here a space into a field: the station has not said Space is its
  // PTT key) must not stay down there because this page never saw its key-up.
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

it('THE PTT KEY NEVER TRAVELS AS A KEY: a Space press over the stream is held PTT, and no Space key event reaches Nexus\'s window', async () => {
  // Nexus's window at the shack, apart from this page: its own document, with the input bridge in it,
  // and the Phone cockpit's Space armed (control, Lock off) as the cockpit arms it.
  const frame = document.body.appendChild(document.createElement('iframe'))
  const station = frame.contentWindow as Window & typeof globalThis
  const keys: string[] = []
  const listen = (event: KeyboardEvent) => keys.push(`${event.type} ${event.code}`)
  station.addEventListener('keydown', listen); station.addEventListener('keyup', listen)
  let under: Element = station.document.body
  Object.defineProperty(station.document, 'elementFromPoint', { configurable: true, value: () => under })
  const disarm = armStreamPttKey()
  const v = view(controlling)
  fireEvent.click(screen.getByRole('button', { name: 'Start the stream' }))
  await v.live()
  const control = v.peer.channel('control'), ptt = v.peer.channel('ptt')
  // The station between them: the window's word goes to the page, the page's input to the window.
  const bridge = new StreamInputDispatcher(station, armed => act(() => control.deliver({ type: 'pttKey', armed })))
  const sentKeys = () => control.sent.filter(m => m.type === 'key')
  const relay = () => { for (const m of control.sent.splice(0)) if (m.type !== 'heartbeat') bridge.handle(m) }
  try {
    // The reset a stream starts with: the window says where it stands.
    bridge.handle({ type: 'reset' })
    expect(v.link.getSnapshot().pttKey).toBe(true)
    v.video.focus()
    fireEvent.keyDown(v.video, { key: ' ', code: 'Space' })
    await act(async () => { await new Promise(resolve => setTimeout(resolve, 250)) })
    fireEvent.keyDown(v.video, { key: ' ', code: 'Space', repeat: true })
    fireEvent.keyUp(v.video, { key: ' ', code: 'Space' })
    const holds = ptt.sent.filter(m => m.type === 'pttHold')
    expect(holds.length, 're-asserted while held').toBeGreaterThanOrEqual(3)
    expect(new Set(holds.map(m => m.holdId)).size, 'one press, one hold id').toBe(1)
    expect(last(ptt.sent)).toMatchObject({ type: 'pttRelease', holdId: holds[0].holdId })
    expect(sentKeys(), 'the page sent no key for it').toEqual([])
    relay()
    expect(keys, 'and nothing dispatched a Space key event in Nexus\'s window').toEqual([])
    // Positive control: a click puts focus in a field at the shack, the window says Space is typing
    // there, and the same press goes as a key and is typed - nothing is held.
    under = station.document.body.appendChild(station.document.createElement('input'))
    v.video.dispatchEvent(new MouseEvent('pointerdown', { bubbles: true, clientX: 800, clientY: 550, button: 0, buttons: 1, detail: 1 }))
    v.video.dispatchEvent(new MouseEvent('pointerup', { bubbles: true, clientX: 800, clientY: 550, button: 0, buttons: 0, detail: 1 }))
    relay()
    expect(v.link.getSnapshot().pttKey).toBe(false)
    const held = ptt.sent.length
    fireEvent.keyDown(v.video, { key: ' ', code: 'Space' })
    fireEvent.keyUp(v.video, { key: ' ', code: 'Space' })
    expect(sentKeys().map(m => m.action)).toEqual(['down', 'up'])
    relay()
    expect(keys).toEqual(['keydown Space', 'keyup Space'])
    expect((under as HTMLInputElement).value).toBe(' ')
    expect(ptt.sent, 'nothing held').toHaveLength(held)
  } finally {
    disarm(); bridge.dispose(); frame.remove()
  }
})

it('a Space held as PTT lets go when the picture loses focus, and its key-up is never sent as a key', async () => {
  const v = view(controlling)
  fireEvent.click(screen.getByRole('button', { name: 'Start the stream' }))
  await v.live()
  const control = v.peer.channel('control'), ptt = v.peer.channel('ptt')
  const keys = () => control.sent.filter(m => m.type === 'key')
  act(() => control.deliver({ type: 'pttKey', armed: true }))
  v.video.focus()
  fireEvent.keyDown(v.video, { key: ' ', code: 'Space' })
  expect(v.link.getSnapshot().ptt).toBe(true)
  fireEvent.blur(v.video)
  expect(v.link.getSnapshot().ptt).toBe(false)
  expect(last(ptt.sent)).toMatchObject({ type: 'pttRelease' })
  // Focus comes back before the key does: that key-up belongs to the PTT press, and stays here.
  v.video.focus()
  fireEvent.keyUp(v.video, { key: ' ', code: 'Space' })
  expect(keys()).toEqual([])
  // Positive control: a new press is decided afresh - the station now says Space is typing there.
  act(() => control.deliver({ type: 'pttKey', armed: false }))
  fireEvent.keyDown(v.video, { key: ' ', code: 'Space' })
  fireEvent.keyUp(v.video, { key: ' ', code: 'Space' })
  expect(keys().map(m => m.action)).toEqual(['down', 'up'])
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
