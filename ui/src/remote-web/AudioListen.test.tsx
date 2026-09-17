// @vitest-environment jsdom
import { afterEach, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react'
import { AudioListen } from './AudioListen'
import { AudioLink } from './audio-listen'
import type { AudioEnvironment } from './audio-listen'
import { OperationClient } from './operation-client'
import { pendingControlStorage } from './control-storage'
import type { ControlCapability } from './station-operation'
import type { OperationState } from './operation-protocol'

const clients: OperationClient[] = []
afterEach(() => { cleanup(); clients.splice(0).forEach(client => client.disconnected()); vi.useRealTimers() })

function environment(supported = true): AudioEnvironment {
  return {
    // The fake timers mock Date, so advancing them advances this too - which is what
    // makes the stall case below a test of elapsed time rather than of a counter.
    now: () => Date.now(),
    decoderAvailable: () => supported,
    context: () => Promise.resolve({ sampleRate: 48000, push: () => {}, reset: () => {}, close: () => Promise.resolve() }),
    decoder: () => ({ configure: () => {}, decode: () => {}, close: () => {} }),
    document: { visibilityState: 'visible', addEventListener: () => {}, removeEventListener: () => {} },
  }
}

function fixture(options: { capabilities?: ControlCapability[]; supported?: boolean; phase?: 'controlling' | 'available' } = {}) {
  vi.useFakeTimers()
  const sent: { request: { type: string; requestId: string } }[] = []
  const values = new Map<string, string>()
  const storage = { getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => { values.set(key, value) }, removeItem: (key: string) => { values.delete(key) } }
  let now = 1000
  const client = new OperationClient(wire => sent.push(JSON.parse(wire)), true, () => now, undefined,
    4, pendingControlStorage(() => storage, 'audio-test', async (_key, run) => run()))
  clients.push(client)
  const phase = options.phase ?? 'controlling'
  const state: OperationState = {
    stationBootId: crypto.randomUUID(), allowed: true, phase,
    leaseId: phase === 'controlling' ? crypto.randomUUID() : null,
    revision: 1, commandWindowId: phase === 'controlling' ? crypto.randomUUID() : null,
    nextSequence: phase === 'controlling' ? 1 : null, leaseRemainingMs: phase === 'controlling' ? 5000 : null,
    actions: [], txArmed: false,
    controls: { context: { radioId: 1, radioConnection: 1, ampConnection: 1, ampReadSequence: 1 },
      capabilities: options.capabilities ?? ['audioListen'] },
  }
  client.open()
  client.receive({ type: 'operationResponse', requestId: sent[sent.length - 1]!.request.requestId, value: state })
  const messages: Record<string, unknown>[] = []
  const link = new AudioLink(message => messages.push(message as Record<string, unknown>), environment(options.supported ?? true))
  const rendered = render(<AudioListen audio={link} client={client} />)
  const reply = (value: unknown) =>
    client.receive({ type: 'operationResponse', requestId: sent[sent.length - 1]!.request.requestId, value })
  const refuse = (error: string) =>
    client.receive({ type: 'operationResponse', requestId: sent[sent.length - 1]!.request.requestId, error })
  const advance = (ms: number) => { now += ms }
  return { ...rendered, link, client, messages, state, reply, refuse, advance, sent }
}
const listen = () => screen.getByRole('button', { name: 'Listen' })

it('is muted on open and offers nothing until the operator asks', () => {
  const h = fixture()
  expect(listen()).toBeTruthy()
  expect(h.messages).toEqual([])
  expect(screen.queryByRole('status')).toBeNull()
})

it('starts and stops on the control, and says which it is', async () => {
  const h = fixture()
  await act(async () => { fireEvent.click(listen()) })
  expect(h.messages[0]).toMatchObject({ type: 'audioListen', listening: true, leaseId: h.state.leaseId })
  expect(screen.getByRole('status').textContent).toBe('Connecting audio')
  await act(async () => { h.link.receive(bundle(0)) })
  expect(screen.getByRole('status').textContent).toBe('Listening to the station')
  await act(async () => { fireEvent.click(screen.getByRole('button', { name: 'Stop listening' })) })
  expect(h.messages[h.messages.length - 1]).toMatchObject({ type: 'audioListen', listening: false })
  expect(screen.queryByRole('status')).toBeNull()
})

it('shows a gap and a stall as different things, because they need different answers', async () => {
  const h = fixture()
  await act(async () => { fireEvent.click(listen()) })
  await act(async () => { h.link.receive(bundle(0)) })
  await act(async () => { h.link.receive(bundle(30)) })
  expect(screen.getByRole('status').textContent).toBe('Audio gap - the link is losing packets')
  await act(async () => { vi.advanceTimersByTime(3000) })
  expect(screen.getByRole('status').textContent).toBe('Audio stalled - the link, not the band')
})

it('renders nothing at all on a station that never advertised the lane', () => {
  const h = fixture({ capabilities: ['amplifier'] })
  expect(screen.queryByRole('button', { name: 'Listen' })).toBeNull()
  expect(h.messages).toEqual([])
  // Control: the identical fixture WITH the hint does render the control, so the absence
  // above is the hint and not a component that never renders.
  cleanup()
  fixture({ capabilities: ['audioListen'] })
  expect(screen.getByRole('button', { name: 'Listen' })).toBeTruthy()
})

it('says so rather than failing silently when the browser has no decoder', () => {
  fixture({ supported: false })
  expect(screen.queryByRole('button', { name: 'Listen' })).toBeNull()
  expect(screen.getByRole('note').textContent)
    .toBe('This browser cannot play station audio. Chrome, Edge, Firefox on a computer, or Safari 26 can.')
})

it('cannot be started without station control', () => {
  fixture({ phase: 'available' })
  expect(listen().hasAttribute('disabled')).toBe(true)
})

it('stops the sound itself the moment station control is lost', async () => {
  const h = fixture()
  await act(async () => { fireEvent.click(listen()) })
  await act(async () => { h.link.receive(bundle(0)) })
  expect(screen.getByRole('status').textContent).toBe('Listening to the station')
  // The lease goes. The station stops feeding on its own re-check within a second; this
  // is the half that stops the SOUND at once, which is the half the operator hears - and
  // it also tells the station, so a shack's upload is not spent on audio nobody wants.
  await act(async () => { h.client.disconnected() })
  expect(h.link.getSnapshot()).toMatchObject({ phase: 'ended', reason: 'notController' })
  expect(h.messages[h.messages.length - 1]).toMatchObject({ type: 'audioListen', listening: false })
})

// The station answers a heartbeat `stationBusy` while an operation of ITS OWN is in flight - it is
// contention, not a refusal, and the station keeps feeding audio through it on purpose (its audio
// lane tolerates a busy authority). The browser used to undo that: any error reply nulled the lease,
// and this component then released the audio saying control was lost.
it('keeps listening through a heartbeat the station was merely too busy to answer', async () => {
  for (const busy of ['stationBusy', 'remoteBusy']) {
    const h = fixture()
    await act(async () => { fireEvent.click(listen()) })
    await act(async () => { h.link.receive(bundle(0)) })
    h.advance(1000)
    await act(async () => { vi.advanceTimersByTime(250) })
    expect(h.sent[h.sent.length - 1]!.request.type).toBe('heartbeat')
    const before = h.messages.length
    await act(async () => { h.refuse(busy) })
    expect(screen.getByRole('status').textContent).toBe('Listening to the station')
    expect(h.messages).toHaveLength(before)
    expect(h.client.getSnapshot().state?.phase).toBe('controlling')
    // And the NEXT heartbeat still carries the lease: control was kept, not merely displayed.
    h.advance(1000)
    await act(async () => { vi.advanceTimersByTime(250) })
    expect(h.sent[h.sent.length - 1]!.request).toMatchObject({ type: 'heartbeat', leaseId: h.state.leaseId })
    cleanup()
  }
  // Positive control: a refusal that IS about this browser's control still stops the sound at once.
  const h = fixture()
  await act(async () => { fireEvent.click(listen()) })
  await act(async () => { h.link.receive(bundle(0)) })
  h.advance(1000)
  await act(async () => { vi.advanceTimersByTime(250) })
  await act(async () => { h.refuse('leaseExpired') })
  expect(h.link.getSnapshot()).toMatchObject({ phase: 'ended', reason: 'notController' })
  expect(h.messages[h.messages.length - 1]).toMatchObject({ type: 'audioListen', listening: false })
})

it("shows the station's own word for why it stopped, and offers a start again", async () => {
  const h = fixture()
  await act(async () => { fireEvent.click(listen()) })
  await act(async () => { h.link.receive(bundle(0)) })
  await act(async () => { h.link.receive({ type: 'audioState', listening: false, reason: 'audioInUse' }) })
  expect(screen.getByRole('alert').textContent).toBe('Another browser is listening to this station.')
  // And the control goes back to offering a start rather than sitting on "stop": an
  // operator told only that it ended has nothing to press.
  expect(screen.getByRole('button', { name: 'Listen' })).toBeTruthy()
  expect(screen.queryByRole('status')).toBeNull()
})

function bundle(seq: number) {
  const bytes: number[] = []
  for (let i = 0; i < 3; i++) bytes.push(0, 2, 0x10 + i, 0x20 + i)
  return { type: 'audioRx', seq, epoch: '0000000000000001', firstFrameMs: seq * 20, frameMs: 20, count: 3,
    payload: btoa(String.fromCharCode(...bytes)) }
}
