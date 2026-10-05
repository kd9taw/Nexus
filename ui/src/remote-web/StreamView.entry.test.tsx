// @vitest-environment jsdom
// One clean way into streaming (the operator, 2026-10-02: "really confusing on both the site and the
// app as far as getting things connected and people needing to press a lot of buttons"). The station
// card's Stream or Listen IS the operator asking, so the page it opens starts once on its own; every
// state the page can be in says one plain sentence and the one next step; and Listen is the station's
// receive audio with no picture, under the same lease and the same Stop. Nothing here changes what the
// station checks: the offer still goes only under the lease this session holds, once per ask.
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react'
import { StreamView } from './StreamView'
import type { HostedConnection } from './client'
import type { OperationState } from './operation-protocol'
import type { OperationView } from './operation-client'
import type { AudioView } from './audio-listen'
import { ANSWER, LEASE, SIGNAL, answerChecked, byName, harness } from './stream-link.testkit'

const BOOT = '0f7d1c2e-5b3a-4c1d-9e8f-7a6b5c4d3e2f'
const EPOCH = '000000000000002b'
const KEY = 'a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f90'
// The key as both ends show it: the first 128 bits of the fingerprint, eight groups of four (S3-L1).
const SHORT = 'A1B2 C3D4 E5F6 0718 293A 4B5C 6D7E 8F90'
const BETA = 'Remote streaming is a beta feature. Access could be revoked at any time.'
const OFFLINE = "The station isn't online. Check that Nexus is running at the shack with Remote turned on, and that the computer is awake."
const OCCUPIED = 'Another browser is using this station. You can start once it lets go.'
const DISABLED = 'Streaming is off at the shack. In Nexus there, turn on “Stream this station from my browser” (Settings → Station → Remote access), then start the stream again.'
const IN_USE = 'Another browser is streaming this station. You can stream once it ends.'
const NOT_PINNED = 'Nexus at the shack is asking you to approve this browser. Approve it there if it shows this browser’s key, below, then start the stream again.'
const STATION_KEY_CHANGED = 'This station’s key has changed, so nothing was connected. Return to your stations to compare the new key with “This station’s key” in Nexus at the shack.'
const LISTEN_READY = "Ready. Press Listen to hear the station's receive audio."
const NO_AUDIO = "This station isn't sending its receive audio. Check that Nexus at the shack is up to date and has the radio's audio input set."

function state(phase: OperationState['phase'], extra: Partial<OperationState> = {}): OperationState {
  return { stationBootId: BOOT, allowed: phase !== 'localPermissionRequired', phase, leaseId: phase === 'controlling' ? LEASE : null, revision: 1,
    commandWindowId: null, nextSequence: null, leaseRemainingMs: phase === 'controlling' ? 5000 : null, actions: [],
    txArmed: false, transmitEpoch: phase === 'controlling' ? EPOCH : null,
    controls: { context: {} as never, capabilities: phase === 'localPermissionRequired' ? [] : ['audioListen'] }, ...extra } as OperationState
}

const frames = new Map<HTMLVideoElement, (now: number, metadata: { rtpTimestamp?: number }) => void>()
beforeEach(() => {
  Object.defineProperty(HTMLVideoElement.prototype, 'requestVideoFrameCallback', { configurable: true,
    value(this: HTMLVideoElement, callback: (now: number, metadata: { rtpTimestamp?: number }) => void) { frames.set(this, callback); return 1 } })
  Object.defineProperty(HTMLVideoElement.prototype, 'cancelVideoFrameCallback', { configurable: true, value(this: HTMLVideoElement) { frames.delete(this) } })
})
afterEach(() => {
  cleanup(); frames.clear(); vi.useRealTimers()
  delete (HTMLVideoElement.prototype as { requestVideoFrameCallback?: unknown }).requestVideoFrameCallback
  delete (HTMLVideoElement.prototype as { cancelVideoFrameCallback?: unknown }).cancelVideoFrameCallback
  delete (navigator as { userActivation?: unknown }).userActivation
})

/** The relay's audio lane as the page sees it: a view, and listen/release that say what they were asked. */
function relayAudio() {
  let view: AudioView = { phase: 'off', reason: null, supported: true }
  const listeners = new Set<() => void>()
  const set = (next: Partial<AudioView>) => { view = { ...view, ...next }; for (const f of listeners) f() }
  const listened: string[] = [], released: (string | null)[] = []
  return { listened, released, set,
    subscribe: (f: () => void) => { listeners.add(f); return () => { listeners.delete(f) } },
    getSnapshot: () => view,
    listen: (lease: string) => { listened.push(lease); set({ phase: 'connecting', reason: null }) },
    release: (reason: string | null = null) => { released.push(reason); set({ phase: reason ? 'ended' : 'off', reason }) },
  }
}

function entry(initial: Partial<OperationView>, props: { autostart?: boolean; mode?: 'stream' | 'listen'; browserKey?: string | null } = {},
  link: Parameters<typeof harness>[0] = {}) {
  const h = harness(link)
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
  const audio = relayAudio()
  const connection = { operations, stream: h.link, audio, source: { id: 'fake', kind: 'native', read: () => Promise.reject(Error('none')) } } as unknown as HostedConnection
  const utils = render(<StreamView connection={connection} station="Home" disconnect={() => {}} signOut={() => {}} {...props} />)
  return {
    h, operations, audio, utils,
    set: (next: Partial<OperationView>) => act(() => { snapshot = { ...snapshot, ...next }; for (const f of listeners) f() }),
    status: () => document.querySelector('.remote-stream-placeholder p[role="status"]')?.textContent ?? null,
    settle: () => act(async () => { await Promise.resolve(); await Promise.resolve() }),
  }
}

it('opened from the station card, the stream starts on its own, once: one acquire, one offer under its lease, and an ended stream stays ended', async () => {
  const v = entry({ fresh: true, state: state('available') }, { autostart: true })
  await v.settle()
  expect(v.operations.acquire, 'the card\'s Stream was the ask: no second press').toHaveBeenCalledTimes(1)
  expect(v.h.signals, 'A3: nothing is offered before the lease is held').toHaveLength(0)
  v.set({ state: state('controlling') })
  await v.settle()
  expect(v.h.signals.filter(s => s.payload.kind === 'offer'), 'the offer goes under the lease this session holds').toEqual([
    expect.objectContaining({ leaseId: LEASE })])
  await act(async () => { v.h.link.receive({ type: 'streamSignal', payload: { kind: 'answer', sdp: ANSWER } }); await answerChecked() })
  act(() => { v.h.link.receive(byName(SIGNAL.roomToBrowser, 'refused: streamClosed')) })
  await v.settle()
  // The ask was consumed by the start: re-reads, a fresh state, time passing - none of them re-offer.
  v.set({ state: state('controlling', { revision: 2 }) })
  v.set({ fresh: false }); v.set({ fresh: true })
  await v.settle()
  expect(v.h.peers, 'A3: never re-offered on its own').toHaveLength(1)
  expect(v.operations.acquire).toHaveBeenCalledTimes(1)
  // Positive control: the page's own Start asks again, and that does start a new one.
  fireEvent.click(screen.getByRole('button', { name: 'Start the stream' }))
  await v.settle()
  expect(v.h.peers).toHaveLength(2)
})

it('without the card\'s ask (a view opened any other way) nothing is acquired or offered until Start is pressed', async () => {
  const v = entry({ fresh: true, state: state('available') })
  await v.settle()
  v.set({ state: state('controlling') })
  await v.settle()
  expect(v.operations.acquire).not.toHaveBeenCalled()
  expect(v.h.signals).toHaveLength(0)
  expect(screen.getByRole('button', { name: 'Start the stream' })).toBeTruthy()
})

it('the card\'s ask waits for a current reading of the station, and is spent by a refusal: another browser in control is said, and nothing starts later on its own', async () => {
  const v = entry({ fresh: false, state: null }, { autostart: true })
  await v.settle()
  expect(v.operations.acquire, 'no reading yet: nothing is asked of a station the page has not heard').not.toHaveBeenCalled()
  v.set({ fresh: true, state: state('occupied') })
  await v.settle()
  expect(v.status()).toBe(OCCUPIED)
  expect(v.operations.acquire).not.toHaveBeenCalled()
  // It lets go: the page now offers Start, and does not take the station on its own.
  v.set({ state: state('available') })
  await v.settle()
  expect(v.operations.acquire, 'a spent ask never takes the station when it frees up').not.toHaveBeenCalled()
  expect(screen.getByRole('button', { name: 'Start the stream' })).toBeTruthy()
})

it('the station away: after eight seconds without a session the page says it is not online and what to check, and the sentence goes once the session lands', async () => {
  vi.useFakeTimers()
  const v = entry({ connected: false }, { autostart: true })
  expect(v.status()).toBe('Connecting to the station…')
  await act(async () => { await vi.advanceTimersByTimeAsync(7900) })
  expect(v.status(), 'a slow link is not called offline').toBe('Connecting to the station…')
  await act(async () => { await vi.advanceTimersByTimeAsync(200) })
  expect(v.status()).toBe(OFFLINE)
  v.set({ connected: true })
  expect(v.status()).toBe('Waiting for the station…')
  // And the next drop starts its own count from nothing.
  v.set({ connected: false })
  expect(v.status()).toBe('Connecting to the station…')
})

it('streaming off at the shack, and a stream another browser holds, each say so in one sentence with the next step, and Start stays beside them', async () => {
  for (const [reason, words] of [['streamDisabled', DISABLED], ['streamInUse', IN_USE]] as const) {
    const v = entry({ fresh: true, state: state('controlling') }, { autostart: true })
    await v.settle()
    expect(v.h.signals.filter(s => s.payload.kind === 'offer'), reason).toHaveLength(1)
    act(() => { v.h.link.receive(byName(SIGNAL.roomToBrowser, `refused: ${reason}`)) })
    await v.settle()
    expect(v.status(), reason).toBe(words)
    expect(screen.getByRole('button', { name: 'Start the stream' }), reason).toBeTruthy()
    cleanup()
  }
})

it('a browser the shack has not approved to stream: the page says Nexus there is asking, and shows the key to compare with the one it shows', async () => {
  const v = entry({ fresh: true, state: state('controlling') }, { autostart: true, browserKey: KEY })
  await v.settle()
  act(() => { v.h.link.receive(byName(SIGNAL.roomToBrowser, 'refused: deviceNotPinned')) })
  await v.settle()
  expect(v.status()).toBe(NOT_PINNED)
  expect(screen.getByText(`This browser’s key: ${SHORT}`)).toBeTruthy()
  // Control: a stream the station ended for another reason shows no key.
  cleanup()
  const w = entry({ fresh: true, state: state('controlling') }, { autostart: true, browserKey: KEY })
  await w.settle()
  act(() => { w.h.link.receive(byName(SIGNAL.roomToBrowser, 'refused: streamClosed')) })
  await w.settle()
  expect(screen.queryByText(`This browser’s key: ${SHORT}`)).toBeNull()
})

it('S3-L1: a stream refused because the service lists another key for the station connects nothing and sends the operator to the station card', async () => {
  const v = entry({ fresh: true, state: state('controlling') }, { autostart: true, browserKey: KEY }, { verify: async () => 'stationKeyChanged' })
  await v.settle()
  await act(async () => { v.h.link.receive({ type: 'streamSignal', payload: { kind: 'answer', sdp: ANSWER } }); await answerChecked() })
  await v.settle()
  expect(v.h.peer.remote, 'nothing reaches the browser').toBeNull()
  expect(v.status()).toBe(STATION_KEY_CHANGED)
  // This browser's key did not change, so it is not the one put up to compare.
  expect(screen.queryByText(`This browser’s key: ${SHORT}`)).toBeNull()
  // Control: the station's own answer is taken, and nothing of the sort is said.
  cleanup()
  const w = entry({ fresh: true, state: state('controlling') }, { autostart: true, browserKey: KEY })
  await w.settle()
  await act(async () => { w.h.link.receive({ type: 'streamSignal', payload: { kind: 'answer', sdp: ANSWER } }); await answerChecked() })
  expect(w.h.peer.remote).toEqual({ type: 'answer', sdp: ANSWER })
  expect(document.body.textContent).not.toContain('key has changed')
})

it('the beta line is on the stream page the whole time: before the stream, and while it is live', async () => {
  const v = entry({ fresh: true, state: state('controlling') }, { autostart: true })
  const line = () => document.querySelector('.remote-stream-header .remote-beta')
  expect(line()?.textContent).toBe(`Beta ${BETA}`)
  await v.settle()
  await act(async () => { v.h.link.receive({ type: 'streamSignal', payload: { kind: 'answer', sdp: ANSWER } }); await answerChecked() })
  act(() => {
    v.h.peer.ontrack?.({ track: 'track', streams: ['media'] })
    for (const label of ['control', 'ptt', 'audio']) v.h.peer.channel(label).open()
    frames.get(v.utils.container.querySelector('video')!)?.(0, { rtpTimestamp: 90000 })
  })
  await v.settle()
  expect(v.h.link.getSnapshot().phase).toBe('live')
  expect(line()?.textContent, 'still there while live').toBe(`Beta ${BETA}`)
})

it('Listen: the station\'s receive audio under the lease, no picture, Stop TX on screen; the card\'s press starts it once', async () => {
  const v = entry({ fresh: true, state: state('available'), stopAvailable: true }, { autostart: true, mode: 'listen' })
  await v.settle()
  expect(v.utils.container.querySelector('video'), 'audio only: no picture, and nothing to click through').toBeNull()
  expect(screen.getByRole('button', { name: 'Stop TX' }), 'THE STOP LINE: Stop TX whenever the view is').toBeTruthy()
  expect(v.operations.acquire).toHaveBeenCalledTimes(1)
  expect(v.audio.listened).toEqual([])
  v.set({ state: state('controlling') })
  await v.settle()
  expect(v.audio.listened, 'once, under the lease the station granted').toEqual([LEASE])
  expect(v.h.signals, 'Listen never offers a stream').toHaveLength(0)
  act(() => v.audio.set({ phase: 'live' }))
  expect(v.status()).toBe('Listening to the station')
  fireEvent.click(screen.getByRole('button', { name: 'Stop listening' }))
  expect(v.audio.released).toEqual([null])
  // Stopped, it stays stopped: Listen starts it again, inside that press.
  await v.settle()
  expect(v.audio.listened).toHaveLength(1)
  fireEvent.click(screen.getByRole('button', { name: 'Listen' }))
  expect(v.audio.listened).toEqual([LEASE, LEASE])
})

it('Listen: sound a press did not start just now waits for a press (Safari refuses it otherwise), and control lost ends the sound', async () => {
  Object.defineProperty(navigator, 'userActivation', { configurable: true, value: { isActive: false, hasBeenActive: true } })
  const v = entry({ fresh: true, state: state('available') }, { autostart: true, mode: 'listen' })
  await v.settle()
  v.set({ state: state('controlling') })
  await v.settle()
  expect(v.audio.listened, 'the activation had lapsed: no sound started behind the operator\'s back').toEqual([])
  expect(v.status()).toBe(LISTEN_READY)
  fireEvent.click(screen.getByRole('button', { name: 'Listen' }))
  expect(v.audio.listened).toEqual([LEASE])
  act(() => v.audio.set({ phase: 'live' }))
  // Control goes (taken over, or the lease lapsed): the sound stops here at once.
  v.set({ state: state('available') })
  expect(v.audio.released).toEqual(['notController'])
})

it('Listen: a station that sends no receive audio says so instead of offering a Listen that cannot work', async () => {
  const v = entry({ fresh: true, state: state('available', { controls: { context: {} as never, capabilities: [] } }) }, { mode: 'listen' })
  await v.settle()
  expect(v.status()).toBe(NO_AUDIO)
  expect(screen.queryByRole('button', { name: 'Listen' })).toBeNull()
  // Control off for this browser is the reason given, not "no audio" (the station lists no capability then).
  cleanup()
  entry({ fresh: true, state: state('localPermissionRequired') }, { mode: 'listen' })
  await act(async () => { await Promise.resolve() })
  expect(document.querySelector('.remote-stream-placeholder p[role="status"]')?.textContent).toMatch(/^Station control is off for this browser/)
})
