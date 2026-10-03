// @vitest-environment jsdom
// "Start the stream" against the REAL operation client, which polls the station once a second, and
// the real page. Operator-found on the release candidate, 2026-10-02: a press went to "Waiting for
// the station…" for a moment and then back to the Start button, saying nothing. That is the page's
// handling of an acquire the station (or the relay) REFUSED: the refusal clears the held state, so
// the button goes; the next read restores it, and the refusal's reason was never shown.
import { afterEach, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react'
import { StreamView } from './StreamView'
import { OperationClient } from './operation-client'
import type { HostedConnection } from './client'
import type { OperationState } from './operation-protocol'
import { LEASE, harness } from './stream-link.testkit'

const BOOT = '0f7d1c2e-5b3a-4c1d-9e8f-7a6b5c4d3e2f'
const READY = 'Ready. Start the stream to see and operate Nexus at the shack.'
const WAITING = 'Waiting for the station…'
const STARTING = 'Starting the stream…'
const REFUSED = "The station couldn't start the stream just now. Start the stream again."
const PERMISSION = 'Station control is off for this browser. Allow it in Nexus at the shack (Settings → Station → Remote access → Advanced), then try again.'
const OCCUPIED = 'Another browser is using this station. You can start once it lets go.'

const clients: OperationClient[] = []
afterEach(() => { cleanup(); clients.splice(0).forEach(c => c.disconnected()); vi.useRealTimers() })

function fixture() {
  vi.useFakeTimers()
  let now = 1000
  const sent: { request: { type: string; requestId: string; stationBootId?: string } }[] = []
  const client = new OperationClient(wire => sent.push(JSON.parse(wire)), true, () => now, undefined, 4)
  clients.push(client)
  const h = harness()
  let revision = 1
  const available = (): OperationState => ({ stationBootId: BOOT, allowed: true, phase: 'available', leaseId: null, revision: revision++,
    commandWindowId: null, nextSequence: null, leaseRemainingMs: null, actions: [], txArmed: false })
  const controlling = (): OperationState => ({ ...available(), phase: 'controlling', leaseId: LEASE,
    commandWindowId: '5a1c9e2b-7d3f-4e8a-9b6c-1d2e3f4a5b6c', nextSequence: 1, leaseRemainingMs: 5000 })
  const last = () => sent[sent.length - 1]!.request
  /** The station's (or the relay's) word on the request in flight. */
  const answer = (body: { value: unknown } | { error: string }) => act(() => client.receive({ type: 'operationResponse', requestId: last().requestId, ...body }))
  client.open()
  answer({ value: available() })
  const connection = { operations: client, stream: h.link, source: { id: 'fake', kind: 'native', read: () => Promise.reject(Error('none')) } } as unknown as HostedConnection
  render(<StreamView connection={connection} station="Home" disconnect={() => {}} signOut={() => {}} />)
  /** Time passes as it does in a browser: the client's own 250 ms tick runs on the way. */
  const advance = async (ms: number) => {
    let left = ms
    do { const step = Math.min(250, left); now += step; h.tick(step); await act(async () => { await vi.advanceTimersByTimeAsync(step) }); left -= step } while (left > 0)
  }
  /** The placeholder's one line on where things stand (it drops its status role while a stream runs). */
  const status = () => document.querySelector('.remote-stream-placeholder p:not([role="note"])')?.textContent
  const start = () => screen.queryByRole('button', { name: 'Start the stream' }) as HTMLButtonElement | null
  const acquires = () => sent.filter(s => s.request.type === 'acquire')
  return { client, sent, h, available, controlling, last, answer, advance, status, start, acquires }
}

it('a Start pressed while the automatic read is in flight starts the stream once the read settles: one acquire, then the offer under its lease', async () => {
  const f = fixture()
  await f.advance(1000)
  expect(f.last().type, 'scene: the once-a-second read is out, unanswered').toBe('state')
  expect(f.client.getSnapshot().busy, 'scene: the read really is in flight').toBe(true)
  // Twice, as two clicks arrive: each its own event, the page re-rendered between them.
  fireEvent.click(f.start()!)
  expect(f.status(), 'the press is taken, not swallowed').toBe(STARTING)
  fireEvent.click(f.start()!)
  await f.advance(0)
  expect(f.acquires(), 'nothing leaves while the read is in flight').toHaveLength(0)
  f.answer({ value: f.available() })
  await f.advance(0)
  expect(f.acquires(), 'the press was not lost: exactly one acquire, sent after the read settled').toHaveLength(1)
  expect(f.acquires()[0]!.request.stationBootId, 'naming the station the read just described').toBe(BOOT)
  f.answer({ value: f.controlling() })
  await f.advance(0)
  expect(f.h.signals, 'the stream is offered, once, under the lease the station granted').toEqual([
    expect.objectContaining({ leaseId: LEASE, payload: expect.objectContaining({ kind: 'offer' }) })])
})

it('THE OPERATOR\'S REPORT: a start the station refuses says why, in plain words, instead of going back to Ready', async () => {
  const f = fixture()
  await f.advance(500)
  expect(f.status(), 'scene: ready, no read in flight').toBe(READY)
  fireEvent.click(f.start()!)
  expect(f.last().type).toBe('acquire')
  expect(f.status()).toBe(STARTING)
  f.answer({ error: 'stationBusy' })
  await f.advance(0)
  expect(f.status(), 'refused: said at once, not "Waiting for the station…"').toBe(REFUSED)
  await f.advance(500)
  expect(f.last().type, 'the next once-a-second read').toBe('state')
  f.answer({ value: f.available() })
  await f.advance(0)
  expect(f.status(), 'and still said once the station is ready again, beside the button').toBe(REFUSED)
  expect(f.start()?.disabled).toBe(false)
  // The next press is a new start: the old refusal goes with it.
  fireEvent.click(f.start()!)
  expect(f.status()).toBe(STARTING)
  // CONTROL: a start the station takes says nothing of a refusal.
  f.answer({ value: f.controlling() })
  await f.advance(0)
  expect(f.status()).toBe("Waiting for the station's picture…")
})

it('names each refusal by what the operator can do: control off for this browser, another browser in control, or try again', async () => {
  for (const [code, said, phase] of [
    ['localPermissionRequired', PERMISSION, 'localPermissionRequired'],
    ['controllerBusy', OCCUPIED, 'occupied'],
    ['staleStation', REFUSED, 'available'],
    ['remoteBusy', REFUSED, 'available'],
  ] as const) {
    const f = fixture()
    await f.advance(500)
    fireEvent.click(f.start()!)
    f.answer({ error: code })
    await f.advance(0)
    expect(f.status(), `${code}: before the station's next state`).toBe(said)
    expect(f.status(), `${code}: never the bare wait`).not.toBe(WAITING)
    await f.advance(500)
    f.answer({ value: { ...f.available(), phase, allowed: phase !== 'localPermissionRequired' } })
    await f.advance(0)
    expect(f.status(), `${code}: once the station says where things stand`).toBe(said)
    cleanup(); clients.splice(0).forEach(c => c.disconnected()); vi.useRealTimers()
  }
})

it('a start refused at the page for a state gone stale says try again, never that control is off', async () => {
  const f = fixture()
  await f.advance(1000)
  expect(f.last().type, 'scene: a read out').toBe('state')
  // Its reply lands after the state's 1200 ms freshness window: held, but not current.
  await f.advance(1250)
  f.answer({ value: f.available() })
  await f.advance(0)
  expect(f.client.getSnapshot(), 'scene: a stale state, nothing in flight').toMatchObject({ fresh: false, busy: false })
  fireEvent.click(f.start()!)
  await f.advance(0)
  expect(f.acquires(), 'nothing is sent on a state that is not current').toHaveLength(0)
  expect(f.status()).toBe(REFUSED)
})
