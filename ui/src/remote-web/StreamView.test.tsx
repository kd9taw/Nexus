// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { StreamView } from './StreamView'
import type { HostedConnection } from './client'
import type { OperationState } from './operation-protocol'
import type { OperationView } from './operation-client'
import type { MonitorSource } from '../remote-monitor/session'
import { fixtureSource } from '../remote-monitor/fixtureSource'
import { ANSWER, CHANNEL, LEASE, SIGNAL, answerChecked, byName, harness, last } from './stream-link.testkit'
import { MIC_PEAK, micGain } from './mic-level'
import { STREAM_BLIND_MS, STREAM_UPLINK_BUDGET_BYTES } from './stream-protocol'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { chainOf, contrast, expandWith, parseRules, toRgb, tokensAt, winnerAt, type Mode } from '../cssCascade'

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

function view(initial: Partial<OperationView>, options: Parameters<typeof harness>[0] = {}, source?: MonitorSource) {
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
  const connection = { operations, stream: h.link, source: source ?? { id: 'fake', kind: 'native', read: () => Promise.reject(Error('none')) } } as unknown as HostedConnection
  const utils = render(<StreamView connection={connection} station="Home" disconnect={() => {}} signOut={() => {}} />)
  const video = utils.container.querySelector('video')!
  // A 16:9 picture laid out at 1600×900, so every point on it maps to the frame one to one.
  Object.defineProperty(video, 'videoWidth', { configurable: true, value: 1920 })
  Object.defineProperty(video, 'videoHeight', { configurable: true, value: 1080 })
  video.getBoundingClientRect = () => ({ left: 0, top: 100, width: 1600, height: 900, right: 1600, bottom: 1000, x: 0, y: 100, toJSON: () => ({}) })
  return {
    // `h.peer` is a getter over the peers made so far; spreading would freeze it at none.
    link: h.link, env: h.env, peers: h.peers, signals: h.signals, get peer() { return h.peer }, tick: h.tick, advance: h.advance, operations, connection, video,
    micAsks: h.micAsks, micTracks: h.micTracks, levelGraphs: h.levelGraphs,
    /** The station's word on its microphone over, as the contract carries it. */
    station: (name: string) => act(() => { h.peer.channel('control').deliver(byName(CHANNEL.controlStationToBrowser, name)) }),
    set: (next: Partial<OperationView>) => act(() => { snapshot = { ...snapshot, ...next }; for (const f of listeners) f() }),
    /** Answer, open the channels and present a frame: the stream is live. */
    async live() {
      await act(async () => { await Promise.resolve() })
      await act(async () => { h.link.receive({ type: 'streamSignal', payload: { kind: 'answer', sdp: ANSWER } }); await answerChecked() })
      act(() => {
        h.peer.ontrack?.({ track: 'track', streams: ['media'] })
        for (const label of ['control', 'ptt', 'audio']) h.peer.channel(label).open()
        frames.get(video)?.(0, { rtpTimestamp: 90000 })
      })
    },
  }
}
const controlling = { state: state('controlling'), fresh: true }

it('draws a picture the station sized to the stage one pixel to a device pixel, and fits any other as before', () => {
  let observed: { element: Element; report: (entries: unknown[]) => void } | null = null
  vi.stubGlobal('ResizeObserver', class {
    constructor(private readonly report: (entries: unknown[]) => void) {}
    observe(element: Element) { observed = { element, report: this.report } }
    disconnect() { observed = null }
  })
  try {
    const { video } = view({})
    expect(observed, 'nothing watched').not.toBeNull()
    expect(observed!.element, 'the stage is watched, not the picture: its drawn size never feeds back').toBe(video.parentElement)
    const stage = (width: number, height: number) => act(() => {
      observed!.report([{ devicePixelContentBoxSize: [{ inlineSize: width, blockSize: height }], contentRect: { width, height } }])
    })
    const drawn = () => [video.style.width, video.style.height, video.hasAttribute('data-exact')]
    // The 1920×1080 picture in a stage a pixel wider than it: its own size, not stretched by a hair.
    stage(1921, 1200)
    expect(drawn()).toEqual(['1920px', '1080px', true])
    // A stage far larger (the shack's window is the smaller): fitted to it, as before.
    stage(2560, 1440)
    expect(drawn()).toEqual(['', '', false])
    // A stage smaller than the picture (the station has not caught up): fitted, shrunk.
    stage(1600, 900)
    expect(drawn()).toEqual(['', '', false])
    // The station catches up: the next picture is the stage's size, drawn at its own.
    Object.defineProperty(video, 'videoWidth', { configurable: true, value: 1600 })
    Object.defineProperty(video, 'videoHeight', { configurable: true, value: 900 })
    act(() => { video.dispatchEvent(new Event('resize')) })
    expect(drawn()).toEqual(['1600px', '900px', true])
  } finally { vi.unstubAllGlobals() }
})

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

// A Stop the station refused used to be shown as nothing: the socket's refusal fell back to no
// word at all, and the stream's own answer was never read. Either path may carry the answer, and
// the first acceptance is it; a Stop that no path accepted says it failed.
it('a refused Stop says so, on either path, and an acceptance on either path is the answer', async () => {
  const FAILED = 'Could not stop transmit'
  const v = view({ ...controlling, stopAvailable: true })
  fireEvent.click(screen.getByRole('button', { name: 'Start the stream' }))
  await v.live()
  const control = v.peer.channel('control')
  const stop = () => fireEvent.click(screen.getByRole('button', { name: 'Stop TX' }))
  const answer = (reply: Record<string, unknown>) => act(() => {
    control.deliver({ type: 'operationResponse', requestId: last(control.sent.filter(m => m.type === 'stopTransmit'))!.requestId, ...reply })
  })
  const word = () => document.querySelector('.remote-stream-stopstate')?.textContent ?? null
  // Both paths refuse.
  stop()
  v.set({ stopSending: true })
  answer({ error: 'staleConnection' })
  expect(word(), 'still waiting on the socket').toBe('Sending stop…')
  v.set({ stopSending: false, stopError: 'notController' })
  expect(word(), 'a Stop refused on both paths was shown as nothing').toBe(FAILED)
  // The socket refuses, the stream accepts: the acceptance is the answer.
  stop()
  v.set({ stopError: 'remoteBusy' })
  answer({ value: { stop: 'accepted' } })
  expect(word()).toBe('Stop sent')
  // The stream path alone (the socket carried nothing), refused.
  v.set({ stopAvailable: false, stopError: null, stopAccepted: false })
  stop()
  expect(word()).toBe('Sending stop…')
  answer({ error: 'staleConnection' })
  expect(word(), 'the stream\'s refusal went unread').toBe(FAILED)
  // CONTROL: the same path's acceptance reads as one, and never as a failure.
  stop()
  answer({ value: { stop: 'accepted' } })
  expect(word()).toBe('Stop sent')
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

it('a press that began anywhere but on the picture is never sent, wherever it moves and is let go; CONTROL: a hover, and a drag that began on the picture, are', async () => {
  const v = view(controlling)
  fireEvent.click(screen.getByRole('button', { name: 'Start the stream' }))
  await v.live()
  // Laid out wider than the frame, so the picture has a bar either side of it (the frame is x 100..1700).
  v.video.getBoundingClientRect = () => ({ left: 0, top: 100, width: 1800, height: 900, right: 1800, bottom: 1000, x: 0, y: 100, toJSON: () => ({}) })
  const pointers = () => v.peer.channel('control').sent.filter(m => m.type === 'pointer').map(m => `${m.action} ${m.buttons}`)
  const held = () => v.peer.channel('ptt').sent.filter(m => m.type === 'held' && m.buttons !== 0)
  // A mouse's events on the picture as Chrome sends them: a move names no button of its own (-1), only those held.
  const mouse = (type: string, x: number, buttons: number) => v.video.dispatchEvent(new MouseEvent(type,
    { bubbles: true, clientX: x, clientY: 550, button: type === 'pointermove' ? -1 : 0, buttons }))
  const flushed = () => new Promise(resolve => setTimeout(resolve, 40))
  // Pressed on the page's own header. So is a press on "Still there?" or on More's cover, which can go while it is
  // held: the browser hit-tests the rest of the press, so its moves and its release land on the picture.
  document.querySelector('header')!.dispatchEvent(new MouseEvent('pointerdown', { bubbles: true, clientX: 10, clientY: 10, button: 0, buttons: 1 }))
  mouse('pointermove', 800, 1)
  mouse('pointermove', 900, 1)
  await flushed()
  mouse('pointerup', 900, 0)
  // Pressed on the bar beside the frame, which is no press on Nexus, and dragged onto the frame.
  mouse('pointerdown', 50, 1)
  mouse('pointermove', 800, 1)
  await flushed()
  mouse('pointerup', 800, 0)
  await flushed()
  expect(pointers(), 'nothing of either press reached Nexus').toEqual([])
  expect(held(), 'nor was anything held there').toEqual([])
  // CONTROL: the pointer over the picture with nothing held is the shack's pointer moving, and a press that began on
  // the frame drags there and is let go there.
  mouse('pointermove', 700, 0)
  await flushed()
  mouse('pointerdown', 800, 1)
  mouse('pointermove', 900, 1)
  await flushed()
  mouse('pointerup', 900, 0)
  expect(pointers()).toEqual(['move 0', 'down 1', 'move 1', 'up 0'])
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

// ── A held PTT and a greyed-out one (the operator's picks "Keep held PTT enabled" and "Refuse it on the page",
// 2026-10-04). The PTT is lit on a live picture under a fresh lease, and the operations state goes stale for a round
// trip now and then. Held, it stays lit through that: a browser that blurs a focused button it disables would end the
// over through the button's blur. Not held, a press on it while it is greyed out sends nothing, however it is pressed.

const pageTimers = () => vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval', 'setTimeout', 'clearTimeout'] })
/** Every way the page's PTT is pressed, and its let-go: a mouse, a pen or a finger on it (the phone's rail and thumbs'
 *  bar are this same button), or Space or Enter on it. */
const PRESSES: [string, (ptt: HTMLElement) => void, (ptt: HTMLElement) => void][] = [
  ['a mouse', ptt => fireEvent.pointerDown(ptt, { button: 0, pointerId: 1, pointerType: 'mouse' }), ptt => fireEvent.pointerUp(ptt, { button: 0, pointerId: 1, pointerType: 'mouse' })],
  ['a pen', ptt => fireEvent.pointerDown(ptt, { button: 0, pointerId: 2, pointerType: 'pen' }), ptt => fireEvent.pointerUp(ptt, { button: 0, pointerId: 2, pointerType: 'pen' })],
  ['a finger', ptt => fireEvent.pointerDown(ptt, { button: 0, pointerId: 3, pointerType: 'touch' }), ptt => fireEvent.pointerUp(ptt, { button: 0, pointerId: 3, pointerType: 'touch' })],
  ['Space', ptt => fireEvent.keyDown(ptt, { key: ' ', code: 'Space' }), ptt => fireEvent.keyUp(ptt, { key: ' ', code: 'Space' })],
  ['Enter', ptt => fireEvent.keyDown(ptt, { key: 'Enter', code: 'Enter' }), ptt => fireEvent.keyUp(ptt, { key: 'Enter', code: 'Enter' })],
]

it.each(PRESSES)('%s on a greyed-out PTT starts nothing, the lease lapsed or the picture blind; CONTROL: lit again, the same press holds', async (_, press, letGo) => {
  pageTimers()
  try {
    const v = view(controlling)
    fireEvent.click(screen.getByRole('button', { name: 'Start the stream' }))
    await v.live()
    const ptt = screen.getByRole<HTMLButtonElement>('button', { name: 'Hold PTT' })
    const holds = () => v.peer.channel('ptt').sent.filter(m => m.type === 'pttHold')
    // Focused while lit: greyed out, it keeps that focus, so a key still lands on it.
    ptt.focus()
    // The state stale for a round trip: no lease, so greyed out, and the press sends nothing.
    v.set({ fresh: false })
    expect(ptt.getAttribute('aria-disabled'), 'greyed out by the lapse').toBe('true')
    press(ptt)
    expect(holds(), 'a press on the PTT greyed out by the lapse').toEqual([])
    expect(ptt.getAttribute('aria-pressed')).toBe('false')
    letGo(ptt)
    v.set({ fresh: true })
    // The picture blind: greyed out, and the press sends nothing.
    act(() => { v.advance(STREAM_BLIND_MS + 250) })
    expect(v.link.getSnapshot().phase).toBe('stalled')
    expect(ptt.getAttribute('aria-disabled'), 'greyed out by the blind picture').toBe('true')
    press(ptt)
    expect(holds(), 'a press on the PTT greyed out by the blind picture').toEqual([])
    letGo(ptt)
    // CONTROL: a picture again, under the fresh lease: lit, and the same press holds.
    act(() => { frames.get(v.video)?.(0, { rtpTimestamp: 270000 }) })
    expect(ptt.getAttribute('aria-disabled')).toBe(null)
    press(ptt)
    expect(holds(), 'the press holds once the PTT is lit').toHaveLength(1)
    letGo(ptt)
    expect(last(v.peer.channel('ptt').sent)).toMatchObject({ type: 'pttRelease' })
  } finally { vi.useRealTimers() }
})

// An idle PTT the operator tabbed to keeps the keyboard's focus through a lapse (the operator's pick "Stay focusable
// while greyed", 2026-10-04). A browser blurs a focused button it disables (measured on Chrome: stale, focusout,
// disabled, then the next key on the page body), so Space did nothing after a lapse until the PTT was focused again.
// jsdom keeps the focus of a button it disables, and will not blur one; `browserFocus` moves that focus to the page,
// with its blur, as the browser does.
const browserFocus = () => {
  const at = document.activeElement
  if (!(at instanceof HTMLButtonElement) || !at.disabled) return
  document.body.tabIndex = -1; document.body.focus(); document.body.removeAttribute('tabindex')
}

it('an idle PTT keeps its focus through a lapse: greyed out, unavailable to a screen reader, and looking as a disabled button does', async () => {
  const v = view(controlling)
  fireEvent.click(screen.getByRole('button', { name: 'Start the stream' }))
  await v.live()
  const ptt = screen.getByRole<HTMLButtonElement>('button', { name: 'Hold PTT' })
  ptt.focus()
  v.set({ fresh: false })
  browserFocus()
  expect(document.activeElement, 'the PTT keeps its focus through the lapse').toBe(ptt)
  expect(ptt.getAttribute('aria-disabled'), 'greyed out, it is unavailable to a screen reader').toBe('true')
  const disabledLook = PAGE_RULES.find(rule => rule.selector === '.remote-button:disabled')!.decls
  for (const mode of THEMES) for (const { prop, value } of disabledLook) {
    expect(winnerAt(PAGE_RULES, mode, chainOf(ptt), prop)?.value, `greyed out, its ${prop} in ${mode}`).toBe(value)
  }
  v.set({ fresh: true })
  expect(ptt.getAttribute('aria-disabled'), 'lit again').toBe(null)
  expect(document.activeElement, 'and still focused').toBe(ptt)
})

it('Space or Enter on the greyed-out PTT that kept its focus starts nothing, nor does the click a key makes; CONTROL: lit again, the same Space holds', async () => {
  pageTimers()
  try {
    const v = view(controlling)
    fireEvent.click(screen.getByRole('button', { name: 'Start the stream' }))
    await v.live()
    const ptt = screen.getByRole<HTMLButtonElement>('button', { name: 'Hold PTT' })
    const holds = () => v.peer.channel('ptt').sent.filter(m => m.type === 'pttHold')
    // Where the browser sends a key: the focused element, never the PTT by name.
    const focused = () => document.activeElement as HTMLElement
    ptt.focus()
    v.set({ fresh: false })
    browserFocus()
    for (const key of [{ key: ' ', code: 'Space' }, { key: 'Enter', code: 'Enter' }]) {
      fireEvent.keyDown(focused(), key)
      fireEvent.keyUp(focused(), key)
      // A focusable button turns Space and Enter into a click; nothing on the PTT starts an over from one.
      fireEvent.click(focused())
    }
    expect(holds(), 'Space or Enter on the greyed-out PTT').toEqual([])
    expect(ptt.getAttribute('aria-pressed')).toBe('false')
    v.set({ fresh: true })
    fireEvent.keyDown(focused(), { key: ' ', code: 'Space' })
    expect(holds(), 'lit again, the same Space press holds').toHaveLength(1)
    fireEvent.keyUp(focused(), { key: ' ', code: 'Space' })
    expect(last(v.peer.channel('ptt').sent)).toMatchObject({ type: 'pttRelease' })
  } finally { vi.useRealTimers() }
})

const mouseDown = (ptt: HTMLElement) => fireEvent.pointerDown(ptt, { button: 0, pointerId: 1 })
const spaceDown = (ptt: HTMLElement) => fireEvent.keyDown(ptt, { key: ' ', code: 'Space' })
/** Every way an over ends, each after the PTT was pressed and the lease then lapsed, which leaves it lit. */
const ENDINGS: [string, (ptt: HTMLElement) => void, (v: ReturnType<typeof view>, ptt: HTMLElement) => void][] = [
  ['letting go', mouseDown, (_, ptt) => fireEvent.pointerUp(ptt, { button: 0, pointerId: 1 })],
  ['letting go of Space', spaceDown, (_, ptt) => fireEvent.keyUp(ptt, { key: ' ', code: 'Space' })],
  ['the pointer cancelled', mouseDown, (_, ptt) => fireEvent.pointerCancel(ptt, { pointerId: 1 })],
  ['its pointer capture lost', mouseDown, (_, ptt) => fireEvent.lostPointerCapture(ptt, { pointerId: 1 })],
  ['focus moved off it', spaceDown, (_, ptt) => fireEvent.blur(ptt)],
  ['the window losing focus', mouseDown, () => act(() => { window.dispatchEvent(new Event('blur')) })],
  ['the picture going blind', mouseDown, v => act(() => { v.advance(STREAM_BLIND_MS + 250) })],
  // A re-assertion each 100 ms, each reading the clock it moved to.
  ['the uplink backing up', mouseDown, v => { v.peer.channel('ptt').bufferedAmount = STREAM_UPLINK_BUDGET_BYTES + 1; for (let i = 0; i < 3; i++) act(() => { v.advance(100) }) }],
  ['End the stream', mouseDown, () => fireEvent.click(screen.getByRole('button', { name: 'End the stream' }))],
  ['station control going', mouseDown, v => v.set({ state: state('available') })],
  ['the socket going', mouseDown, v => v.set({ connected: false })],
]

it.each(ENDINGS)('a PTT held through a lapse of the lease stays lit, and still ends on %s', async (_, press, end) => {
  pageTimers()
  try {
    const v = view(controlling)
    v.env.window = window
    fireEvent.click(screen.getByRole('button', { name: 'Start the stream' }))
    await v.live()
    const ptt = screen.getByRole<HTMLButtonElement>('button', { name: 'Hold PTT' })
    const releases = () => v.peer.channel('ptt').sent.filter(m => m.type === 'pttRelease')
    ptt.focus()
    press(ptt)
    v.set({ fresh: false })
    expect(ptt.getAttribute('aria-disabled'), 'held: lit through the lapse').toBe(null)
    expect(ptt.getAttribute('aria-pressed')).toBe('true')
    expect(releases(), 'nothing let go of it').toEqual([])
    end(v, ptt)
    expect(v.link.getSnapshot().ptt, 'still held').toBe(false)
    expect(releases(), 'released once').toHaveLength(1)
    // Let go under the lapse, it is greyed out at once, as it always was (where a teardown has not taken it away).
    const after = screen.queryByRole<HTMLButtonElement>('button', { name: 'Hold PTT' })
    if (after) expect(after.getAttribute('aria-disabled'), 'let go: greyed out by the lapse').toBe('true')
  } finally { vi.useRealTimers() }
})

it('Stop TX still goes both ways for a PTT held through a lapse of the lease: the station ends the over', async () => {
  const v = view({ ...controlling, stopAvailable: true })
  fireEvent.click(screen.getByRole('button', { name: 'Start the stream' }))
  await v.live()
  const ptt = screen.getByRole<HTMLButtonElement>('button', { name: 'Hold PTT' })
  mouseDown(ptt)
  v.set({ fresh: false })
  expect(ptt.getAttribute('aria-disabled'), 'held: lit through the lapse').toBe(null)
  fireEvent.click(screen.getByRole('button', { name: 'Stop TX' }))
  expect(v.peer.channel('control').sent.filter(m => m.type === 'stopTransmit'), 'on the stream')
    .toEqual([expect.objectContaining({ stationBootId: BOOT, leaseId: LEASE, transmitEpoch: EPOCH })])
  expect(v.operations.stopTransmit, 'on the socket').toHaveBeenCalledTimes(1)
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
    ['deviceNotPinned', 'Nexus at the shack is asking you to approve this browser. Approve it there if it shows this browser’s key, below, then start the stream again.'],
    ['deviceKeyMismatch', 'This browser’s key has changed, so Nexus at the shack asks you to approve it again.'],
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

// "Outbound is a little quiet" (the operator, 2026-10-03): the level the voice goes at, beside Mic.
it('Mic level: beside Mic while it is on, read in dB, applied at once, with a meter of what is sent; CONTROL: not with the microphone off', async () => {
  const v = view(controlling, { micLevel: 'ok' })
  fireEvent.click(screen.getByRole('button', { name: 'Start the stream' }))
  await v.live()
  expect(screen.queryByRole('slider', { name: 'Mic level' }), 'a level with the microphone off').toBeNull()
  await act(async () => { fireEvent.click(screen.getByRole('button', { name: 'Mic off' })); for (let i = 0; i < 5; i++) await Promise.resolve() })
  const slider = screen.getByRole('slider', { name: 'Mic level' })
  expect(slider.getAttribute('aria-valuetext')).toBe('0 dB')
  fireEvent.change(slider, { target: { value: '6' } })
  expect(v.link.micLevel).toBe(6)
  expect(screen.getByRole('slider', { name: 'Mic level' }).getAttribute('aria-valuetext')).toBe('+6 dB')
  expect(last(v.levelGraphs[0].gains)).toBeCloseTo(micGain(6), 9)
  fireEvent.change(screen.getByRole('slider', { name: 'Mic level' }), { target: { value: '-3' } })
  expect(screen.getByRole('slider', { name: 'Mic level' }).getAttribute('aria-valuetext')).toBe('-3 dB')
  // The meter: the loudest of what the level sends, in dBFS, marked while the limiter holds it down.
  act(() => { v.levelGraphs[0].meter(0.5, false) })
  const meter = () => screen.getByRole('meter', { name: 'Microphone level sent' })
  expect(meter().getAttribute('aria-valuenow')).toBe('-6')
  expect(meter().hasAttribute('data-limited')).toBe(false)
  act(() => { v.levelGraphs[0].meter(MIC_PEAK, true) })
  expect(meter().getAttribute('data-limited')).toBe('true')
  // Off: the control goes with the microphone.
  await act(async () => { fireEvent.click(screen.getByRole('button', { name: 'Mic on' })); await Promise.resolve() })
  expect(screen.queryByRole('slider', { name: 'Mic level' })).toBeNull()
  expect(screen.queryByRole('meter', { name: 'Microphone level sent' })).toBeNull()
})

it('Mic level: a browser that cannot build the level shows none, and its microphone still goes as it is', async () => {
  const v = view(controlling, { micLevel: 'fails' })
  fireEvent.click(screen.getByRole('button', { name: 'Start the stream' }))
  await v.live()
  await act(async () => { fireEvent.click(screen.getByRole('button', { name: 'Mic off' })); for (let i = 0; i < 5; i++) await Promise.resolve() })
  expect(screen.getByRole('button', { name: 'Mic on' })).toBeTruthy()
  expect(v.peer.mic.track).toBe(v.micTracks[0])
  expect(screen.queryByRole('slider', { name: 'Mic level' })).toBeNull()
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

// Each way the browser gives no microphone is fixed in a different place, so each has its own words.
const REFUSED = {
  site: 'The microphone is blocked for this site. Allow it in the browser\'s settings for this site (the icon at the left of the address bar), then turn the microphone on again.',
  system: 'Your computer\'s privacy settings are blocking the microphone for the browser. On Windows, open Settings ▸ Privacy & security ▸ Microphone and turn on microphone access, including for desktop apps. On a Mac, open System Settings ▸ Privacy & Security ▸ Microphone, turn the browser on, and reopen it. Then turn the microphone on again.',
  dismissed: 'The browser\'s question about the microphone was closed without an answer. Turn the microphone on again and choose Allow.',
  noDevice: 'No microphone was found on this computer. Plug one in, then turn the microphone on again.',
  busy: 'The microphone was found but could not start. Another program may be using it: close that program, then turn the microphone on again.',
  other: (name: string) => `The browser could not get the microphone (${name}). Check the browser's settings for this site and the computer's microphone settings, then turn the microphone on again.`,
}

it('says why the browser gave no microphone, and where to fix it, in the notes under the picture', async () => {
  const cases = [
    [{ micRefusal: 'NotAllowedError', micPermission: 'denied' }, REFUSED.site],
    [{ micRefusal: 'NotAllowedError', micPermission: 'granted' }, REFUSED.system],
    [{ micRefusal: 'NotAllowedError', micPermission: 'prompt' }, REFUSED.dismissed],
    [{ micRefusal: 'NotFoundError', micPermission: 'granted' }, REFUSED.noDevice],
    [{ micRefusal: 'NotReadableError', micPermission: 'granted' }, REFUSED.busy],
    // Anything the page cannot place is said with the browser's own name for it.
    [{ micRefusal: 'NotAllowedError', micPermission: null }, REFUSED.other('NotAllowedError')],
    [{ micRefusal: 'AbortError', micPermission: 'granted' }, REFUSED.other('AbortError')],
  ] as const
  for (const [options, text] of cases) {
    const v = view(controlling, { microphone: 'denied', ...options })
    fireEvent.click(screen.getByRole('button', { name: 'Start the stream' }))
    await v.live()
    await act(async () => { fireEvent.click(screen.getByRole('button', { name: 'Mic off' })) })
    const notes = () => within(screen.getByRole('list', { name: 'Microphone' })).getAllByRole('alert').map(note => note.textContent)
    await waitFor(() => expect(notes(), JSON.stringify(options)).toEqual([text]))
    // The button is the operator's to press again: it says the microphone is off, and it is not held down.
    expect(screen.getByRole('button', { name: 'Mic off' }).hasAttribute('disabled')).toBe(false)
    cleanup()
  }
})

it('a PTT press with the microphone off never asks the browser for it: the station arms the over and the page says the microphone is off', async () => {
  const v = view(controlling)
  fireEvent.click(screen.getByRole('button', { name: 'Start the stream' }))
  await v.live()
  const ptt = screen.getByRole('button', { name: 'Hold PTT' })
  fireEvent.pointerDown(ptt, { button: 0 })
  expect(v.peer.channel('ptt').sent.filter(m => m.type === 'pttHold'), 'the press was not sent').toHaveLength(1)
  v.station(ARMED)
  expect(screen.getByText(NEEDED)).toBeTruthy()
  fireEvent.pointerUp(ptt)
  expect(v.micAsks, 'a PTT press asked the browser for the microphone').toEqual([])
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

// ── An idle stream (the operator's picks "15 min + prompt" and "Only clicks, keys, PTT") ───────────

const MIN = 60_000
const STILL_THERE = 'Still there? The stream ends in a minute unless you click.'
const KEEP = 'Keep streaming'
const IDLE_ENDED = 'Nobody answered “Still there?”, so the stream ended.'
const idleTimers = () => vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval', 'setTimeout', 'clearTimeout'] })
const stillThere = () => screen.queryByText(STILL_THERE)
const closes = (v: ReturnType<typeof view>) => v.signals.filter(s => s.payload.kind === 'close')
let rtp = 90000
/** The operator watching for `ms`: the page's timers and the link's clock run together, a frame
 *  arrives every second (the waterfall always moves), and `each` runs once a second beside it. */
function watching(v: ReturnType<typeof view>, ms: number, each?: () => void) {
  for (let done = 0; done < ms; done += 1000) act(() => {
    frames.get(v.video)?.(0, { rtpTimestamp: rtp += 90000 })
    each?.()
    v.advance(Math.min(1000, ms - done))
  })
}
async function streaming(initial: Partial<OperationView> = controlling, source?: MonitorSource) {
  const v = view(initial, {}, source)
  fireEvent.click(screen.getByRole('button', { name: 'Start the stream' }))
  await v.live()
  return v
}

it('IDLE: "Still there?" at 15:00 and not at 14:59; unanswered, the stream ends at 16:00 and not at 15:59, as End the stream ends it: the station is told, and control is released', async () => {
  idleTimers()
  try {
    const v = await streaming()
    watching(v, 15 * MIN - 1000)
    expect(stillThere(), '14:59').toBeNull()
    watching(v, 1000)
    expect(stillThere(), '15:00').toBeTruthy()
    expect(screen.getByRole('button', { name: KEEP })).toBeTruthy()
    watching(v, MIN - 1000)
    expect(v.link.getSnapshot().phase, '15:59: still streaming').toBe('live')
    expect(closes(v), '15:59: the station has been told nothing').toEqual([])
    expect(v.operations.release, '15:59: control is still held').not.toHaveBeenCalled()
    const control = v.peer.channel('control')
    const heartbeats = () => control.sent.filter(m => m.type === 'heartbeat').length
    watching(v, 1000)
    // 16:00, unanswered: what reaches the station is what End the stream sends - the close on the
    // socket's signalling lane and the release of control - so its presence for this browser ends,
    // and with it any transmission (S8).
    expect(closes(v), '16:00: the station is told the stream is closed').toEqual([{ leaseId: LEASE, payload: { kind: 'close' } }])
    expect(v.operations.release, '16:00: control is released').toHaveBeenCalledTimes(1)
    expect(v.link.getSnapshot().phase).toBe('idle')
    expect(v.peer.closed).toBe(true)
    const sent = heartbeats()
    watching(v, 5000)
    expect(heartbeats(), 'no heartbeat renews anything after the end').toBe(sent)
    expect(stillThere()).toBeNull()
    expect(screen.getByText(IDLE_ENDED)).toBeTruthy()
    // Starting again is the operator's to ask, and it clears the note.
    fireEvent.click(screen.getByRole('button', { name: 'Start the stream' }))
    expect(screen.queryByText(IDLE_ENDED)).toBeNull()
  } finally { vi.useRealTimers() }
})

it('IDLE: a click anywhere on the page starts the fifteen minutes again', async () => {
  idleTimers()
  try {
    const v = await streaming()
    watching(v, 10 * MIN)
    fireEvent.pointerDown(document.querySelector('header')!, { button: 0, buttons: 1 })
    watching(v, 5 * MIN + 30_000)
    expect(stillThere(), '15:30 from the start, when it would stand had the click not counted').toBeNull()
    expect(v.link.getSnapshot().phase).toBe('live')
    watching(v, 9 * MIN + 29_000)
    expect(stillThere(), 'fourteen fifty-nine after the click').toBeNull()
    watching(v, 1000)
    expect(stillThere(), 'fifteen after the click').toBeTruthy()
  } finally { vi.useRealTimers() }
})

it('IDLE: a key anywhere on the page starts the fifteen minutes again', async () => {
  idleTimers()
  try {
    const v = await streaming()
    watching(v, 10 * MIN)
    fireEvent.keyDown(document.body, { key: 'a', code: 'KeyA' })
    watching(v, 5 * MIN + 30_000)
    expect(stillThere(), '15:30 from the start, when it would stand had the key not counted').toBeNull()
    expect(v.link.getSnapshot().phase).toBe('live')
    watching(v, 9 * MIN + 29_000)
    expect(stillThere(), 'fourteen fifty-nine after the key').toBeNull()
    watching(v, 1000)
    expect(stillThere(), 'fifteen after the key').toBeTruthy()
  } finally { vi.useRealTimers() }
})

it('IDLE: a turn of the mouse wheel on the picture starts the fifteen minutes again (ruling B3: tuning with the wheel is the operator at work)', async () => {
  idleTimers()
  try {
    const v = await streaming()
    watching(v, 10 * MIN)
    v.video.dispatchEvent(new WheelEvent('wheel', { bubbles: true, cancelable: true, clientX: 800, clientY: 550, deltaY: 120 }))
    watching(v, 5 * MIN + 30_000)
    expect(stillThere(), '15:30 from the start, when it would stand had the wheel not counted').toBeNull()
    expect(v.link.getSnapshot().phase).toBe('live')
    watching(v, 9 * MIN + 29_000)
    expect(stillThere(), 'fourteen fifty-nine after the wheel').toBeNull()
    watching(v, 1000)
    expect(stillThere(), 'fifteen after the wheel').toBeTruthy()
  } finally { vi.useRealTimers() }
})

it('IDLE: a held PTT counts for as long as it is held - its re-assertions are activity - and the fifteen minutes start when it is let go', async () => {
  idleTimers()
  try {
    const v = await streaming()
    watching(v, MIN)
    const ptt = screen.getByRole('button', { name: 'Hold PTT' })
    fireEvent.pointerDown(ptt, { button: 0 })
    // Held for twenty minutes: were only the press counted, the prompt would stand at 15:00 into it.
    watching(v, 15 * MIN + 30_000)
    expect(stillThere(), '15:30 into the hold').toBeNull()
    expect(v.link.getSnapshot().phase).toBe('live')
    watching(v, 4 * MIN + 30_000)
    expect(v.peer.channel('ptt').sent.filter(m => m.type === 'pttHold').length, 'held, and re-asserted').toBeGreaterThan(10_000)
    expect(stillThere(), 'twenty minutes into the hold').toBeNull()
    fireEvent.pointerUp(ptt)
    watching(v, 15 * MIN - 1000)
    expect(stillThere(), 'fourteen fifty-nine after the release').toBeNull()
    watching(v, 1000)
    expect(stillThere(), 'fifteen after the release').toBeTruthy()
  } finally { vi.useRealTimers() }
})

it('IDLE: a key held on the picture counts the same way while held (it may be the cockpit\'s own PTT, Space)', async () => {
  idleTimers()
  try {
    const v = await streaming()
    watching(v, MIN)
    v.video.focus()
    fireEvent.keyDown(v.video, { key: ' ', code: 'Space' })
    watching(v, 15 * MIN + 30_000)
    expect(stillThere(), '15:30 into the hold').toBeNull()
    expect(v.link.getSnapshot().phase).toBe('live')
    watching(v, 4 * MIN + 30_000)
    expect(stillThere(), 'twenty minutes into the hold').toBeNull()
    fireEvent.keyUp(v.video, { key: ' ', code: 'Space' })
    watching(v, 15 * MIN - 1000)
    expect(stillThere(), 'fourteen fifty-nine after the release').toBeNull()
    watching(v, 1000)
    expect(stillThere(), 'fifteen after the release').toBeTruthy()
  } finally { vi.useRealTimers() }
})

/** A station whose rig is keyed - an FT sequence running there, say - as its monitor reports it. */
function keyedStation(): MonitorSource {
  const { source } = fixtureSource('spe')
  return { ...source, read: async signal => {
    const frame = await source.read(signal) as { station: { radio: { rigKeyed: boolean | null } } }
    frame.station.radio.rigKeyed = true
    return frame
  } }
}

it('IDLE: watching, listening and the station transmitting do not count - a running FT sequence does not hold the stream open', async () => {
  idleTimers()
  try {
    const v = await streaming(controlling, keyedStation())
    await act(async () => { for (let i = 0; i < 5; i++) await Promise.resolve() })
    expect(screen.getByText('▲ TX'), 'the station reports its rig keyed').toBeTruthy()
    // Listening: the station's audio arrives the whole time.
    const audio = v.peer.channel('audio')
    const bundle = byName(CHANNEL.audioStationToBrowser, 'receive audio bundle (the relay\'s audioRx, unchanged)')
    act(() => { v.link.audio.listen(LEASE); audio.deliver(byName(CHANNEL.audioStationToBrowser, 'audio started')) })
    let n = 0
    const listen = () => { n++; audio.deliver({ ...bundle, seq: (bundle.seq as number) + n, firstFrameMs: (bundle.firstFrameMs as number) + 60 * n }) }
    watching(v, 15 * MIN - 1000, listen)
    expect(v.link.audio.getSnapshot().phase, 'listening, with audio arriving').toBe('live')
    expect(stillThere(), '14:59').toBeNull()
    watching(v, 1000, listen)
    expect(screen.getByText('▲ TX'), 'still keyed').toBeTruthy()
    expect(stillThere(), '15:00 all the same').toBeTruthy()
    watching(v, MIN, listen)
    expect(v.operations.release, 'and the end at 16:00, keyed or not').toHaveBeenCalledTimes(1)
    expect(closes(v)).toHaveLength(1)
  } finally { vi.useRealTimers() }
})

it('IDLE: the prompt\'s own click keeps the stream and starts the fifteen minutes again, and none of it reaches Nexus', async () => {
  idleTimers()
  try {
    const v = await streaming()
    watching(v, 15 * MIN)
    const keep = screen.getByRole('button', { name: KEEP })
    const input = () => v.peer.channel('control').sent.filter(m => m.type !== 'heartbeat')
    const sent = input().length
    // A mouse's click, over the middle of the picture: down, up, click. The press alone does not take the
    // prompt down, so a click let go before the page's next look lands on it whole (one held past it: below).
    fireEvent.pointerDown(keep, { button: 0, buttons: 1, clientX: 800, clientY: 550 })
    expect(stillThere(), 'still up while pressed').toBeTruthy()
    fireEvent.pointerUp(keep, { button: 0, buttons: 0, clientX: 800, clientY: 550 })
    fireEvent.click(keep, { clientX: 800, clientY: 550 })
    expect(stillThere(), 'answered').toBeNull()
    expect(input(), 'nothing of the click reached Nexus').toHaveLength(sent)
    expect(v.peer.channel('ptt').sent.filter(m => m.type === 'held'), 'nor was anything held there').toEqual([])
    watching(v, 15 * MIN - 1000)
    expect(stillThere(), 'fifteen minutes run from the answer').toBeNull()
    expect(v.link.getSnapshot().phase).toBe('live')
    watching(v, 1000)
    expect(stillThere()).toBeTruthy()
    // Assistive technology activates a button with a click alone, and that keeps it too.
    fireEvent.click(screen.getByRole('button', { name: KEEP }))
    expect(stillThere()).toBeNull()
    watching(v, 2 * MIN)
    expect(v.operations.release).not.toHaveBeenCalled()
  } finally { vi.useRealTimers() }
})

it('IDLE: a press on the prompt held past the page\'s next look: the prompt goes from under it, and the rest of the press, on the picture, reaches nothing at the shack', async () => {
  idleTimers()
  try {
    const v = await streaming()
    watching(v, 15 * MIN)
    const input = () => v.peer.channel('control').sent.filter(m => m.type !== 'heartbeat')
    const sent = input().length
    fireEvent.pointerDown(screen.getByRole('button', { name: KEEP }), { pointerId: 1, button: 0, buttons: 1, clientX: 800, clientY: 550 })
    // The press is the answer, so the page's next look (once a second) takes the prompt down while the button is still
    // held, and the prompt's pointer capture goes with it: the browser hands the rest of the press to the picture.
    watching(v, 1000)
    expect(stillThere(), 'premise: the prompt went while its press was held').toBeNull()
    v.video.dispatchEvent(new MouseEvent('pointermove', { bubbles: true, clientX: 800, clientY: 690, button: -1, buttons: 1 }))
    act(() => { v.advance(100) })
    v.video.dispatchEvent(new MouseEvent('pointerup', { bubbles: true, clientX: 800, clientY: 690, button: 0, buttons: 0 }))
    act(() => { v.advance(100) })
    expect(input().slice(sent), 'nothing of the press reached Nexus').toEqual([])
    expect(v.peer.channel('ptt').sent.filter(m => m.type === 'held'), 'nor was anything held there').toEqual([])
    // CONTROL: the picture's own press, made after it, is sent.
    v.video.dispatchEvent(new MouseEvent('pointerdown', { bubbles: true, clientX: 800, clientY: 550, button: 0, buttons: 1 }))
    expect(input().slice(sent).map(m => `${m.type} ${m.action} ${m.buttons}`)).toEqual(['pointer down 1'])
  } finally { vi.useRealTimers() }
})

it('IDLE, a background tab: its timers throttled to one a minute, the clock still runs, and the stream ends at the first look after 16:00 - by 17:00 at the latest', async () => {
  idleTimers()
  try {
    const v = await streaming()
    // Chrome's throttling of a hidden tab: its timers run about once a minute. The link's clock runs
    // on; each minute only one second's worth of the page's timers fires.
    let at = 0
    while (v.link.getSnapshot().phase !== 'idle' && at < 30 * MIN) { v.tick(59_000); act(() => { v.advance(1000) }); at += MIN }
    expect(at, 'ended by 17:00 at the latest').toBeLessThanOrEqual(17 * MIN)
    expect(at, 'and not before 16:00').toBeGreaterThanOrEqual(16 * MIN)
    expect(v.operations.release).toHaveBeenCalledTimes(1)
    expect(closes(v)).toHaveLength(1)
  } finally { vi.useRealTimers() }
})

it('IDLE: the tab being shown again is a look of its own - with no tick at all, a stream idle past 16:00 ends then', async () => {
  idleTimers()
  try {
    const v = await streaming()
    v.tick(16 * MIN + 30_000)
    expect(v.operations.release, 'no look yet').not.toHaveBeenCalled()
    act(() => { document.dispatchEvent(new Event('visibilitychange')) })
    expect(v.operations.release).toHaveBeenCalledTimes(1)
    expect(closes(v)).toHaveLength(1)
  } finally { vi.useRealTimers() }
})

it('THE STOP LINE while "Still there?" is up: Stop TX is where it was, as enabled as it was, outside the prompt and the picture, and still reaches the station both ways', async () => {
  idleTimers()
  try {
    const v = await streaming({ ...controlling, stopAvailable: true })
    const stop = () => screen.getByRole('button', { name: 'Stop TX' }) as HTMLButtonElement
    const header = document.querySelector('header')!.innerHTML
    expect(stop().disabled).toBe(false)
    watching(v, 15 * MIN)
    expect(stillThere()).toBeTruthy()
    expect(stop().disabled, 'as enabled as before').toBe(false)
    expect(document.querySelector('header')!.innerHTML, 'the header, Stop TX in it, is untouched by the prompt').toBe(header)
    expect(stop().closest('.remote-stream-idle'), 'Stop is no part of the prompt').toBeNull()
    expect(stop().closest('.remote-stream-stage'), 'nor of the picture').toBeNull()
    fireEvent.click(stop())
    expect(last(v.peer.channel('control').sent)).toMatchObject({ type: 'stopTransmit', stationBootId: BOOT, leaseId: LEASE, transmitEpoch: EPOCH })
    expect(v.operations.stopTransmit).toHaveBeenCalledTimes(1)
  } finally { vi.useRealTimers() }
})

// ── The display note (P7: "Full desktop needs a display at the shack", as far as it is verified) ─

const STREAM_DISPLAY = 'The stream is the Nexus window as Windows draws it at the shack, so Nexus there must stay open, and not minimized.'

it('the stream\'s entry says, beside Start the stream, that Nexus at the shack must stay open and not minimized; CONTROL: not while it streams, nor with no stream to start', async () => {
  const v = view(controlling)
  const note = () => screen.queryByText(STREAM_DISPLAY)
  expect(note(), 'on the entry').toBeTruthy()
  expect(note()!.closest('.remote-stream-placeholder')?.contains(screen.getByRole('button', { name: 'Start the stream' })) ?? false, 'beside Start').toBe(true)
  fireEvent.click(screen.getByRole('button', { name: 'Start the stream' }))
  await v.live()
  expect(note(), 'streaming: the picture is the answer').toBeNull()
  cleanup()
  // Another browser has control: nothing can be started here, and there is nothing to prepare for.
  view({ state: state('occupied'), fresh: true })
  expect(screen.queryByRole('button', { name: 'Start the stream' })).toBeNull()
  expect(note(), 'with another browser in control').toBeNull()
})

// ── The Hold PTT's colours (the operator's pick "Page PTT colours", 2026-09-28), DISPLAY ONLY ─────
//
// A press on this page ARMS the station's microphone over and the voice keys it (M1), so the held
// button takes the accent, the microphone button's own "on, and not on the air" look, while the over
// waits for the voice, and the transmit colour only once the station reports the voice keyed. Its
// name, its pressed state and what a press and a release send are unchanged. Measured on the cascade
// WINNER of the page's own sheets, as it imports them (entry.tsx, then this view), in both themes.

const pageSheet = (path: string) =>
  readFileSync(resolve(process.cwd(), 'src', path), 'utf8').replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, ' '))
const PAGE_RULES = parseRules(['styles.css', 'remote-monitor/monitor.css', 'remote-web/remote.css', 'remote-web/stream.css']
  .map(pageSheet).join('\n'))
const THEMES: Mode[] = ['dark', 'light']
type Look = 'neutral' | 'accent' | 'tx'

/** What the cascade paints on `el` in `mode` against what `look` must paint, token by token. */
function paint(el: Element, mode: Mode, look: Look) {
  const chain = chainOf(el)
  const tokens = tokensAt(PAGE_RULES, mode, chain)
  const winner = (...props: string[]) => {
    const win = winnerAt(PAGE_RULES, mode, chain, ...props)
    return win ? expandWith(tokens, win.value) : null
  }
  const token = (name: string) => {
    const value = tokens.get(name)
    expect(value, `${name} is not defined in ${mode}`).toBeTruthy()
    return value!
  }
  const got = { color: winner('color'), background: winner('background', 'background-color'), border: winner('border-color') }
  const want = look === 'tx' ? { color: token('--bg'), background: token('--tx'), border: token('--tx') }
    : look === 'accent' ? { color: token('--accent'), background: token('--bg-elev'), border: token('--accent') }
    : { color: token('--text'), background: token('--bg-elev'), border: null }
  return { got, want, tokens }
}

it('the Hold PTT held is the accent while the over waits for the voice, and the transmit colour once the voice keys it', async () => {
  const v = view(controlling)
  fireEvent.click(screen.getByRole('button', { name: 'Start the stream' }))
  await v.live()
  const ptt = screen.getByRole('button', { name: 'Hold PTT' })
  const looks = (what: string, look: Look) => {
    for (const mode of THEMES) {
      const { got, want } = paint(ptt, mode, look)
      expect(got, `${what} (${mode})`).toEqual(want)
    }
  }
  looks('not held', 'neutral')
  fireEvent.pointerDown(ptt, { button: 0 })
  expect(ptt.getAttribute('aria-pressed')).toBe('true')
  looks('held, before the station reports the over', 'accent')
  v.station(ARMED)
  looks('armed, no voice yet', 'accent')
  expect(screen.getByRole('button', { name: 'Hold PTT' }), 'the name changed').toBe(ptt)
  expect(ptt.getAttribute('aria-pressed'), 'the pressed state changed').toBe('true')
  v.station(KEYED)
  looks('keyed by the voice', 'tx')
  v.station(KEYED_NO_POWER)
  looks('keyed, the rig showing no power out', 'tx')
  // The station ends the over while the button is still held (a gap in the audio): not on the air.
  v.station('mic over ended: audioGap')
  looks('held, the over ended', 'accent')
  fireEvent.pointerUp(ptt)
  expect(ptt.getAttribute('aria-pressed')).toBe('false')
  looks('let go', 'neutral')
  expect(last(v.peer.channel('ptt').sent), 'a release sent something else').toMatchObject({ type: 'pttRelease' })
})

it('CONTROL: an over the voice keyed from the cockpit through the picture, this page holding nothing, leaves the Hold PTT as it was', async () => {
  const v = view(controlling)
  fireEvent.click(screen.getByRole('button', { name: 'Start the stream' }))
  await v.live()
  const ptt = screen.getByRole('button', { name: 'Hold PTT' })
  v.station(KEYED)
  for (const mode of THEMES) {
    const { got, want } = paint(ptt, mode, 'neutral')
    expect(got, mode).toEqual(want)
  }
})

it('the armed look reads in both themes: the accent label on the button is at least 4.5:1, as the microphone button is', async () => {
  const v = view(controlling)
  fireEvent.click(screen.getByRole('button', { name: 'Start the stream' }))
  await v.live()
  const ptt = screen.getByRole('button', { name: 'Hold PTT' })
  const mic = screen.getByRole('button', { name: 'Mic off' })
  await act(async () => { fireEvent.click(mic); await Promise.resolve() })
  fireEvent.pointerDown(ptt, { button: 0 })
  v.station(ARMED)
  for (const mode of THEMES) {
    const { got, tokens } = paint(ptt, mode, 'accent')
    const on = paint(screen.getByRole('button', { name: 'Mic on' }), mode, 'accent').got
    expect({ color: got.color, border: got.border }, `the microphone's own look (${mode})`).toEqual({ color: on.color, border: on.border })
    const backdrop = toRgb(tokens.get('--bg')!, [0, 0, 0])!
    const fill = toRgb(got.background!, backdrop)!
    expect(contrast(toRgb(got.color!, fill)!, fill), `the label (${mode})`).toBeGreaterThanOrEqual(4.5)
  }
})

// ── Esc is a Stop anywhere on this page (the desktop's own rule) ───────────────────────────────────

it('Esc stops TX from wherever the keyboard is, as Stop TX does, and the picture still sends it on to Nexus at the shack', async () => {
  const v = await streaming({ ...controlling, stopAvailable: true })
  const control = v.peer.channel('control')
  const stops = () => ({ stream: control.sent.filter(m => m.type === 'stopTransmit').length, socket: v.operations.stopTransmit.mock.calls.length })
  const escape = (init: KeyboardEventInit = {}) => {
    const event = new KeyboardEvent('keydown', { key: 'Escape', code: 'Escape', bubbles: true, cancelable: true, ...init })
    act(() => { (document.activeElement ?? document.body).dispatchEvent(event) })
    return event
  }
  const places: [string, HTMLElement][] = [['nothing focused', document.body], ['Stop TX', screen.getByRole('button', { name: 'Stop TX' })],
    ['Hold PTT', screen.getByRole('button', { name: 'Hold PTT' })], ['the microphone', screen.getByRole('button', { name: 'Mic off' })],
    ['End the stream', screen.getByRole('button', { name: 'End the stream' })]]
  for (const [i, [where, place]] of places.entries()) {
    place.focus()
    const event = escape()
    expect(stops(), where).toEqual({ stream: i + 1, socket: i + 1 })
    expect(event.defaultPrevented, `${where}: Esc keeps every other meaning it has`).toBe(false)
  }
  // On the picture: a Stop here, and the key goes on to Nexus, which stops there as well.
  v.video.focus()
  escape()
  expect(stops(), 'the picture').toEqual({ stream: places.length + 1, socket: places.length + 1 })
  expect(control.sent.filter(m => m.type === 'key' && m.key === 'Escape')).toEqual([{ type: 'key', action: 'down', key: 'Escape', code: 'Escape', modifiers: 0, repeat: false }])
  // A held Esc is one press; CONTROL: any other key is no Stop.
  document.body.focus()
  escape({ repeat: true })
  fireEvent.keyDown(document.body, { key: 'q', code: 'KeyQ' })
  expect(stops()).toEqual({ stream: places.length + 1, socket: places.length + 1 })
})

it('CONTROL: where Stop TX could not be pressed (nothing could carry it), Esc sends nothing either', () => {
  const v = view({ state: state('available'), fresh: true })
  expect((screen.getByRole('button', { name: 'Stop TX' }) as HTMLButtonElement).disabled).toBe(true)
  fireEvent.keyDown(document.body, { key: 'Escape', code: 'Escape' })
  expect(v.operations.stopTransmit).not.toHaveBeenCalled()
  expect(v.peers).toHaveLength(0)
})

// ── Full screen: the whole page, Stop TX with it, and Esc still a Stop ─────────────────────────────
// jsdom has no Fullscreen API, Keyboard Lock or orientation lock, so each test is given the browser it is
// about: a desktop with Keyboard Lock (Chrome, Edge), one without (Firefox, Safari), a phone or tablet (a
// coarse pointer), and an iPhone (no full screen for a page at all).

describe('Full screen', () => {
  function browser(kind: { coarse?: boolean; keyboardLock?: 'locks' | 'refuses' | 'absent'; enabled?: boolean } = {}) {
    let element: Element | null = null
    const changed = () => document.dispatchEvent(new Event('fullscreenchange'))
    const request = vi.fn(function (this: Element) { element = this; changed(); return Promise.resolve() })
    const exit = vi.fn(() => { element = null; changed(); return Promise.resolve() })
    const keyboard = kind.keyboardLock === 'absent' ? undefined : {
      lock: vi.fn(() => kind.keyboardLock === 'refuses' ? Promise.reject(new DOMException('refused', 'NotAllowedError')) : Promise.resolve()),
      unlock: vi.fn(),
    }
    const orientation = { lock: vi.fn(() => kind.coarse ? Promise.resolve() : Promise.reject(new DOMException('not here', 'NotSupportedError'))), unlock: vi.fn() }
    Object.defineProperty(document, 'fullscreenEnabled', { configurable: true, value: kind.enabled ?? true })
    Object.defineProperty(document, 'fullscreenElement', { configurable: true, get: () => element })
    Object.defineProperty(document, 'exitFullscreen', { configurable: true, value: exit })
    Object.defineProperty(Element.prototype, 'requestFullscreen', { configurable: true, value: request })
    Object.defineProperty(navigator, 'keyboard', { configurable: true, value: keyboard })
    Object.defineProperty(window.screen, 'orientation', { configurable: true, value: orientation })
    vi.stubGlobal('matchMedia', (query: string) => ({ matches: query === '(pointer: coarse)' && !!kind.coarse }))
    return {
      request, exit, keyboard, orientation, get element() { return element },
      /** The browser leaving full screen on its own: Esc where the page cannot keep it, a back gesture, a swipe. */
      leave: () => act(() => { element = null; changed() }),
    }
  }
  afterEach(() => {
    for (const name of ['fullscreenEnabled', 'fullscreenElement', 'exitFullscreen']) delete (document as unknown as Record<string, unknown>)[name]
    delete (Element.prototype as unknown as Record<string, unknown>).requestFullscreen
    delete (navigator as unknown as Record<string, unknown>).keyboard
    delete (window.screen as unknown as Record<string, unknown>).orientation
    vi.unstubAllGlobals()
  })
  const press = (name: string) => act(async () => { fireEvent.click(screen.getByRole('button', { name })); for (let i = 0; i < 4; i++) await Promise.resolve() })
  const stops = (v: ReturnType<typeof view>) => ({
    stream: v.peer.channel('control').sent.filter(m => m.type === 'stopTransmit').length, socket: v.operations.stopTransmit.mock.calls.length,
  })

  it('Chrome and Edge: the whole page goes full screen with Esc locked to it, so Esc is still a Stop; holding Esc to leave stops again, and its own Exit adds none', async () => {
    const fs = browser({ keyboardLock: 'locks' })
    const v = await streaming({ ...controlling, stopAvailable: true })
    await press('Full screen')
    expect(fs.request.mock.contexts[0], 'the whole page, Stop TX with it: never the picture alone').toBe(document.documentElement)
    expect(fs.keyboard!.lock).toHaveBeenCalledWith(['Escape'])
    expect(fs.orientation.lock).not.toHaveBeenCalled()
    expect(screen.getByRole('button', { name: 'Stop TX' })).toBeTruthy()
    // Esc, locked to the page, reaches it and stops; the browser stays full screen.
    fireEvent.keyDown(document.body, { key: 'Escape', code: 'Escape' })
    expect(stops(v)).toEqual({ stream: 1, socket: 1 })
    // Held for two seconds, Esc leaves. Its repeats are the same press, and the exit is one Stop more: the page
    // cannot tell it from the exit of a lock that never held Esc (below).
    fireEvent.keyDown(document.body, { key: 'Escape', code: 'Escape', repeat: true })
    fs.leave()
    expect(stops(v)).toEqual({ stream: 2, socket: 2 })
    expect(fs.keyboard!.unlock).toHaveBeenCalled()
    await press('Full screen')
    await press('Exit full screen')
    expect(fs.exit).toHaveBeenCalledTimes(1)
    expect(fs.element).toBeNull()
    expect(stops(v), 'its own Exit').toEqual({ stream: 2, socket: 2 })
    expect(screen.getByRole('button', { name: 'Full screen' })).toBeTruthy()
  })

  it('a Keyboard Lock that resolves without holding Esc (WebView2 may): Esc leaves full screen unheard, and that exit still sends Stop TX; CONTROL: its own Exit sends none', async () => {
    const fs = browser({ keyboardLock: 'locks' })
    const v = await streaming({ ...controlling, stopAvailable: true })
    await press('Full screen')
    expect(fs.keyboard!.lock).toHaveBeenCalledWith(['Escape'])
    // Esc leaves at once and no key reaches the page. The unlock shows the page took the lock as held.
    fs.leave()
    expect(fs.keyboard!.unlock).toHaveBeenCalled()
    expect(stops(v)).toEqual({ stream: 1, socket: 1 })
    await press('Full screen')
    await press('Exit full screen')
    expect(fs.exit).toHaveBeenCalledTimes(1)
    expect(stops(v), 'its own Exit').toEqual({ stream: 1, socket: 1 })
  })

  it('Firefox and Safari (no Keyboard Lock), or a lock the browser refuses: any exit the page did not ask for sends Stop TX; CONTROL: its own Exit sends none', async () => {
    for (const keyboardLock of ['absent', 'refuses'] as const) {
      const fs = browser({ keyboardLock })
      const v = await streaming({ ...controlling, stopAvailable: true })
      await press('Full screen')
      expect(fs.element, keyboardLock).toBe(document.documentElement)
      fs.leave()
      expect(stops(v), keyboardLock).toEqual({ stream: 1, socket: 1 })
      await press('Full screen')
      await press('Exit full screen')
      expect(stops(v), `${keyboardLock}: its own Exit`).toEqual({ stream: 1, socket: 1 })
      cleanup()
    }
  })

  it('a phone or tablet: full screen turned to landscape, with no Esc to keep, and the back gesture leaves it with no Stop', async () => {
    const fs = browser({ coarse: true, keyboardLock: 'locks' })
    const v = await streaming({ ...controlling, stopAvailable: true })
    await press('Full screen')
    expect(fs.element).toBe(document.documentElement)
    expect(fs.orientation.lock).toHaveBeenCalledWith('landscape')
    expect(fs.keyboard!.lock, 'Chrome on Android has the call, and no Esc key to lock').not.toHaveBeenCalled()
    fs.leave()
    expect(stops(v)).toEqual({ stream: 0, socket: 0 })
    expect(fs.orientation.unlock).toHaveBeenCalled()
  })

  it('an iPhone has no full screen for a page, so there is no button; CONTROL: a browser with one has it', () => {
    browser({ enabled: false })
    view(controlling)
    expect(screen.queryByRole('button', { name: 'Full screen' })).toBeNull()
    cleanup()
    browser()
    view(controlling)
    expect(screen.getByRole('button', { name: 'Full screen' })).toBeTruthy()
  })

  it('leaving the stream page leaves the full screen it entered, as asked: no Stop', async () => {
    const fs = browser({ keyboardLock: 'absent' })
    const v = await streaming({ ...controlling, stopAvailable: true })
    await press('Full screen')
    cleanup()
    expect(fs.exit).toHaveBeenCalledTimes(1)
    expect(fs.element).toBeNull()
    expect(stops(v)).toEqual({ stream: 0, socket: 0 })
  })
})

// ── Touch: two fingers zoom and pan the picture HERE and are never sent; one finger is Nexus's mouse ──

const finger = (type: 'pointerdown' | 'pointermove' | 'pointerup' | 'pointercancel', id: number, x: number, target?: Element) => act(() => {
  ;(target ?? document.querySelector('video')!).dispatchEvent(new PointerEvent(type, {
    bubbles: true, cancelable: true, pointerId: id, pointerType: 'touch', isPrimary: id === 1, clientX: x, clientY: 550,
    button: type === 'pointermove' ? -1 : 0, buttons: type === 'pointerup' || type === 'pointercancel' ? 0 : 1,
  }))
})
async function touchStream() {
  idleTimers()
  const v = await streaming()
  // The stage laid out where the picture is (1600 x 900 at the top of the page's 100 px header), so a
  // zoom has room to work in. Its centre is (800, 550).
  v.video.parentElement!.getBoundingClientRect = () => ({ left: 0, top: 100, width: 1600, height: 900, right: 1600, bottom: 1000, x: 0, y: 100, toJSON: () => ({}) })
  // What the page asked the station for as it zoomed, recorded on the way through (and not a spy, so that
  // on a page with no zoom at all the test still reaches what was sent).
  const zooms: number[] = []
  const link = v.link as unknown as { setZoom?: (zoom: number) => void }
  const setZoom = link.setZoom?.bind(v.link)
  link.setZoom = zoom => { zooms.push(zoom); setZoom?.(zoom) }
  return {
    v, zooms,
    pointers: () => v.peer.channel('control').sent.filter(m => m.type === 'pointer'),
    held: () => v.peer.channel('ptt').sent.filter(m => m.type === 'held' && m.buttons !== 0),
  }
}
/** Two fingers 20 px apart about the picture's centre, spread to 116 px apart: 5.8 times. */
function pinch(v: ReturnType<typeof view>) {
  finger('pointerdown', 1, 790)
  v.advance(30)
  finger('pointerdown', 2, 810)
  for (let step = 1; step <= 8; step++) { finger('pointermove', 1, 790 - step * 6); finger('pointermove', 2, 810 + step * 6); v.advance(16) }
  finger('pointerup', 1, 742)
  finger('pointerup', 2, 858)
}

it('a pinch on the picture sends the shack nothing - no press, no drag - and zooms the picture here instead (it used to send two presses and a drag)', async () => {
  const { v, zooms, pointers, held } = await touchStream()
  try {
    pinch(v)
    v.advance(500)
    expect({ pointers: pointers(), held: held() }).toEqual({ pointers: [], held: [] })
    expect(v.video.style.transform).toBe('translate(0px, 0px) scale(5.8)')
    expect(last(zooms)).toBe(5.8)
    expect(screen.getByRole('button', { name: 'Fit' })).toBeTruthy()
    // CONTROL: one finger on the same picture is Nexus's mouse, and reaches the shack.
    finger('pointerdown', 3, 800)
    finger('pointerup', 3, 800)
    expect(pointers().map(m => `${m.action} ${m.pointerType}`)).toEqual(['down touch', 'up touch'])
  } finally { vi.useRealTimers() }
})

it('one finger is Nexus\'s mouse, its press held back 100 ms for a second finger: a tap sends press and release together, a longer press goes at 100 ms with its drag after it', async () => {
  const { v, pointers } = await touchStream()
  try {
    const sent = () => pointers().map(m => `${m.action} ${m.x} ${m.y} ${m.buttons} ${m.clicks} ${m.pointerType}`)
    finger('pointerdown', 1, 800)
    v.advance(60)
    expect(sent(), 'held back').toEqual([])
    finger('pointerup', 1, 800)
    expect(sent(), 'a tap: its press and release together').toEqual(['down 0.5 0.5 1 1 touch', 'up 0.5 0.5 0 1 touch'])
    // Held: the press goes at 100 ms where it was made, and the finger's travel meanwhile after it.
    v.advance(1000)
    finger('pointerdown', 1, 400)
    finger('pointermove', 1, 440)
    v.advance(99)
    expect(sent()).toHaveLength(2)
    v.advance(1)
    expect(sent().slice(2)).toEqual(['down 0.25 0.5 1 1 touch'])
    v.advance(16)
    expect(sent().slice(3)).toEqual(['move 0.275 0.5 1 0 touch'])
    finger('pointermove', 1, 480)
    finger('pointerup', 1, 480)
    expect(sent().slice(4)).toEqual(['move 0.3 0.5 1 0 touch', 'up 0.3 0.5 0 1 touch'])
    // CONTROL: a mouse is never held back.
    v.advance(1000)
    act(() => { v.video.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, cancelable: true, pointerId: 9, pointerType: 'mouse', clientX: 800, clientY: 550, button: 0, buttons: 1 })) })
    expect(sent().slice(6)).toEqual(['down 0.5 0.5 1 1 mouse'])
  } finally { vi.useRealTimers() }
})

it('a second finger after the press went ends it at the shack with a cancel, never a click, and nothing of the pinch after it is sent', async () => {
  const { v, pointers, held } = await touchStream()
  try {
    finger('pointerdown', 1, 800)
    v.advance(150)
    expect(pointers().map(m => m.action)).toEqual(['down'])
    finger('pointerdown', 2, 820)
    expect(pointers().map(m => m.action)).toEqual(['down', 'cancel'])
    const heldThen = held().length
    for (let step = 1; step <= 4; step++) { finger('pointermove', 1, 800 - step * 10); finger('pointermove', 2, 820 + step * 10); v.advance(16) }
    finger('pointerup', 2, 860)
    finger('pointermove', 1, 700)
    finger('pointerup', 1, 700)
    v.advance(500)
    expect(pointers().map(m => m.action)).toEqual(['down', 'cancel'])
    expect(held(), 'and nothing is held there after it').toHaveLength(heldThen)
  } finally { vi.useRealTimers() }
})

it('Fit shows the whole picture again and asks the station for the stage\'s own size; a stream that ends forgets its zoom', async () => {
  const { v, zooms } = await touchStream()
  try {
    pinch(v)
    act(() => { fireEvent.click(screen.getByRole('button', { name: 'Fit' })) })
    expect(v.video.style.transform).toBe('')
    expect(last(zooms)).toBe(1)
    expect(screen.queryByRole('button', { name: 'Fit' })).toBeNull()
    pinch(v)
    expect(v.video.style.transform).not.toBe('')
    act(() => { fireEvent.click(screen.getByRole('button', { name: 'End the stream' })) })
    expect(v.video.style.transform).toBe('')
    expect(last(zooms)).toBe(1)
  } finally { vi.useRealTimers() }
})

it('the stage takes every touch for itself, so the browser neither pans nor zooms the page under the picture; CONTROL: the header is the browser\'s as before', () => {
  view(controlling)
  for (const mode of THEMES) {
    expect(winnerAt(PAGE_RULES, mode, chainOf(document.querySelector('.remote-stream-stage')!), 'touch-action')?.value, mode).toBe('none')
    expect(winnerAt(PAGE_RULES, mode, chainOf(document.querySelector('header')!), 'touch-action'), mode).toBeNull()
  }
})
